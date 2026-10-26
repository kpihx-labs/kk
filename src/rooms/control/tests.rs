use super::*;
use crate::rooms::{Anchor, Grant, Rights, RootBundle, mls::CryptoMember};

fn fixture() -> (
    tempfile::TempDir,
    Vec<Identity>,
    Anchor,
    CryptoMember,
    CryptoMember,
    View,
) {
    let directory = tempfile::tempdir().unwrap();
    let identities: Vec<_> = (0..3)
        .map(|i| Identity::create(&directory.path().join(format!("control-{i}"))).unwrap())
        .collect();
    let anchor = RootBundle::create(&identities[0], "Public control fixture")
        .unwrap()
        .verify(&identities[0].peer())
        .unwrap();
    let grant = Grant::issue(
        &anchor,
        &[],
        &identities[0],
        &identities[1].peer(),
        Rights::MODERATOR,
    )
    .unwrap();
    let alice = CryptoMember::new(&identities[0], &anchor, vec![]).unwrap();
    let bob = CryptoMember::new(&identities[1], &anchor, vec![grant]).unwrap();
    let view = View::genesis(&anchor, alice.public_identity().unwrap()).unwrap();
    (directory, identities, anchor, alice, bob, view)
}

#[test]
fn signer_context_payload_and_codec_are_bound_without_a_quorum_claim() {
    let (_directory, identities, anchor, _alice, mut bob, view) = fixture();
    let package = bob.key_package().unwrap();
    let proposal = Proposal::add(
        &identities[0],
        anchor.id(),
        0,
        view.head(),
        b"synthetic-encrypted-commit",
        &package,
    )
    .unwrap();
    let decoded = Proposal::decode(&proposal.encode().unwrap()).unwrap();
    assert_eq!(decoded.id().unwrap(), proposal.id().unwrap());
    assert!(view.validate(&decoded).is_ok());
    assert!(
        decoded
            .verify([7; 32], 0, view.head(), &identities[0].peer())
            .is_err()
    );
    assert!(
        decoded
            .verify(anchor.id(), 1, view.head(), &identities[0].peer())
            .is_err()
    );
    assert!(
        decoded
            .verify(anchor.id(), 0, [8; 32], &identities[0].peer())
            .is_err()
    );
    assert!(
        decoded
            .verify(anchor.id(), 0, view.head(), &identities[2].peer())
            .is_err()
    );
    let mut tampered = decoded.clone();
    tampered.commit = b"different commit".to_vec().into();
    assert!(tampered.encode().is_err());
    let mut forged = decoded;
    forged.header.author = identities[2].peer().fingerprint_bytes();
    assert!(
        forged
            .verify(anchor.id(), 0, view.head(), &identities[2].peer())
            .is_err()
    );
    assert!(Proposal::decode(&vec![0; config::ROOM_CONTROL_WIRE_BYTES + 1]).is_err());
    assert!(Proposal::decode(&[255; 256]).is_err());
    let sealed = Proposal::add_with_welcome(
        &identities[0],
        anchor.id(),
        0,
        view.head(),
        b"synthetic encrypted control fixture",
        &package,
        b"recipient ciphertext fixture",
    )
    .unwrap();
    assert!(view.validate(&sealed).is_ok());
    let mut altered = sealed.clone();
    altered.welcome = b"substituted recipient ciphertext".to_vec().into();
    assert!(altered.encode().is_err());
    altered.header.payload = payload_digest(
        altered.commit.as_slice(),
        altered.subject.as_slice(),
        altered.welcome.as_slice(),
    );
    assert!(view.validate(&altered).is_err()); // Hash repair cannot forge signature.
}

#[test]
fn replicated_welcome_snapshot_limit_rejects_without_mutating_membership() {
    let (_directory, identities, anchor, _alice, mut bob, mut view) = fixture();
    let package = bob.key_package().unwrap();
    let proposal = Proposal::add_with_welcome(
        &identities[0],
        anchor.id(),
        0,
        view.head(),
        b"synthetic encrypted control fixture",
        &package,
        &vec![255; config::ROOM_MAX_WIRE_BYTES],
    )
    .unwrap();
    assert!(view.apply_accepted(&proposal).is_err());
    assert_eq!(view.epoch(), 0);
    assert_eq!(view.peers().len(), 1);
    assert!(
        view.welcome_for(&identities[1].peer().fingerprint_bytes())
            .is_none()
    );
}

