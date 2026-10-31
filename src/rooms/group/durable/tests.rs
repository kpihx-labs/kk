use super::*;
use crate::{
    Identity,
    rooms::{Anchor, Grant, Rights, RootBundle},
};
use std::{fs, path::Path};

struct Pair {
    directory: tempfile::TempDir,
    identities: Vec<Identity>,
    anchor: Anchor,
    alice: Group,
    bob: Group,
}
fn load(root: &Path, identities: &[Identity], anchor: &Anchor, index: usize) -> Group {
    let actor = CryptoMember::open_private(
        &identities[index],
        anchor,
        &root.join(format!("state-{index}")),
    )
    .unwrap();
    Group::restore(actor).unwrap()
}

#[test]
fn candidate_marker_survives_restart_and_retained_activation_evidence_forbids_reset() {
    let directory = crate::test_support::tempdir().unwrap();
    let identity = Identity::create(&directory.path().join("identity")).unwrap();
    let anchor = RootBundle::create(&identity, "Activation marker fixture")
        .unwrap()
        .verify(&identity.peer())
        .unwrap();
    let state = directory.path().join("private");
    let candidate = CryptoMember::create_private(&identity, &anchor, vec![], &state).unwrap();
    let original = serde_json::to_vec(&candidate.public_identity().unwrap()).unwrap();
    assert!(!Group::has_membership(&candidate).unwrap());
    drop(candidate);
    let candidate = CryptoMember::open_private(&identity, &anchor, &state).unwrap();
    assert!(!Group::has_membership(&candidate).unwrap());
    assert_eq!(
        serde_json::to_vec(&candidate.public_identity().unwrap()).unwrap(),
        original
    );
    // Model missing group/checkpoint rows while retaining the independent
    // durable proof that membership was already initialized.
    let transaction = candidate.provider.transaction().unwrap();
    candidate
        .provider
        .set_metadata("membership_started", &true)
        .unwrap();
    transaction.commit().unwrap();
    assert!(Group::has_membership(&candidate).unwrap());
    assert!(Group::create(candidate).is_err());
    let candidate = CryptoMember::open_private(&identity, &anchor, &state).unwrap();
    assert!(Group::restore(candidate).is_err());
}
impl Pair {
    fn new() -> Self {
        let directory = crate::test_support::tempdir().unwrap();
        let root = directory.path();
        let identities: Vec<_> = (0..2)
            .map(|i| Identity::create(&root.join(format!("identity-{i}"))).unwrap())
            .collect();
        let bundle = RootBundle::create(&identities[0], "Private persistence fixture").unwrap();
        fs::write(root.join("anchor.json"), bundle.encode().unwrap()).unwrap();
        let anchor = bundle.verify(&identities[0].peer()).unwrap();
        let grant = Grant::issue(
            &anchor,
            &[],
            &identities[0],
            &identities[1].peer(),
            Rights::MODERATOR,
        )
        .unwrap();
        let actor =
            CryptoMember::create_private(&identities[0], &anchor, vec![], &root.join("state-0"))
                .unwrap();
        let mut alice = Group::create(actor).unwrap();
        let mut bob = CryptoMember::create_private(
            &identities[1],
            &anchor,
            vec![grant],
            &root.join("state-1"),
        )
        .unwrap();
        let package = bob.key_package().unwrap();
        drop(bob); // Init secrets must survive before Welcome is received.
        let commit = alice
            .prepare_add(alice.validate_package(&package).unwrap())
            .unwrap();
        drop(alice); // Prepared commit and withheld Welcome must survive too.
        let mut alice = load(root, &identities, &anchor, 0);
        assert!(alice.confirm_ordered(1, b"wrong record").is_err());
        let welcome = alice.confirm_ordered(1, &commit).unwrap().unwrap();
        let bob =
            CryptoMember::open_private(&identities[1], &anchor, &root.join("state-1")).unwrap();
        let bob = Group::join(bob, &welcome, &identities[0].peer(), 1).unwrap();
        Self {
            directory,
            identities,
            anchor,
            alice,
            bob,
        }
    }
}

#[test]
fn private_restart_preserves_sender_and_receiver_ratchets_and_pending_changes() {
    let mut pair = Pair::new();
    let first = pair.alice.encrypt_text("synthetic first", true).unwrap();
    assert_eq!(
        pair.bob.decrypt_text(&first).unwrap().content.text(),
        "synthetic first"
    );
    let Pair {
        directory,
        identities,
        anchor,
        alice,
        bob,
    } = pair;
    drop(alice);
    drop(bob);
    let mut alice = load(directory.path(), &identities, &anchor, 0);
    let mut bob = load(directory.path(), &identities, &anchor, 1);
    assert!(bob.decrypt_text(&first).is_err());
    let next = alice.encrypt_text("synthetic next", false).unwrap();
    assert_eq!(
        bob.decrypt_text(&next).unwrap().content.text(),
        "synthetic next"
    );
    drop(alice);
    let existing =
        CryptoMember::open_private(&identities[0], &anchor, &directory.path().join("state-0"))
            .unwrap();
    assert!(Group::create(existing).is_err());
    assert_eq!(load(directory.path(), &identities, &anchor, 0).epoch(), 1);
}

