//! Bounded MLS state machine, not a network sequencer or durable room service.
//! Only a verified ordered-delivery owner may confirm/apply membership changes.
//! Global profile revocation is an additional runtime admission/effect boundary.

use super::{
    Member as VerifiedMember,
    mls::{Candidate, CryptoMember, SUITE, verify_credential},
};
use crate::{Error, Peer, ReceivedText, Result, config};
use openmls::prelude::*;
use openmls_traits::OpenMlsProvider;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, rc::Rc};
use tls_codec::{DeserializeBytes, Serialize as TlsSerialize};
use zeroize::Zeroizing;
mod durable;

const CONTROL_AAD: &[u8] = b"K&K room control v1";
const TEXT_AAD: &[u8] = b"K&K room text v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    digest: [u8; 32],
    welcome: Option<Vec<u8>>,
}

pub struct Group {
    actor: CryptoMember,
    state: MlsGroup,
    members: Rc<Vec<(LeafNodeIndex, VerifiedMember)>>,
    committed_epoch: u64,
    consumed: BTreeSet<Vec<u8>>,
    pending: Option<Pending>,
    poisoned: bool,
}

#[derive(Debug)]
pub struct Text {
    pub sender: String,
    pub content: ReceivedText,
}

impl Group {
    pub fn create(actor: CryptoMember) -> Result<Self> {
        let creator = verify_credential(
            &actor.anchor,
            &actor.credential.credential,
            actor.credential.signature_key.as_slice(),
        )?;
        if creator.peer().fingerprint() != actor.anchor.creator.fingerprint() {
            return Err(Error::Denied);
        }
        let config = MlsGroupCreateConfig::builder()
            .ciphersuite(SUITE)
            .wire_format_policy(PURE_CIPHERTEXT_WIRE_FORMAT_POLICY)
            .use_ratchet_tree_extension(true)
            .max_past_epochs(0)
            .build();
        let transaction = actor.provider.transaction()?;
        Self::ensure_fresh(&actor)?;
        let state = MlsGroup::new_with_group_id(
            &actor.provider,
            &actor.signer,
            &config,
            GroupId::from_slice(&actor.anchor.id()),
            actor.credential.clone(),
        )
        .map_err(|_| Error::Authentication)?;
        let group = Self::finish(actor, state)?;
        group.save_checkpoint()?;
        transaction.commit()?;
        Ok(group)
    }

    /// Welcome must already belong to an accepted ordered change and an explicitly
    /// approved inviter. A raw Welcome or wall clock is not membership authority.
    pub fn join(
        actor: CryptoMember,
        welcome: &[u8],
        approved_inviter: &Peer,
        committed_epoch: u64,
    ) -> Result<Self> {
        bound(welcome)?;
        let transaction = actor.provider.transaction()?;
        Self::ensure_fresh(&actor)?;
        let message =
            MlsMessageIn::tls_deserialize_exact_bytes(welcome).map_err(|_| Error::Protocol)?;
        let MlsMessageBodyIn::Welcome(welcome) = message.extract() else {
            return Err(Error::Protocol);
        };
        let config = MlsGroupJoinConfig::builder()
            .wire_format_policy(PURE_CIPHERTEXT_WIRE_FORMAT_POLICY)
            .use_ratchet_tree_extension(true)
            .max_past_epochs(0)
            .build();
        let staged = StagedWelcome::new_from_welcome(&actor.provider, &config, welcome, None)
            .map_err(|_| Error::Authentication)?;
        let inviter = staged.welcome_sender().map_err(|_| Error::Authentication)?;
        let inviter = verify_credential(
            &actor.anchor,
            inviter.credential(),
            inviter.signature_key().as_slice(),
        )?;
        if inviter.peer().fingerprint() != approved_inviter.fingerprint()
            || !inviter.rights().can_invite()
        {
            return Err(Error::Denied);
        }
        verify_members(&actor.anchor, staged.members())?;
        let state = staged
            .into_group(&actor.provider)
            .map_err(|_| Error::Authentication)?;
        if state.group_id().as_slice() != actor.anchor.id()
            || state.epoch().as_u64() != committed_epoch
        {
            return Err(Error::Authentication);
        }
        let group = Self::finish(actor, state)?;
        group.save_checkpoint()?;
        transaction.commit()?;
        Ok(group)
    }

