use super::*;
use crate::{
    Identity,
    rooms::{Grant, Rights, RootBundle},
};

#[test]
fn committed_welcome_survives_crash_gap_and_verified_delivery_ack() {
    let directory = crate::test_support::tempdir().unwrap();
    let alice = Identity::create(&directory.path().join("alice")).unwrap();
    let bob = Identity::create(&directory.path().join("bob")).unwrap();
    let anchor = RootBundle::create(&alice, "Welcome outbox fixture")
        .unwrap()
        .verify(&alice.peer())
        .unwrap();
    let grant = Grant::issue(&anchor, &[], &alice, &bob.peer(), Rights::PARTICIPANT).unwrap();
    let path = directory.path().join("alice-state");
    let actor = CryptoMember::create_private(&alice, &anchor, vec![], &path).unwrap();
    let mut group = Group::create(actor).unwrap();
    let mut candidate = CryptoMember::new(&bob, &anchor, vec![grant]).unwrap();
    let package = candidate.key_package().unwrap();
    let commit = group
        .prepare_add(group.validate_package(&package).unwrap())
        .unwrap();
    assert!(group.committed_welcome(&commit).unwrap().is_none());
    let returned = group.confirm_ordered(1, &commit).unwrap().unwrap();
    drop(group); // COMMIT succeeded, but no delivery happened before the owner stopped.
    let actor = CryptoMember::open_private(&alice, &anchor, &path).unwrap();
    let mut group = Group::restore(actor).unwrap();
    let replayed = group.committed_welcome(&commit).unwrap().unwrap();
    assert!(replayed == returned);
    let joined = Group::join(candidate, &replayed, &alice.peer(), 1).unwrap();
    assert_eq!(joined.epoch(), 1);
    group.acknowledge_welcome(&commit).unwrap(); // Controlled verified unit delivery.
    drop(group);
    let group =
        Group::restore(CryptoMember::open_private(&alice, &anchor, &path).unwrap()).unwrap();
    assert!(group.committed_welcome(&commit).unwrap().is_none());
}

#[test]
fn failed_confirmation_does_not_create_a_deliverable_and_can_recover() {
    let directory = crate::test_support::tempdir().unwrap();
    let alice = Identity::create(&directory.path().join("alice")).unwrap();
    let bob = Identity::create(&directory.path().join("bob")).unwrap();
    let anchor = RootBundle::create(&alice, "Failed outbox fixture")
        .unwrap()
        .verify(&alice.peer())
        .unwrap();
    let grant = Grant::issue(&anchor, &[], &alice, &bob.peer(), Rights::READER).unwrap();
    let path = directory.path().join("alice-state");
    let mut group =
        Group::create(CryptoMember::create_private(&alice, &anchor, vec![], &path).unwrap())
            .unwrap();
    let mut candidate = CryptoMember::new(&bob, &anchor, vec![grant]).unwrap();
    let package = candidate.key_package().unwrap();
    let commit = group
        .prepare_add(group.validate_package(&package).unwrap())
        .unwrap();
    group.actor.provider.inject_checkpoint_commit_failure();
    assert!(group.confirm_ordered(1, &commit).is_err());
    assert!(group.committed_welcome(&commit).is_err());
    drop(group);
    let mut group =
        Group::restore(CryptoMember::open_private(&alice, &anchor, &path).unwrap()).unwrap();
    assert!(group.committed_welcome(&commit).unwrap().is_none());
    group.confirm_ordered(1, &commit).unwrap();
    assert!(group.committed_welcome(&commit).unwrap().is_some());
}