#[test]
fn package_budget_is_durable_and_missing_state_never_regenerates() {
    let directory = crate::test_support::tempdir().unwrap();
    let identity = Identity::create(&directory.path().join("identity")).unwrap();
    let anchor = RootBundle::create(&identity, "Budget fixture")
        .unwrap()
        .verify(&identity.peer())
        .unwrap();
    let path = directory.path().join("state");
    let mut actor = CryptoMember::create_private(&identity, &anchor, vec![], &path).unwrap();
    actor.key_package().unwrap();
    drop(actor);
    for _ in 1..config::ROOM_KEY_PACKAGE_BUDGET {
        let mut actor = CryptoMember::open_private(&identity, &anchor, &path).unwrap();
        actor.key_package().unwrap();
    }
    let mut actor = CryptoMember::open_private(&identity, &anchor, &path).unwrap();
    assert!(actor.key_package().is_err());
    assert!(Group::restore(actor).is_err()); // A candidate is not an existing room.
    let empty = directory.path().join("empty");
    drop(super::super::super::store::Provider::create(&empty).unwrap());
    assert!(CryptoMember::open_private(&identity, &anchor, &empty).is_err());
}

#[test]
fn crypto_commit_failure_exposes_no_ciphertext_and_requires_verified_reopen() {
    let mut pair = Pair::new();
    let first = pair.alice.encrypt_text("before failure", false).unwrap();
    pair.bob.decrypt_text(&first).unwrap();
    pair.alice.actor.provider.inject_checkpoint_commit_failure();
    assert!(matches!(
        pair.alice.encrypt_text("must not escape", true),
        Err(Error::Storage(_))
    ));
    assert!(!pair.alice.is_active());
    let Pair {
        directory,
        identities,
        anchor,
        alice,
        mut bob,
    } = pair;
    drop(alice);
    let mut alice = load(directory.path(), &identities, &anchor, 0);
    let next = alice.encrypt_text("after verified reopen", false).unwrap();
    assert_eq!(
        bob.decrypt_text(&next).unwrap().content.text(),
        "after verified reopen"
    );
}

#[test]
fn failed_membership_commit_keeps_the_last_durable_public_view() {
    let mut pair = Pair::new();
    let removed = pair.identities[1].peer().fingerprint();
    let commit = pair.alice.prepare_remove(&removed).unwrap();
    pair.alice.actor.provider.inject_checkpoint_commit_failure();
    assert!(matches!(
        pair.alice.confirm_ordered(2, &commit),
        Err(Error::Storage(_))
    ));
    assert_eq!(pair.alice.epoch(), 1);
    assert_eq!(pair.alice.peers().len(), 2);
    assert!(!pair.alice.is_active());
    let Pair {
        directory,
        identities,
        anchor,
        alice,
        mut bob,
    } = pair;
    drop(alice);
    let mut alice = load(directory.path(), &identities, &anchor, 0);
    assert_eq!(alice.epoch(), 1);
    alice.confirm_ordered(2, &commit).unwrap();
    bob.apply_ordered(2, &commit).unwrap();
    assert_eq!(alice.epoch(), 2);
    assert_eq!(alice.peers().len(), 1);
    assert!(!bob.is_active());
}

#[test]
fn failed_mutation_and_unavailable_recovery_keep_the_last_acknowledged_membership() {
    let mut pair = Pair::new();
    let prior = pair.alice.membership_digest().unwrap();
    let commit = pair
        .alice
        .prepare_remove(&pair.identities[1].peer().fingerprint())
        .unwrap();
    let result = pair.alice.transactional(|group| {
        group.confirm_ordered_mut(2, &commit)?;
        group.actor.provider.inject_recovery_quarantine();
        Err::<(), _>(Error::Authentication)
    });
    assert!(matches!(result, Err(Error::Storage(_))));
    assert_eq!(pair.alice.epoch(), 1);
    assert_eq!(pair.alice.peers().len(), 2);
    assert_eq!(pair.alice.membership_digest().unwrap(), prior);
    assert!(!pair.alice.is_active());
    assert!(pair.alice.prepared_control().is_err());
    let Pair {
        directory,
        identities,
        anchor,
        alice,
        mut bob,
    } = pair;
    drop(alice);
    let mut alice = load(directory.path(), &identities, &anchor, 0);
    assert_eq!(alice.peers().len(), 2);
    alice.confirm_ordered(2, &commit).unwrap();
    bob.apply_ordered(2, &commit).unwrap();
    assert_eq!(alice.epoch(), 2);
    assert!(!bob.is_active());
}
