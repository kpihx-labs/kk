use super::*;
use crate::{
    Identity,
    rooms::{RootBundle, mls::CryptoMember},
};
use openraft::testing::{StoreBuilder, Suite};

struct Builder {
    anchor: Anchor,
    public: PublicIdentity,
}
impl Builder {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let identity = Identity::create(&directory.path().join("identity")).unwrap();
        let anchor = RootBundle::create(&identity, "Public storage conformance fixture")
            .unwrap()
            .verify(&identity.peer())
            .unwrap();
        let public = {
            let actor = CryptoMember::new(&identity, &anchor, vec![]).unwrap();
            actor.public_identity().unwrap()
        };
        Self { anchor, public }
    }
}
impl StoreBuilder<TypeConfig, LogStore, StateMachine, tempfile::TempDir> for Builder {
    async fn build(&self) -> Stored<(tempfile::TempDir, LogStore, StateMachine)> {
        let directory = tempfile::tempdir()
            .map_err(|_| *failure(ErrorVerb::Write, "cannot create test directory"))?;
        let store = LogStore::create(&directory.path().join("public-control")).unwrap();
        let machine = StateMachine::create(store.clone(), &self.anchor, self.public.clone())
            .await
            .map_err(|error| *error)?;
        Ok((directory, store, machine))
    }
}

#[test]
fn upstream_raft_storage_conformance_suite() {
    Suite::<TypeConfig, LogStore, StateMachine, Builder, tempfile::TempDir>::test_all(
        Builder::new(),
    )
    .unwrap();
}

#[tokio::test]
async fn public_snapshots_are_hashed_bounded_and_restorable_without_private_state() {
    let (_guard, store, mut machine) = Builder::new().build().await.unwrap();
    let snapshot = machine.build_snapshot().await.unwrap();
    let meta = snapshot.meta.clone();
    let bytes = (*snapshot.snapshot).into_bytes();
    let public_text = std::str::from_utf8(&bytes).unwrap();
    assert!(!public_text.contains("private.sqlite3"));
    assert!(!public_text.contains("private_signing_key"));
    machine
        .install_snapshot(&meta, Box::new(SnapshotBuffer::new(bytes.clone()).unwrap()))
        .await
        .unwrap();
    assert!(machine.get_current_snapshot().await.unwrap().is_some());
    // A matching outer digest does not make unknown versions or inconsistent
    // applied positions valid. Reject BEFORE replacing the protected database.
    for kind in 0..3 {
        let mut state: State = decode(&bytes).unwrap();
        match kind {
            0 => state.version = 1,
            1 => {
                state.position = Some(Position {
                    log: LogId::new(openraft::CommittedLeaderId::new(99, PeerId([7; 32])), 99),
                    hash: [3; 32],
                })
            }
            _ => {
                let mut origin = serde_json::to_value(&state.origin).unwrap();
                let first = origin["signing_key"][0].as_u64().unwrap();
                origin["signing_key"][0] = (first ^ 1).into();
                state.origin = serde_json::from_value(origin).unwrap();
            }
        }
        let data = encode(&state).unwrap();
        let mut changed = meta.clone();
        changed.snapshot_id = snapshot_id(&data);
        assert!(
            machine
                .install_snapshot(&changed, Box::new(SnapshotBuffer::new(data).unwrap()))
                .await
                .is_err()
        );
        assert_eq!(machine.public_view().await.unwrap().epoch(), 0);
    }
    let mut wrong = meta;
    wrong.snapshot_id = "not-the-hash".to_owned();
    assert!(
        machine
            .install_snapshot(&wrong, Box::new(SnapshotBuffer::new(bytes).unwrap()))
            .await
            .is_err()
    );
    let reopened = StateMachine::open(store, &machine.anchor).await.unwrap();
    assert_eq!(reopened.public_view().await.unwrap().epoch(), 0);
}

