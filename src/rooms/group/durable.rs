//! Private checkpoint/transaction boundary. No plaintext messages are serialized.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    version: u8,
    room: [u8; 32],
    owner: String,
    epoch: u64,
    consumed: Vec<Vec<u8>>,
    pending: Option<Pending>,
    #[serde(default)]
    outgoing: Vec<[u8; 32]>,
    #[serde(default)]
    applied: Option<super::super::control::raft::Position>,
}
struct Loaded {
    state: MlsGroup,
    members: Rc<Vec<(LeafNodeIndex, VerifiedMember)>>,
    checkpoint: Checkpoint,
}

impl Group {
    /// Distinguish an untouched candidate from lost/corrupt membership history.
    /// Legacy initialized stores remain restorable; an unmarked empty store is
    /// not permission to regenerate keys or retry a consumed Welcome.
    pub fn has_membership(actor: &CryptoMember) -> Result<bool> {
        let checkpoint = actor.provider.metadata::<Checkpoint>("group")?.is_some();
        let sdk = MlsGroup::load(
            actor.provider.storage(),
            &GroupId::from_slice(&actor.anchor.id()),
        )
        .map_err(|_| Error::Storage("cannot inspect existing MLS membership"))?
        .is_some();
        match actor.provider.metadata::<bool>("membership_started")? {
            Some(true) => Ok(true),
            Some(false) if !checkpoint && !sdk => Ok(false),
            None if checkpoint || sdk => Ok(true),
            _ => Err(Error::Storage(
                "membership initialization evidence is missing or inconsistent; verified recovery required",
            )),
        }
    }
    pub(super) fn ensure_fresh(actor: &CryptoMember) -> Result<()> {
        if actor.provider.metadata::<bool>("membership_started")? != Some(false)
            || actor.provider.metadata::<Checkpoint>("group")?.is_some()
            || MlsGroup::load(
                actor.provider.storage(),
                &GroupId::from_slice(&actor.anchor.id()),
            )
            .map_err(|_| Error::Storage("cannot inspect existing MLS state"))?
            .is_some()
        {
            return Err(Error::Storage(
                "membership is not proven fresh; restore instead of regenerating",
            ));
        }
        Ok(())
    }
    /// Restore with an already verified local client and room anchor. Missing or
    /// conflicting private state fails closed; this does not perform network rejoin.
    pub fn restore(actor: CryptoMember) -> Result<Self> {
        let loaded = Self::load(&actor)?;
        Ok(Self {
            actor,
            state: loaded.state,
            members: loaded.members,
            committed_epoch: loaded.checkpoint.epoch,
            outgoing: loaded.checkpoint.outgoing.into_iter().collect(),
            consumed: loaded.checkpoint.consumed.into_iter().collect(),
            pending: loaded.checkpoint.pending,
            poisoned: false,
            applied: loaded.checkpoint.applied,
        })
    }
    fn load(actor: &CryptoMember) -> Result<Loaded> {
        let checkpoint = actor
            .provider
            .metadata::<Checkpoint>("group")?
            .ok_or(Error::Storage(
                "room checkpoint is missing; explicit recovery required",
            ))?;
        if checkpoint.version != 1
            || checkpoint.room != actor.anchor.id()
            || checkpoint.owner != actor.owner
            || checkpoint.consumed.len() > config::ROOM_PACKAGE_REPLAY_LIMIT
            || checkpoint.consumed.iter().any(|value| value.len() != 32)
            || checkpoint.consumed.iter().collect::<BTreeSet<_>>().len()
                != checkpoint.consumed.len()
            || checkpoint
                .pending
                .as_ref()
                .and_then(|pending| pending.welcome.as_ref())
                .is_some_and(|wire| wire.len() > config::ROOM_MAX_WIRE_BYTES)
        {
            return Err(Error::Storage("invalid room checkpoint"));
        }
        super::outbox::validate(actor, &checkpoint.outgoing, 0)?;
        Self::load_prepared_control(actor, checkpoint.pending.as_ref(), checkpoint.epoch)?;
        let state = MlsGroup::load(
            actor.provider.storage(),
            &GroupId::from_slice(&actor.anchor.id()),
        )
        .map_err(|_| Error::Storage("cannot load private MLS state"))?
        .ok_or(Error::Storage(
            "private MLS state is missing; explicit recovery required",
        ))?;
        if state.epoch().as_u64() != checkpoint.epoch
            || state.pending_commit().is_some() != checkpoint.pending.is_some()
        {
            return Err(Error::Storage("room checkpoint and crypto state disagree"));
        }
        let members = verify_members(&actor.anchor, state.members())?;
        Self::verify_own_key(actor, &state)?;
        Ok(Loaded {
            state,
            members: Rc::new(members),
            checkpoint,
        })
    }
    pub(super) fn verify_own_key(actor: &CryptoMember, state: &MlsGroup) -> Result<()> {
        if state.is_active() {
            let own = state.own_leaf_node().ok_or(Error::Authentication)?;
            if own.signature_key().as_slice() != actor.signer.to_public_vec() {
                return Err(Error::Authentication);
            }
            if verify_credential(
                &actor.anchor,
                own.credential(),
                own.signature_key().as_slice(),
            )?
            .peer()
            .fingerprint()
                != actor.owner
            {
                return Err(Error::Authentication);
            }
        }
        Ok(())
    }
    pub(super) fn save_checkpoint(&self) -> Result<()> {
        let pending = self.pending.as_ref().map(|pending| Pending {
            digest: pending.digest,
            welcome: pending.welcome.clone(),
            proposal: pending.proposal,
        });
        self.actor.provider.set_metadata(
            "group",
            &Checkpoint {
                version: 1,
                room: self.actor.anchor.id(),
                owner: self.actor.owner.clone(),
                epoch: self.state.epoch().as_u64(),
                consumed: self.consumed.iter().cloned().collect(),
                outgoing: self.outgoing.iter().copied().collect(),
                pending,
                applied: self.applied,
            },
        )?;
        self.actor
            .provider
            .set_metadata("membership_started", &true)
    }
    pub(super) fn transactional<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let transaction = self.actor.provider.transaction()?;
        let prior_members = self.members.clone();
        let prior_epoch = self.committed_epoch;
        let prior_outgoing = self.outgoing.clone();
        let prior_applied = self.applied;
        let outcome = operation(self).and_then(|output| {
            self.save_checkpoint()?;
            Ok(output)
        });
        match outcome {
            Ok(output) => match transaction.commit() {
                Ok(()) => {
                    self.committed_epoch = self.state.epoch().as_u64();
                    Ok(output)
                }
                Err(error) => {
                    // Public status is the last durable view, never a dirty
                    // membership/epoch left in memory after failed COMMIT.
                    self.members = prior_members;
                    self.committed_epoch = prior_epoch;
                    self.outgoing = prior_outgoing;
                    self.applied = prior_applied;
                    self.poisoned = true;
                    Err(error)
                }
            },
            Err(error) => {
                // No effect has escaped this method. Discard dirty in-memory
                // ratchets as well as SQL writes, then reload the last checkpoint.
                drop(transaction);
                match Self::load(&self.actor) {
                    Ok(loaded) => {
                        self.state = loaded.state;
                        self.members = loaded.members;
                        self.committed_epoch = loaded.checkpoint.epoch;
                        self.outgoing = loaded.checkpoint.outgoing.into_iter().collect();
                        self.consumed = loaded.checkpoint.consumed.into_iter().collect();
                        self.pending = loaded.checkpoint.pending;
                        self.applied = loaded.checkpoint.applied;
                        self.poisoned = false;
                        Err(error)
                    }
                    Err(_) => {
                        self.members = prior_members;
                        self.committed_epoch = prior_epoch;
                        self.outgoing = prior_outgoing;
                        self.applied = prior_applied;
                        self.poisoned = true;
                        Err(Error::Storage("failed mutation requires verified recovery"))
                    }
                }
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests;
