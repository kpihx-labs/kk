//! Private MLS and applied public-log cursor share one durable transaction.
use super::*;
use crate::rooms::control::raft::{CommittedRecord, Position};

impl Group {
    pub fn applied_position(&self) -> Option<Position> {
        self.applied
    }

    /// The record must come from this device's protected committed control state.
    /// Epoch and logical audience are checked before private join COMMIT.
    pub fn join_committed(
        actor: CryptoMember,
        welcome: &[u8],
        approved_inviter: &Peer,
        record: &CommittedRecord,
    ) -> Result<Self> {
        Self::join_inner(
            actor,
            welcome,
            approved_inviter,
            record.epoch(),
            Some(record),
        )
    }

    pub(super) fn bind_join_record(
        &mut self,
        record: &CommittedRecord,
        inviter: &Peer,
    ) -> Result<()> {
        let (author, _, accepted) = record.change().ok_or(Error::Authentication)?;
        if !accepted
            || author != inviter.fingerprint_bytes()
            || record.room() != self.actor.anchor.id()
            || record
                .admitted_peer()
                .is_none_or(|peer| peer.to_string() != self.actor.owner)
            || record.epoch() != self.state.epoch().as_u64()
        {
            return Err(Error::Authentication);
        }
        self.check_transition_view(record.membership_digest())?;
        self.applied = Some(record.position()?);
        Ok(())
    }

    /// Blank, rejected and exact-retry entries advance the log cursor, not MLS.
    /// Exact replay of the already applied position is harmless; skips/forks fail.
    /// Returned Welcome is durably indexed by the existing local outbox.
    pub fn apply_committed(&mut self, record: &CommittedRecord) -> Result<Option<Vec<u8>>> {
        self.ready()?;
        if record.room() != self.actor.anchor.id() {
            return Err(Error::Authentication);
        }
        let position = record.position()?;
        if self.applied == Some(position) {
            if self.state.epoch().as_u64() != record.epoch() {
                return Err(Error::Authentication);
            }
            self.check_transition_view(record.membership_digest())?;
            return Ok(None);
        }
        if record.previous() != self.applied
            || self.applied.map_or(position.log.index != 0, |prior| {
                prior.log.index.checked_add(1) != Some(position.log.index)
            })
        {
            return Err(Error::Protocol);
        }
        self.transactional(|group| {
            let mut welcome = None;
            if let Some((author, commit, true)) = record.change() {
                if super::super::control::raft::PeerId(author).to_string() == group.actor.owner {
                    welcome = group.confirm_ordered_mut(record.epoch(), commit)?;
                } else {
                    // An authoritative competing epoch makes our old preparation
                    // impossible. A mere timeout/rejected forged record does not.
                    if group.pending.is_some() {
                        group
                            .state
                            .clear_pending_commit(group.actor.provider.storage())
                            .map_err(|_| Error::Authentication)?;
                        group.pending = None;
                        group.actor.provider.delete_blob("prepared_control")?;
                    }
                    group.apply_ordered_from(record.epoch(), commit, Some(author))?;
                }
            }
            if group.state.epoch().as_u64() != record.epoch() {
                return Err(Error::Authentication);
            }
            group.check_transition_view(record.membership_digest())?;
            group.applied = Some(position);
            Ok(welcome)
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{
        Identity,
        rooms::{
            Grant, Rights, RootBundle,
            control::{
                Proposal,
                raft::{LogStore, PeerId, StateMachine, TypeConfig},
            },
        },
    };
    use openraft::{CommittedLeaderId, Entry, EntryPayload, LogId, storage::RaftStateMachine};

    #[tokio::test]
    async fn failed_private_commit_releases_neither_welcome_nor_applied_position() {
        let directory = tempfile::tempdir().unwrap();
        let alice = Identity::create(&directory.path().join("alice")).unwrap();
        let bob = Identity::create(&directory.path().join("bob")).unwrap();
        let anchor = RootBundle::create(&alice, "Private cursor COMMIT fault")
            .unwrap()
            .verify(&alice.peer())
            .unwrap();
        let path = directory.path().join("private-a");
        let actor = CryptoMember::create_private(&alice, &anchor, vec![], &path).unwrap();
        let public = actor.public_identity().unwrap();
        let mut group = Group::create(actor).unwrap();
        let store = LogStore::create(&directory.path().join("control")).unwrap();
        let mut machine = StateMachine::create(store, &anchor, public).await.unwrap();
        let entry = |index, payload| Entry::<TypeConfig> {
            log_id: LogId::new(
                CommittedLeaderId::new(1, PeerId::from_peer(&alice.peer())),
                index,
            ),
            payload,
        };
        machine
            .apply([entry(0, EntryPayload::Blank)])
            .await
            .unwrap();
        let first = machine.committed_after(None).await.unwrap().remove(0);
        group.apply_committed(&first).unwrap();
        let before = group.applied_position();
        let grant = Grant::issue(&anchor, &[], &alice, &bob.peer(), Rights::PARTICIPANT).unwrap();
        let candidate_path = directory.path().join("private-b");
        let mut candidate =
            CryptoMember::create_private(&bob, &anchor, vec![grant], &candidate_path).unwrap();
        let package = candidate.key_package().unwrap();
        let commit = group
            .prepare_add(group.validate_package(&package).unwrap())
            .unwrap();
        let proposal =
            Proposal::add(&alice, anchor.id(), 0, anchor.id(), &commit, &package).unwrap();
        machine
            .apply([entry(1, EntryPayload::Normal(proposal))])
            .await
            .unwrap();
        let accepted = machine.committed_after(before).await.unwrap().remove(0);
        group.actor.provider.inject_checkpoint_commit_failure();
        assert!(matches!(
            group.apply_committed(&accepted),
            Err(Error::Storage(_))
        ));
        assert_eq!(group.applied_position(), before);
        assert_eq!(group.epoch(), 0);
        assert!(group.outgoing.is_empty());
        assert!(!group.is_active());
        drop(group);
        let actor = CryptoMember::open_private(&alice, &anchor, &path).unwrap();
        let mut restored = Group::restore(actor).unwrap();
        assert_eq!(restored.applied_position(), before);
        let welcome = restored.apply_committed(&accepted).unwrap().unwrap();
        assert_eq!(restored.epoch(), 1);
        assert_eq!(
            restored.applied_position(),
            Some(accepted.position().unwrap())
        );
        assert_eq!(restored.outgoing.len(), 1);
        // Wrong authority rolls MLS join back, preserving the candidate's init key.
        assert!(Group::join_committed(candidate, &welcome, &alice.peer(), &first).is_err());
        let candidate = CryptoMember::open_private(&bob, &anchor, &candidate_path).unwrap();
        let mut joined =
            Group::join_committed(candidate, &welcome, &alice.peer(), &accepted).unwrap();
        assert_eq!(joined.applied_position(), restored.applied_position());
        let removal = restored.prepare_remove(&bob.peer().fingerprint()).unwrap();
        assert!(
            joined
                .transactional(|group| group.apply_ordered_from(
                    2,
                    &removal,
                    Some(bob.peer().fingerprint_bytes())
                ))
                .is_err()
        );
        assert_eq!(joined.epoch(), 1);
        joined
            .transactional(|group| {
                group.apply_ordered_from(2, &removal, Some(alice.peer().fingerprint_bytes()))
            })
            .unwrap();
        assert!(!joined.is_active());
    }
}