    fn finish(actor: CryptoMember, state: MlsGroup) -> Result<Self> {
        Self::verify_own_key(&actor, &state)?;
        let members = verify_members(&actor.anchor, state.members())?;
        let committed_epoch = state.epoch().as_u64();
        Ok(Self {
            actor,
            state,
            members: Rc::new(members),
            committed_epoch,
            consumed: BTreeSet::new(),
            pending: None,
            poisoned: false,
        })
    }
    pub fn epoch(&self) -> u64 {
        self.committed_epoch
    }
    pub fn is_active(&self) -> bool {
        !self.poisoned && self.state.is_active()
    }
    pub fn peers(&self) -> Vec<Peer> {
        self.members
            .iter()
            .map(|(_, member)| member.peer())
            .collect()
    }
    pub fn validate_package(&self, wire: &[u8]) -> Result<Candidate> {
        self.ready()?;
        self.actor.validate_package(wire)
    }
    fn ready(&self) -> Result<()> {
        if self.poisoned {
            return Err(Error::Configuration(
                "room state requires verified recovery",
            ));
        }
        if !self.is_active() {
            return Err(Error::Revoked);
        }
        Ok(())
    }
    fn member(&self, index: LeafNodeIndex) -> Result<&VerifiedMember> {
        self.members
            .iter()
            .find(|(known, _)| *known == index)
            .map(|(_, member)| member)
            .ok_or(Error::Denied)
    }
    fn issuer(&self, candidate: &VerifiedMember) -> Result<()> {
        let issuer = self
            .members
            .iter()
            .find(|(_, member)| member.peer().fingerprint() == candidate.issuer)
            .map(|(_, member)| member)
            .ok_or(Error::Denied)?;
        if !issuer.rights().permits_issuance(candidate.rights()) {
            return Err(Error::Denied);
        }
        Ok(())
    }
    fn new_candidate(&self, candidate: &VerifiedMember, committer: &VerifiedMember) -> Result<()> {
        if !committer.rights().permits_issuance(candidate.rights()) {
            return Err(Error::Denied);
        }
        if self
            .members
            .iter()
            .any(|(_, known)| known.peer().fingerprint() == candidate.peer().fingerprint())
        {
            return Err(Error::Denied);
        }
        self.issuer(candidate)
    }

    /// Returns only the encrypted Commit. Welcome remains withheld until confirm.
    pub fn prepare_add(&mut self, candidate: Candidate) -> Result<Vec<u8>> {
        self.transactional(|group| group.prepare_add_mut(candidate))
    }
    fn prepare_add_mut(&mut self, candidate: Candidate) -> Result<Vec<u8>> {
        self.ready()?;
        if self.pending.is_some() || self.members.len() >= config::ROOM_MAX_MEMBERS {
            return Err(Error::Denied);
        }
        self.new_candidate(&candidate.member, self.member(self.state.own_leaf_index())?)?;
        if self.consumed.len() >= config::ROOM_PACKAGE_REPLAY_LIMIT {
            return Err(Error::Configuration("room package replay budget exhausted"));
        }
        if !self.consumed.insert(candidate.hash_reference(&self.actor)?) {
            return Err(Error::Denied);
        }
        self.state.set_aad(CONTROL_AAD.to_vec());
        let (commit, welcome, _) = self
            .state
            .add_members(
                &self.actor.provider,
                &self.actor.signer,
                &[candidate.package],
            )
            .map_err(|_| Error::Authentication)?;
        self.prepared(commit, Some(welcome))
    }
    pub fn prepare_remove(&mut self, fingerprint: &str) -> Result<Vec<u8>> {
        self.transactional(|group| group.prepare_remove_mut(fingerprint))
    }
    fn prepare_remove_mut(&mut self, fingerprint: &str) -> Result<Vec<u8>> {
        self.ready()?;
        if self.pending.is_some()
            || !self
                .member(self.state.own_leaf_index())?
                .rights()
                .can_remove()
        {
            return Err(Error::Denied);
        }
        let target = self
            .members
            .iter()
            .find(|(_, member)| member.peer().fingerprint() == fingerprint)
            .map(|(index, _)| *index)
            .ok_or(Error::Denied)?;
        if target == self.state.own_leaf_index() {
            return Err(Error::Denied);
        }
        self.state.set_aad(CONTROL_AAD.to_vec());
        let (commit, welcome, _) = self
            .state
            .remove_members(&self.actor.provider, &self.actor.signer, &[target])
            .map_err(|_| Error::Authentication)?;
        self.prepared(commit, welcome)
    }
    fn prepared(
        &mut self,
        commit: MlsMessageOut,
        welcome: Option<MlsMessageOut>,
    ) -> Result<Vec<u8>> {
        let encoded = (|| -> Result<_> {
            Ok((
                encode(&commit)?,
                welcome.map(|value| encode(&value)).transpose()?,
            ))
        })();
        let (wire, welcome) = encoded?;
        self.pending = Some(Pending {
            digest: Sha256::digest(&wire).into(),
            welcome,
        });
        Ok(wire)
    }