#[tokio::test]
async fn exact_retry_recovers_original_public_receipt_without_reapplying_or_promoting_forgery() {
    use super::super::super::Proposal;
    use crate::rooms::{Grant, Rights};
    use openraft::{CommittedLeaderId, Entry, EntryPayload};
    let directory = tempfile::tempdir().unwrap();
    let alice = Identity::create(&directory.path().join("alice")).unwrap();
    let bob = Identity::create(&directory.path().join("bob")).unwrap();
    let anchor = RootBundle::create(&alice, "Receipt retry fixture")
        .unwrap()
        .verify(&alice.peer())
        .unwrap();
    let creator = CryptoMember::new(&alice, &anchor, vec![])
        .unwrap()
        .public_identity()
        .unwrap();
    let grant = Grant::issue(&anchor, &[], &alice, &bob.peer(), Rights::MODERATOR).unwrap();
    let mut candidate = CryptoMember::new(&bob, &anchor, vec![grant]).unwrap();
    let package = candidate.key_package().unwrap();
    let path = directory.path().join("public-control");
    let store = LogStore::create(&path).unwrap();
    let mut machine = StateMachine::create(store.clone(), &anchor, creator)
        .await
        .unwrap();
    let proposal = Proposal::add(
        &alice,
        anchor.id(),
        0,
        anchor.id(),
        b"opaque public receipt fixture",
        &package,
    )
    .unwrap();
    let entry = |index, proposal| Entry::<TypeConfig> {
        log_id: LogId::new(
            CommittedLeaderId::new(1, PeerId::from_peer(&alice.peer())),
            index,
        ),
        payload: EntryPayload::Normal(proposal),
    };
    let original = machine
        .apply([entry(0, proposal.clone())])
        .await
        .unwrap()
        .remove(0);
    assert!(original.accepted && !original.replayed);
    let view = machine.public_view().await.unwrap();
    let remove = Proposal::remove(
        &bob,
        anchor.id(),
        1,
        view.head(),
        b"opaque public removal",
        &alice.peer(),
    )
    .unwrap();
    machine.apply([entry(1, remove)]).await.unwrap();
    drop(machine);
    drop(store);
    let store = LogStore::open(&path).unwrap();
    let mut machine = StateMachine::open(store, &anchor).await.unwrap();
    let replay = machine
        .apply([entry(2, proposal.clone())])
        .await
        .unwrap()
        .remove(0);
    assert!(replay.accepted && replay.replayed);
    assert_eq!(replay.first_log, original.first_log);
    assert_eq!(replay.epoch, original.epoch); // Historical outcome, not current view.
    assert_eq!(machine.public_view().await.unwrap().epoch(), 2);
    assert_eq!(machine.public_view().await.unwrap().peers().len(), 1);
    let mut forged = proposal;
    forged.signature = vec![0; 64].into();
    let rejected = machine.apply([entry(3, forged)]).await.unwrap().remove(0);
    assert!(!rejected.accepted);
    assert_eq!(machine.public_view().await.unwrap().epoch(), 2);
}

