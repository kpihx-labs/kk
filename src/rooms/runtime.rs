//! Thread-confined private room orchestration. No implicit daemon or global trust.
//! Quorum acceptance is not recipient delivery; Welcome replication remains a
//! separate gate before claiming producer-independent completed admission.
use super::{
    control::{
        Proposal, View,
        raft::{ApplyResult, Position, service::ControlNode},
    },
    group::{Group, Text},
};
use crate::{
    Error, Identity, Peer, Result, config,
    contacts::{Action, Registry},
};
use std::{sync::Arc, time::Duration};
mod actor;
pub use actor::{Actor, Event, Handle, Publication, Status};

pub struct MediaPolicy {
    pub epoch: u64,
    pub inbound: std::collections::BTreeSet<super::control::raft::PeerId>,
    pub outbound: std::collections::BTreeSet<super::control::raft::PeerId>,
}

pub struct Room {
    identity: Arc<Identity>,
    control: Arc<ControlNode>,
    group: Group,
    profile: Registry,
}

impl Room {
    /// Own this value on its creator thread (or a Tokio LocalSet). The private
    /// provider is intentionally !Send; the public control server remains Send.
    pub fn new(
        identity: Arc<Identity>,
        control: Arc<ControlNode>,
        group: Group,
        profile: Registry,
    ) -> Result<Self> {
        if group.owner() != identity.peer().fingerprint()
            || control.id() != super::control::raft::PeerId::from_peer(&identity.peer())
            || group.room_id() != control.room_id()
        {
            return Err(Error::Authentication);
        }
        Ok(Self {
            identity,
            control,
            group,
            profile,
        })
    }
    /// Complete private activation from a recipient-authenticated committed
    /// replica. The original author certificate is explicitly approved; serving
    /// replicas do not replace that author or import a global contact.
    /// If a late error occurs after private join COMMIT, reopen the existing Group;
    /// never recreate a candidate or reset its consumed init key over that state.
    pub async fn join(
        identity: Arc<Identity>,
        control: Arc<ControlNode>,
        candidate: super::mls::CryptoMember,
        inviter: &Peer,
        profile: Registry,
        deadline: Duration,
    ) -> Result<Self> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        if candidate.owner != identity.peer().fingerprint()
            || candidate.anchor.id() != control.room_id()
            || control.id() != super::control::raft::PeerId::from_peer(&identity.peer())
        {
            return Err(Error::Authentication);
        }
        profile.authorize_peer(inviter, &identity, Action::Rooms, true)?;
        tokio::time::timeout(deadline, async {
            let (artifact, record) = control
                .receive_welcome(deadline)
                .await?
                .ok_or(Error::Node("room admission is not committed"))?;
            membership_policy(&profile, &identity, &control.public_view().await?.peers())?;
            let group = Group::join_committed(candidate, &artifact.wire, inviter, &record)?;
            let mut room = Self::new(identity, control, group, profile)?;
            room.synchronize(deadline).await?;
            membership_policy(&room.profile, &room.identity, &room.peers())?;
            Ok(room)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    pub fn room_id(&self) -> [u8; 32] {
        self.group.room_id()
    }
    pub fn root_bundle(&self) -> super::RootBundle {
        self.group.root_bundle()
    }
    pub fn epoch(&self) -> u64 {
        self.group.epoch()
    }
    pub fn applied_position(&self) -> Option<Position> {
        self.group.applied_position()
    }
    pub fn peers(&self) -> Vec<Peer> {
        self.group.peers()
    }
    pub fn is_active(&self) -> bool {
        self.group.is_active()
    }

    /// Records and the final public audience are read in one protected state
    /// operation AFTER a real quorum read barrier and local control application.
    pub async fn synchronize(&mut self, deadline: Duration) -> Result<View> {
        let (view, records) = self
            .control
            .fresh_catch_up(self.group.applied_position(), deadline)
            .await?;
        for record in records {
            // Welcome ciphertext is already in the durable private outbox.
            // Dropping this transient copy does not lose its deliverable.
            self.group.apply_committed(&record)?;
        }
        if !self.group.is_active() {
            return Err(Error::Revoked);
        }
        if self.group.epoch() != view.epoch()
            || self.group.membership_digest()? != view.membership_digest()?
        {
            return Err(Error::Authentication);
        }
        Ok(view)
    }

    fn authorize_audience(&self) -> Result<()> {
        // Group ciphertext is available to the ENTIRE epoch audience. Merely
        // skipping a revoked direct socket would not remove its group key.
        for peer in self.group.peers() {
            if peer.fingerprint() != self.group.owner() {
                self.profile
                    .authorize_peer(&peer, &self.identity, Action::Text, false)?;
                self.profile
                    .authorize_peer(&peer, &self.identity, Action::Rooms, false)?;
            }
        }
        Ok(())
    }
    fn sender(&self, sender: &Peer) -> Result<()> {
        if !self
            .group
            .peers()
            .iter()
            .any(|member| member.fingerprint_bytes() == sender.fingerprint_bytes())
        {
            return Err(Error::Denied);
        }
        self.profile
            .authorize_peer(sender, &self.identity, Action::Rooms, true)?;
        self.profile
            .authorize_peer(sender, &self.identity, Action::Text, true)
    }
    /// Direct file streams use their pinned TLS peer, not the group's text keys.
    /// Recheck current quorum membership, publisher rights and local file vetoes.
    pub async fn authorize_file(
        &mut self,
        peer: &Peer,
        inbound: bool,
        deadline: Duration,
    ) -> Result<()> {
        let view = self.synchronize(deadline).await?;
        self.stream_policy(&view, peer, inbound, Action::Files)
    }
    /// Authorize bidirectional profile metadata for a current private member.
    /// The caller must bind `peer` to the pinned TLS certificate and repeat this
    /// check immediately before disclosure and again when accepting a response.
    pub async fn authorize_profile_metadata(
        &mut self,
        peer: &Peer,
        deadline: Duration,
    ) -> Result<u64> {
        let view = self.synchronize(deadline).await?;
        let local = self.identity.peer().fingerprint_bytes();
        let remote = peer.fingerprint_bytes();
        if local == remote
            || !view
                .member_rights(&local)
                .is_some_and(|rights| rights.valid())
            || !view
                .member_rights(&remote)
                .is_some_and(|rights| rights.valid())
            || !self
                .group
                .peers()
                .iter()
                .any(|member| member.fingerprint_bytes() == remote)
        {
            return Err(Error::Denied);
        }
        let revision = self.profile.revision()?;
        self.profile
            .authorize_peer(peer, &self.identity, Action::Rooms, true)?;
        self.profile
            .authorize_peer(peer, &self.identity, Action::Rooms, false)?;
        if self.profile.revision()? != revision {
            return Err(Error::Denied);
        }
        Ok(view.epoch())
    }
    fn stream_policy(&self, view: &View, peer: &Peer, inbound: bool, action: Action) -> Result<()> {
        let publisher = if inbound {
            peer.fingerprint_bytes()
        } else {
            self.identity.peer().fingerprint_bytes()
        };
        if !view
            .member_rights(&publisher)
            .is_some_and(|rights| rights.can_publish())
            || view.member_rights(&peer.fingerprint_bytes()).is_none()
        {
            return Err(Error::Denied);
        }
        self.profile
            .authorize_peer(peer, &self.identity, Action::Rooms, inbound)?;
        self.profile
            .authorize_peer(peer, &self.identity, action, inbound)
    }
    /// One quorum-current policy for all active media streams on this device.
    /// Direct TLS recipients can be excluded individually, unlike MLS text.
    pub async fn media_policy(&mut self, deadline: Duration) -> Result<MediaPolicy> {
        let view = self.synchronize(deadline).await?;
        let revision = self.profile.revision()?;
        let mut policy = MediaPolicy {
            epoch: view.epoch(),
            inbound: Default::default(),
            outbound: Default::default(),
        };
        for peer in self
            .peers()
            .into_iter()
            .filter(|peer| peer.fingerprint_bytes() != self.identity.peer().fingerprint_bytes())
        {
            let id = super::control::raft::PeerId::from_peer(&peer);
            match self.stream_policy(&view, &peer, true, Action::Media) {
                Ok(()) => {
                    policy.inbound.insert(id);
                }
                Err(Error::Denied | Error::Revoked) => {}
                Err(error) => return Err(error),
            }
            match self.stream_policy(&view, &peer, false, Action::Media) {
                Ok(()) => {
                    policy.outbound.insert(id);
                }
                Err(Error::Denied | Error::Revoked) => {}
                Err(error) => return Err(error),
            }
        }
        if self.profile.revision()? != revision {
            return Err(Error::Denied);
        }
        Ok(policy)
    }
    pub async fn encrypt_text(
        &mut self,
        text: &str,
        sensitive: bool,
        deadline: Duration,
    ) -> Result<Vec<u8>> {
        if text.len() > config::MAX_MESSAGE_BYTES {
            return Err(Error::MessageTooLarge);
        }
        self.synchronize(deadline).await?;
        self.authorize_audience()?;
        let ciphertext = self.group.encrypt_text(text, sensitive)?;
        // A concurrent local policy update must not release the ciphertext. Its
        // generation remains durably consumed even when output is withheld.
        self.authorize_audience()?;
        Ok(ciphertext)
    }
    pub async fn decrypt_text(
        &mut self,
        sender: &Peer,
        wire: &[u8],
        deadline: Duration,
    ) -> Result<Text> {
        if wire.len() > config::ROOM_MAX_WIRE_BYTES {
            return Err(Error::Limit {
                resource: "room ciphertext",
                max: config::ROOM_MAX_WIRE_BYTES,
            });
        }
        self.synchronize(deadline).await?;
        self.sender(sender)?;
        let text = self.group.decrypt_text_from(wire, sender)?;
        self.sender(sender)?;
        Ok(text)
    }
    pub async fn prepare_admission(
        &mut self,
        package: &[u8],
        deadline: Duration,
    ) -> Result<Proposal> {
        self.prepare_admission_for(package, None, deadline).await
    }
    /// Bind enrollment to the device that proved its key in THIS room ceremony,
    /// not merely to a public KeyPackage copied from some other device/session.
    pub async fn prepare_scoped_admission(
        &mut self,
        approval: &crate::pairing::RoomPeer,
        package: &[u8],
        deadline: Duration,
    ) -> Result<Proposal> {
        self.validate_scope(approval)?;
        self.prepare_admission_for(package, Some(&approval.peer), deadline)
            .await
    }
    fn validate_scope(&self, approval: &crate::pairing::RoomPeer) -> Result<()> {
        if approval.room != self.room_id()
            || approval.local_identity != self.identity.peer().fingerprint()
        {
            return Err(Error::Authentication);
        }
        Ok(())
    }
    pub async fn issue_enrollment_authority(
        &mut self,
        approval: &crate::pairing::RoomPeer,
        rights: super::Rights,
        deadline: Duration,
    ) -> Result<super::enrollment::Authority> {
        self.validate_scope(approval)?;
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        tokio::time::timeout(deadline, async {
            self.synchronize(deadline).await?;
            let creator = self.control.public_origin().await?;
            let seeds = self.control.enrollment_seeds()?;
            self.profile
                .authorize_peer(&approval.peer, &self.identity, Action::Rooms, false)?;
            let authority = self.group.enrollment_authority(
                &self.identity,
                creator,
                &approval.peer,
                rights,
                seeds,
            )?;
            self.profile
                .authorize_peer(&approval.peer, &self.identity, Action::Rooms, false)?;
            Ok(authority)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    /// Replicate to this already-admitted, proved device without granting a vote.
    /// Local policy and private/public audience agreement remain backend-owned.
    pub async fn provision_scoped_learner(
        &mut self,
        approval: &crate::pairing::RoomPeer,
        node: super::control::raft::Node,
        deadline: Duration,
    ) -> Result<()> {
        self.validate_scope(approval)?;
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        tokio::time::timeout(deadline, async {
            self.synchronize(deadline).await?;
            self.profile
                .authorize_peer(&approval.peer, &self.identity, Action::Rooms, false)?;
            self.control
                .provision_learner(
                    super::control::raft::PeerId::from_peer(&approval.peer),
                    node,
                    deadline,
                )
                .await?;
            self.profile
                .authorize_peer(&approval.peer, &self.identity, Action::Rooms, false)?;
            Ok(())
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    /// Explicit operator-approved controller set. This never follows implicitly
    /// from a pairing, admission, data role or a cached status observation.
    pub async fn approve_controllers(
        &mut self,
        voters: std::collections::BTreeSet<super::control::raft::PeerId>,
        deadline: Duration,
    ) -> Result<openraft::LogId<super::control::raft::PeerId>> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        if voters.is_empty() || voters.len() > config::ROOM_MAX_MEMBERS {
            return Err(Error::Configuration(
                "controller selection must be nonempty and bounded",
            ));
        }
        tokio::time::timeout(deadline, async {
            let view = self.synchronize(deadline).await?;
            if !view
                .member_rights(&self.identity.peer().fingerprint_bytes())
                .is_some_and(|rights| rights.can_control())
            {
                return Err(Error::Denied);
            }
            for id in &voters {
                let peer = view
                    .peers()
                    .into_iter()
                    .find(|peer| peer.fingerprint_bytes() == id.0)
                    .ok_or(Error::Denied)?;
                self.profile
                    .authorize_peer(&peer, &self.identity, Action::Rooms, false)?;
            }
            let applied = self.control.set_voters(voters.clone(), deadline).await?;
            for peer in view.peers() {
                if voters.contains(&super::control::raft::PeerId::from_peer(&peer)) {
                    self.profile
                        .authorize_peer(&peer, &self.identity, Action::Rooms, false)?;
                }
            }
            Ok(applied)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    async fn prepare_admission_for(
        &mut self,
        package: &[u8],
        expected: Option<&Peer>,
        deadline: Duration,
    ) -> Result<Proposal> {
        let view = self.synchronize(deadline).await?;
        let candidate = self.group.validate_package(package)?;
        if expected.is_some_and(|peer| {
            peer.fingerprint_bytes() != candidate.member.peer().fingerprint_bytes()
        }) {
            return Err(Error::Authentication);
        }
        self.profile.authorize_peer(
            &candidate.member.peer(),
            &self.identity,
            Action::Rooms,
            false,
        )?;
        self.group.prepare_admission(&self.identity, &view, package)
    }
    pub async fn prepare_departure(&mut self, peer: &Peer, deadline: Duration) -> Result<Proposal> {
        let view = self.synchronize(deadline).await?;
        self.group.prepare_departure(&self.identity, &view, peer)
    }
    pub fn prepared_control(&self) -> Result<Option<Proposal>> {
        self.group.prepared_control()
    }

    /// Retry the persisted EXACT signed operation. An uncertain result leaves its
    /// pending crypto/nonce intact; this method never invents a replacement ID.
    /// Success here is public acceptance plus local application, NOT join delivery.
    pub async fn submit_prepared(&mut self, deadline: Duration) -> Result<ApplyResult> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        let proposal = self.group.prepared_control()?.ok_or(Error::Protocol)?;
        tokio::time::timeout(deadline, async {
            let receipt = self
                .control
                .submit_public(proposal.clone(), deadline)
                .await?;
            self.synchronize(deadline).await?;
            if !receipt.accepted && self.group.prepared_control()?.is_some() {
                self.group.reject_ordered(proposal.commit_bytes())?;
            }
            Ok(receipt)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    pub fn committed_welcome(&self, commit: &[u8]) -> Result<Option<Vec<u8>>> {
        self.group.committed_welcome(commit)
    }
    /// The caller must verify recipient delivery before clearing the local copy.
    pub fn acknowledge_welcome(&mut self, commit: &[u8]) -> Result<()> {
        self.group.acknowledge_welcome(commit)
    }
    pub fn into_group(self) -> Group {
        self.group
    }
}

fn membership_policy(profile: &Registry, identity: &Identity, peers: &[Peer]) -> Result<()> {
    for peer in peers {
        if peer.fingerprint_bytes() != identity.peer().fingerprint_bytes() {
            profile.authorize_peer(peer, identity, Action::Rooms, true)?;
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests;