    /// Caller must verify authoritative ordered acceptance, not merely a timeout
    /// or one unverified peer ACK. This primitive does not implement that service.
    pub fn confirm_ordered(
        &mut self,
        next_epoch: u64,
        committed: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        self.transactional(|group| group.confirm_ordered_mut(next_epoch, committed))
    }
    fn confirm_ordered_mut(
        &mut self,
        next_epoch: u64,
        committed: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        self.ready()?;
        bound(committed)?;
        let digest: [u8; 32] = Sha256::digest(committed).into();
        if self.epoch().checked_add(1) != Some(next_epoch)
            || self
                .pending
                .as_ref()
                .is_none_or(|pending| pending.digest != digest)
        {
            return Err(Error::Protocol);
        }
        self.state
            .merge_pending_commit(&self.actor.provider)
            .map_err(|_| Error::Authentication)?;
        self.refresh()?;
        Ok(self.pending.take().and_then(|pending| pending.welcome))
    }

    /// Only after verified final rejection by the ordered-delivery owner.
    /// An uncertain timeout is not permission to roll a prepared change back.
    pub fn reject_ordered(&mut self, rejected: &[u8]) -> Result<()> {
        self.transactional(|group| group.reject_ordered_mut(rejected))
    }
    fn reject_ordered_mut(&mut self, rejected: &[u8]) -> Result<()> {
        self.ready()?;
        bound(rejected)?;
        let digest: [u8; 32] = Sha256::digest(rejected).into();
        if self
            .pending
            .as_ref()
            .is_none_or(|pending| pending.digest != digest)
        {
            return Err(Error::Protocol);
        }
        self.state
            .clear_pending_commit(self.actor.provider.storage())
            .map_err(|_| Error::Authentication)?;
        self.pending = None;
        Ok(())
    }

    /// Apply one already ordered membership record. Conflicting local preparation
    /// must first be resolved by the delivery owner, never silently overwritten.
    pub fn apply_ordered(&mut self, next_epoch: u64, wire: &[u8]) -> Result<()> {
        self.transactional(|group| group.apply_ordered_mut(next_epoch, wire))
    }
    fn apply_ordered_mut(&mut self, next_epoch: u64, wire: &[u8]) -> Result<()> {
        self.ready()?;
        if self.pending.is_some() || self.epoch().checked_add(1) != Some(next_epoch) {
            return Err(Error::Protocol);
        }
        let message = protocol(wire, ContentType::Commit)?;
        let processed = self
            .state
            .process_message(&self.actor.provider, message)
            .map_err(|_| Error::Authentication)?;
        if processed.aad() != CONTROL_AAD {
            return Err(Error::Protocol);
        }
        let sender = match processed.sender() {
            Sender::Member(index) => *index,
            _ => return Err(Error::Denied),
        };
        let committer = self.member(sender)?.clone();
        let ProcessedMessageContent::StagedCommitMessage(staged) = processed.into_content() else {
            return Err(Error::Protocol);
        };
        self.validate_change(&staged, sender, &committer)?;
        self.state
            .merge_staged_commit(&self.actor.provider, *staged)
            .map_err(|_| Error::Authentication)?;
        self.refresh()
    }
    fn validate_change(
        &self,
        staged: &StagedCommit,
        sender: LeafNodeIndex,
        committer: &VerifiedMember,
    ) -> Result<()> {
        let adds = staged.add_proposals().count();
        let removes = staged.remove_proposals().count();
        // Initially one add OR remove per record. Unsupported batching is refused.
        if adds + removes > 1 || self.members.len() + adds - removes > config::ROOM_MAX_MEMBERS {
            return Err(Error::Denied);
        }
        for queued in staged.queued_proposals() {
            let proposer = match queued.sender() {
                Sender::Member(index) => self.member(*index)?,
                _ => return Err(Error::Denied),
            };
            match queued.proposal() {
                Proposal::Add(add) => {
                    let leaf = add.key_package().leaf_node();
                    let candidate = verify_credential(
                        &self.actor.anchor,
                        leaf.credential(),
                        leaf.signature_key().as_slice(),
                    )?;
                    self.new_candidate(&candidate, proposer)?;
                    self.new_candidate(&candidate, committer)?;
                }
                Proposal::Remove(remove) => {
                    if !proposer.rights().can_remove() || !committer.rights().can_remove() {
                        return Err(Error::Denied);
                    }
                    self.member(remove.removed())?;
                }
                Proposal::Update(update) => {
                    let member = verify_credential(
                        &self.actor.anchor,
                        update.leaf_node().credential(),
                        update.leaf_node().signature_key().as_slice(),
                    )?;
                    same_identity(&member, proposer)?;
                }
                _ => return Err(Error::Denied),
            }
        }
        if let Some(leaf) = staged.update_path_leaf_node() {
            let member = verify_credential(
                &self.actor.anchor,
                leaf.credential(),
                leaf.signature_key().as_slice(),
            )?;
            same_identity(&member, self.member(sender)?)?;
        }
        Ok(())
    }
    fn refresh(&mut self) -> Result<()> {
        match verify_members(&self.actor.anchor, self.state.members()) {
            Ok(members) => {
                self.members = Rc::new(members);
                Ok(())
            }
            Err(error) => {
                self.poisoned = true;
                Err(error)
            }
        }
    }