#[test]
fn current_public_membership_permissions_and_replay_state_survive_snapshot() {
    let (_directory, identities, anchor, _alice, mut bob, mut view) = fixture();
    let package = bob.key_package().unwrap();
    let add = Proposal::add(
        &identities[0],
        anchor.id(),
        0,
        view.head(),
        b"public-fixture opaque commit",
        &package,
    )
    .unwrap();
    view.apply_accepted(&add).unwrap(); // Controlled ordered test input, NOT consensus.
    assert_eq!(view.peers().len(), 2);
    assert!(view.apply_accepted(&add).is_err());
    let remove = Proposal::remove(
        &identities[0],
        anchor.id(),
        1,
        view.head(),
        b"opaque remove",
        &identities[1].peer(),
    )
    .unwrap();
    view.apply_accepted(&remove).unwrap();
    let old_author = Proposal::remove(
        &identities[1],
        anchor.id(),
        2,
        view.head(),
        b"opaque forged current authority",
        &identities[0].peer(),
    )
    .unwrap();
    assert!(view.validate(&old_author).is_err());
    let snapshot = view.snapshot();
    let digest = snapshot.digest().unwrap();
    let restored = View::restore_verified(
        &anchor,
        Snapshot::decode(&snapshot.encode().unwrap()).unwrap(),
        view.head(),
        digest,
    )
    .unwrap();
    assert_eq!(restored.epoch(), view.epoch());
    assert_eq!(restored.peers().len(), 1);
    assert!(View::restore_verified(&anchor, snapshot, view.head(), [0; 32]).is_err());
    let readd_consumed = Proposal::add(
        &identities[0],
        anchor.id(),
        2,
        view.head(),
        b"opaque replay",
        &package,
    )
    .unwrap();
    assert!(restored.validate(&readd_consumed).is_err());
}

#[test]
fn accepted_public_delta_must_match_private_mls_before_durable_effect() {
    use crate::rooms::group::Group;
    let (_directory, identities, anchor, alice_actor, mut bob_actor, mut view) = fixture();
    let mut alice = Group::create(alice_actor).unwrap();
    assert_eq!(
        alice.membership_digest().unwrap(),
        view.membership_digest().unwrap()
    );
    let package = bob_actor.key_package().unwrap();
    let commit = alice
        .prepare_add(alice.validate_package(&package).unwrap())
        .unwrap();
    let add = Proposal::add(
        &identities[0],
        anchor.id(),
        0,
        view.head(),
        &commit,
        &package,
    )
    .unwrap();
    view.apply_accepted(&add).unwrap(); // Deterministic test order, NOT a quorum.
    assert!(alice.confirm_ordered_view(1, &commit, [0; 32]).is_err());
    assert_eq!(alice.epoch(), 0);
    let welcome = alice
        .confirm_ordered_view(1, &commit, view.membership_digest().unwrap())
        .unwrap()
        .unwrap();
    let mut bob = Group::join(bob_actor, &welcome, &identities[0].peer(), 1).unwrap();
    assert_eq!(
        bob.membership_digest().unwrap(),
        view.membership_digest().unwrap()
    );
    let grant = Grant::issue(
        &anchor,
        &[],
        &identities[0],
        &identities[2].peer(),
        Rights::PARTICIPANT,
    )
    .unwrap();
    let mut charlie = CryptoMember::new(&identities[2], &anchor, vec![grant]).unwrap();
    let package = charlie.key_package().unwrap();
    let actually_adds_charlie = alice
        .prepare_add(alice.validate_package(&package).unwrap())
        .unwrap();
    // An authorized signer can produce an inconsistent opaque control payload.
    // Public validation cannot decrypt it; private state must cross-check outcome.
    let falsely_claims_bob_removed = Proposal::remove(
        &identities[0],
        anchor.id(),
        1,
        view.head(),
        &actually_adds_charlie,
        &identities[1].peer(),
    )
    .unwrap();
    view.apply_accepted(&falsely_claims_bob_removed).unwrap();
    assert!(
        bob.apply_ordered_view(2, &actually_adds_charlie, view.membership_digest().unwrap())
            .is_err()
    );
    assert_eq!(bob.epoch(), 1);
    assert_eq!(bob.peers().len(), 2);
    assert!(
        alice
            .confirm_ordered_view(2, &actually_adds_charlie, view.membership_digest().unwrap())
            .is_err()
    );
    assert_eq!(alice.epoch(), 1);
    assert_eq!(alice.peers().len(), 2);
}