#[tokio::test]
async fn private_crypto_cursor_tracks_each_actual_record_not_a_historical_receipt() {
    use crate::rooms::{Grant, Rights, group::Group};
    use openraft::{CommittedLeaderId, Entry, EntryPayload};
    let directory = tempfile::tempdir().unwrap();
    let alice = Identity::create(&directory.path().join("alice")).unwrap();
    let bob = Identity::create(&directory.path().join("bob")).unwrap();
    let anchor = RootBundle::create(&alice, "Atomic private applied records")
        .unwrap()
        .verify(&alice.peer())
        .unwrap();
    let actor =
        CryptoMember::create_private(&alice, &anchor, vec![], &directory.path().join("private-a"))
            .unwrap();
    let creator = actor.public_identity().unwrap();
    let mut group = Group::create(actor).unwrap();
    let grant = Grant::issue(&anchor, &[], &alice, &bob.peer(), Rights::MODERATOR).unwrap();
    let mut candidate = CryptoMember::create_private(
        &bob,
        &anchor,
        vec![grant.clone()],
        &directory.path().join("private-b"),
    )
    .unwrap();
    let package = candidate.key_package().unwrap();
    let store = LogStore::create(&directory.path().join("public")).unwrap();
    let mut machine = StateMachine::create(store, &anchor, creator).await.unwrap();
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
    let initial = group.applied_position();
    let proposal = group
        .prepare_admission(&alice, &machine.public_view().await.unwrap(), &package)
        .unwrap();
    let exact = proposal.encode().unwrap();
    drop(group);
    let actor =
        CryptoMember::open_private(&alice, &anchor, &directory.path().join("private-a")).unwrap();
    let group = Group::restore(actor).unwrap();
    assert_eq!(
        group.prepared_control().unwrap().unwrap().encode().unwrap(),
        exact
    );
    machine
        .apply([entry(1, EntryPayload::Normal(proposal.clone()))])
        .await
        .unwrap();
    let add = machine.committed_after(initial).await.unwrap().remove(0);
    // Admission ciphertext is already in protected replicated public state,
    // before the private producer confirms or exports its local outbox.
    drop(group);
    let snapshot = machine.build_snapshot().await.unwrap();
    machine
        .install_snapshot(&snapshot.meta, snapshot.snapshot)
        .await
        .unwrap();
    let replicated = machine
        .public_view()
        .await
        .unwrap()
        .welcome_for(&bob.peer().fingerprint_bytes())
        .unwrap();
    assert_eq!(replicated.proposal, proposal.id().unwrap());
    assert_eq!(add.proposal_id(), Some(replicated.proposal));
    assert_eq!(replicated.recipient, bob.peer().fingerprint_bytes());
    let mut joined =
        Group::join_committed(candidate, &replicated.wire, &alice.peer(), &add).unwrap();
    let actor =
        CryptoMember::open_private(&alice, &anchor, &directory.path().join("private-a")).unwrap();
    let mut group = Group::restore(actor).unwrap();
    let welcome = group.apply_committed(&add).unwrap().unwrap();
    assert_eq!(welcome, replicated.wire);
    assert!(group.prepared_control().unwrap().is_none());
    assert!(group.apply_committed(&add).unwrap().is_none());
    assert_eq!(joined.applied_position(), group.applied_position());
    let text = joined.encrypt_text("actual joined member", false).unwrap();
    assert!(group.decrypt_text(&text).is_ok());
    let before_removal = group.applied_position();
    let view = machine.public_view().await.unwrap();
    let removal = group.prepare_departure(&alice, &view, &bob.peer()).unwrap();
    machine
        .apply([entry(2, EntryPayload::Normal(removal))])
        .await
        .unwrap();
    let mut forged = proposal.clone();
    forged.signature = vec![0; 64].into();
    machine
        .apply([
            entry(3, EntryPayload::Normal(proposal)),
            entry(4, EntryPayload::Normal(forged)),
            entry(5, EntryPayload::Blank),
        ])
        .await
        .unwrap();
    let records = machine.committed_after(before_removal).await.unwrap();
    assert_eq!(records.len(), 4);
    // The retried add's receipt names epoch 1, but its actual log record is epoch 2.
    assert!(records.iter().all(|record| record.epoch() == 2));
    assert!(group.apply_committed(&records[1]).is_err()); // Cannot skip removal.
    for record in &records {
        group.apply_committed(record).unwrap();
    }
    assert_eq!(group.epoch(), 2);
    assert!(
        machine
            .public_view()
            .await
            .unwrap()
            .welcome_for(&bob.peer().fingerprint_bytes())
            .is_none()
    );
    let final_position = group.applied_position();
    assert_eq!(final_position.unwrap().log.index, 5);
    assert!(group.apply_committed(&first).is_err()); // Cannot rewind.
    drop(group);
    let actor =
        CryptoMember::open_private(&alice, &anchor, &directory.path().join("private-a")).unwrap();
    let restored = Group::restore(actor).unwrap();
    assert_eq!(restored.applied_position(), final_position);
    assert_eq!(restored.epoch(), 2);
    assert!(
        machine
            .committed_after(final_position)
            .await
            .unwrap()
            .is_empty()
    );
    let snapshot = machine.build_snapshot().await.unwrap();
    machine
        .install_snapshot(&snapshot.meta, snapshot.snapshot)
        .await
        .unwrap();
    assert!(machine.committed_after(initial).await.is_err()); // Snapshot is not MLS history.
    assert!(
        machine
            .committed_after(final_position)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn recipient_artifact_lookup_survives_genesis_eviction_but_never_invents_missing_admission_proof()
 {
    use crate::rooms::{Grant, Rights, control::Proposal};
    use openraft::{CommittedLeaderId, Entry, EntryPayload};
    let directory = tempfile::tempdir().unwrap();
    let alice = Identity::create(&directory.path().join("alice")).unwrap();
    let bob = Identity::create(&directory.path().join("bob")).unwrap();
    let anchor = RootBundle::create(&alice, "Bounded admission lookup")
        .unwrap()
        .verify(&alice.peer())
        .unwrap();
    let creator = CryptoMember::new(&alice, &anchor, vec![])
        .unwrap()
        .public_identity()
        .unwrap();
    let grant = Grant::issue(&anchor, &[], &alice, &bob.peer(), Rights::PARTICIPANT).unwrap();
    let mut candidate = CryptoMember::new(&bob, &anchor, vec![grant]).unwrap();
    let package = candidate.key_package().unwrap();
    // Public lookup/storage fixture only; these bytes are not an MLS join proof.
    let proposal = Proposal::add_with_welcome(
        &alice,
        anchor.id(),
        0,
        anchor.id(),
        b"opaque fixture",
        &package,
        b"encrypted fixture",
    )
    .unwrap();
    let store = LogStore::create(&directory.path().join("public")).unwrap();
    let mut machine = StateMachine::create(store, &anchor, creator).await.unwrap();
    let entry = |index, payload| Entry::<TypeConfig> {
        log_id: LogId::new(
            CommittedLeaderId::new(1, PeerId::from_peer(&alice.peer())),
            index,
        ),
        payload,
    };
    machine
        .apply([
            entry(0, EntryPayload::Blank),
            entry(1, EntryPayload::Normal(proposal)),
        ])
        .await
        .unwrap();
    machine
        .apply(
            (2..=config::ROOM_APPLIED_RECORDS as u64)
                .map(|index| entry(index, EntryPayload::Blank)),
        )
        .await
        .unwrap();
    assert!(machine.committed_after(None).await.is_err()); // Genesis was evicted.
    let (_, record) = machine
        .committed_welcome(PeerId::from_peer(&bob.peer()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.position().unwrap().log.index, 1);
    assert!(
        machine
            .committed_welcome(PeerId::from_peer(&alice.peer()))
            .await
            .unwrap()
            .is_none()
    );
    machine
        .apply([entry(
            config::ROOM_APPLIED_RECORDS as u64 + 1,
            EntryPayload::Blank,
        )])
        .await
        .unwrap();
    assert!(
        machine
            .public_view()
            .await
            .unwrap()
            .welcome_for(&bob.peer().fingerprint_bytes())
            .is_some()
    );
    assert!(
        machine
            .committed_welcome(PeerId::from_peer(&bob.peer()))
            .await
            .is_err()
    );
}