    pub fn encrypt_text(&mut self, text: &str, sensitive: bool) -> Result<Vec<u8>> {
        self.transactional(|group| group.encrypt_text_mut(text, sensitive))
    }
    fn encrypt_text_mut(&mut self, text: &str, sensitive: bool) -> Result<Vec<u8>> {
        self.ready()?;
        if self.pending.is_some()
            || !self
                .member(self.state.own_leaf_index())?
                .rights()
                .can_publish()
        {
            return Err(Error::Denied);
        }
        if text.len() > config::MAX_MESSAGE_BYTES {
            return Err(Error::MessageTooLarge);
        }
        let mut plaintext = Zeroizing::new(Vec::with_capacity(1 + text.len()));
        plaintext.push(u8::from(sensitive));
        plaintext.extend_from_slice(text.as_bytes());
        self.state.set_aad(TEXT_AAD.to_vec());
        let message = self
            .state
            .create_message(&self.actor.provider, &self.actor.signer, &plaintext)
            .map_err(|_| Error::Authentication)?;
        encode(&message)
    }
    pub fn decrypt_text(&mut self, wire: &[u8]) -> Result<Text> {
        self.transactional(|group| group.decrypt_text_mut(wire))
    }
    fn decrypt_text_mut(&mut self, wire: &[u8]) -> Result<Text> {
        self.ready()?;
        let message = protocol(wire, ContentType::Application)?;
        let processed = self
            .state
            .process_message(&self.actor.provider, message)
            .map_err(|_| Error::Authentication)?;
        if processed.aad() != TEXT_AAD {
            return Err(Error::Protocol);
        }
        let sender = match processed.sender() {
            Sender::Member(index) => self.member(*index)?,
            _ => return Err(Error::Denied),
        };
        if !sender.rights().can_publish() {
            return Err(Error::Denied);
        }
        let fingerprint = sender.peer().fingerprint();
        let ProcessedMessageContent::ApplicationMessage(message) = processed.into_content() else {
            return Err(Error::Protocol);
        };
        let plaintext = Zeroizing::new(message.into_bytes());
        let (flag, bytes) = plaintext.split_first().ok_or(Error::Protocol)?;
        if *flag > 1 {
            return Err(Error::Protocol);
        }
        Ok(Text {
            sender: fingerprint,
            content: ReceivedText::from_bytes(bytes, *flag == 1)?,
        })
    }
}

fn same_identity(new: &VerifiedMember, old: &VerifiedMember) -> Result<()> {
    if new.peer().fingerprint() != old.peer().fingerprint() || new.grant_id() != old.grant_id() {
        return Err(Error::Denied);
    }
    Ok(())
}
fn verify_members(
    anchor: &super::Anchor,
    members: impl Iterator<Item = openmls::group::Member>,
) -> Result<Vec<(LeafNodeIndex, VerifiedMember)>> {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for member in members {
        if result.len() >= config::ROOM_MAX_MEMBERS {
            return Err(Error::Denied);
        }
        let verified = verify_credential(anchor, &member.credential, &member.signature_key)?;
        if !seen.insert(verified.peer().fingerprint()) {
            return Err(Error::Denied);
        }
        result.push((member.index, verified));
    }
    if result.is_empty() {
        return Err(Error::Protocol);
    }
    Ok(result)
}
fn bound(bytes: &[u8]) -> Result<()> {
    if bytes.len() > config::ROOM_MAX_WIRE_BYTES {
        return Err(Error::Limit {
            resource: "room frame",
            max: config::ROOM_MAX_WIRE_BYTES,
        });
    }
    Ok(())
}
fn encode(message: &MlsMessageOut) -> Result<Vec<u8>> {
    let bytes = message
        .tls_serialize_detached()
        .map_err(|_| Error::Protocol)?;
    bound(&bytes)?;
    Ok(bytes)
}
fn protocol(wire: &[u8], kind: ContentType) -> Result<ProtocolMessage> {
    bound(wire)?;
    let message = MlsMessageIn::tls_deserialize_exact_bytes(wire)
        .map_err(|_| Error::Protocol)?
        .try_into_protocol_message()
        .map_err(|_| Error::Protocol)?;
    if message.content_type() != kind {
        return Err(Error::Protocol);
    }
    Ok(message)
}

#[cfg(all(test, unix))]
mod tests;
