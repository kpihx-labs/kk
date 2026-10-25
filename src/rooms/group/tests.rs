use super::*;
use crate::{
    Identity,
    rooms::{Anchor, Grant, Rights, RootBundle},
};

struct Fixture {
    _directory: tempfile::TempDir,
    identities: Vec<Identity>,
    anchor: Anchor,
    bob_grant: Grant,
    alice: Group,
    bob: Group,
    charlie: Group,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let identities: Vec<_> = (0..4)
            .map(|i| Identity::create(&directory.path().join(format!("group-{i}"))).unwrap())
            .collect();
        let anchor = RootBundle::create(&identities[0], "Real MLS crypto fixture")
            .unwrap()
            .verify(&identities[0].peer())
            .unwrap();
        let bob_grant = Grant::issue(
            &anchor,
            &[],
            &identities[0],
            &identities[1].peer(),
            Rights::MODERATOR,
        )
        .unwrap();
        let charlie_grant = Grant::issue(
            &anchor,
            std::slice::from_ref(&bob_grant),
            &identities[1],
            &identities[2].peer(),
            Rights::PARTICIPANT,
        )
        .unwrap();
        let alice_actor = CryptoMember::new(&identities[0], &anchor, vec![]).unwrap();
        let mut bob_actor =
            CryptoMember::new(&identities[1], &anchor, vec![bob_grant.clone()]).unwrap();
        let mut charlie_actor = CryptoMember::new(
            &identities[2],
            &anchor,
            vec![bob_grant.clone(), charlie_grant],
        )
        .unwrap();
        let mut alice = Group::create(alice_actor).unwrap();
        let candidate = alice
            .actor
            .validate_package(&bob_actor.key_package().unwrap())
            .unwrap();
        let commit = alice.prepare_add(candidate).unwrap();
        assert!(alice.encrypt_text("not-yet-committed", false).is_err());
        let welcome = alice.confirm_ordered(1, &commit).unwrap().unwrap();
        let mut bob = Group::join(bob_actor, &welcome, &identities[0].peer(), 1).unwrap();
        let candidate = bob
            .actor
            .validate_package(&charlie_actor.key_package().unwrap())
            .unwrap();
        let commit = bob.prepare_add(candidate).unwrap();
        // Deterministic ordered test delivery, NOT a network/consensus implementation.
        alice.apply_ordered(2, &commit).unwrap();
        let welcome = bob.confirm_ordered(2, &commit).unwrap().unwrap();
        let charlie = Group::join(charlie_actor, &welcome, &identities[1].peer(), 2).unwrap();
        Self {
            _directory: directory,
            identities,
            anchor,
            bob_grant,
            alice,
            bob,
            charlie,
        }
    }
    fn dave(&self, issuer: usize, rights: Rights) -> CryptoMember {
        let parents = if issuer == 0 {
            vec![]
        } else {
            vec![self.bob_grant.clone()]
        };
        let grant = Grant::issue(
            &self.anchor,
            &parents,
            &self.identities[issuer],
            &self.identities[3].peer(),
            rights,
        )
        .unwrap();
        let mut chain = parents;
        chain.push(grant);
        CryptoMember::new(&self.identities[3], &self.anchor, chain).unwrap()
    }
}

#[test]
fn real_group_messages_replay_reordering_and_removal_use_epoch_keys() {
    let mut fixture = Fixture::new();
    assert_eq!(fixture.alice.peers().len(), 3);
    let private = "synthetic-secret-room-content";
    let wire = fixture.bob.encrypt_text(private, true).unwrap();
    assert!(
        !wire
            .windows(private.len())
            .any(|bytes| bytes == private.as_bytes())
    );
    let text = fixture.charlie.decrypt_text(&wire).unwrap();
    assert!(text.content.sensitive && text.content.text() == private);
    assert!(!format!("{text:?}").contains(private));
    assert!(fixture.charlie.decrypt_text(&wire).is_err());
    assert_eq!(
        fixture.alice.decrypt_text(&wire).unwrap().sender,
        fixture.identities[1].peer().fingerprint()
    );
    let first = fixture.bob.encrypt_text("first", false).unwrap();
    let second = fixture.bob.encrypt_text("second", false).unwrap();
    assert_eq!(
        fixture
            .charlie
            .decrypt_text(&second)
            .unwrap()
            .content
            .text(),
        "second"
    );
    assert_eq!(
        fixture.charlie.decrypt_text(&first).unwrap().content.text(),
        "first"
    );
    let old = fixture.alice.encrypt_text("stale epoch", false).unwrap();
    let removal = fixture
        .bob
        .prepare_remove(&fixture.identities[0].peer().fingerprint())
        .unwrap();
    fixture.charlie.apply_ordered(3, &removal).unwrap();
    assert!(fixture.bob.confirm_ordered(3, &removal).unwrap().is_none());
    let new = fixture
        .bob
        .encrypt_text("after creator removal", false)
        .unwrap();
    // Alice has NOT received the removal, yet its old secrets cannot decrypt epoch 3.
    assert!(fixture.alice.is_active());
    assert!(fixture.alice.decrypt_text(&new).is_err());
    assert_eq!(
        fixture.charlie.decrypt_text(&new).unwrap().content.text(),
        "after creator removal"
    );
    assert!(fixture.charlie.decrypt_text(&old).is_err());
    fixture.alice.apply_ordered(3, &removal).unwrap();
    assert!(!fixture.alice.is_active());
}

#[test]
fn delegation_continues_without_creator_but_historical_issuer_cannot_admit() {
    let mut fixture = Fixture::new();
    let mut stale = fixture.dave(0, Rights::READER);
    let stale_package = stale.key_package().unwrap();
    let remove = fixture
        .bob
        .prepare_remove(&fixture.identities[0].peer().fingerprint())
        .unwrap();
    fixture.charlie.apply_ordered(3, &remove).unwrap();
    fixture.bob.confirm_ordered(3, &remove).unwrap();
    let candidate = fixture.bob.actor.validate_package(&stale_package).unwrap();
    assert!(fixture.bob.prepare_add(candidate).is_err());
    let mut dave = fixture.dave(1, Rights::READER);
    let wire = dave.key_package().unwrap();
    let participant_attempt = fixture.charlie.actor.validate_package(&wire).unwrap();
    assert!(fixture.charlie.prepare_add(participant_attempt).is_err());
    let candidate = fixture.bob.actor.validate_package(&wire).unwrap();
    let add = fixture.bob.prepare_add(candidate).unwrap();
    fixture.charlie.apply_ordered(4, &add).unwrap();
    let welcome = fixture.bob.confirm_ordered(4, &add).unwrap().unwrap();
    let mut dave = Group::join(dave, &welcome, &fixture.identities[1].peer(), 4).unwrap();
    assert!(dave.encrypt_text("reader must not publish", false).is_err());
    let reply = fixture
        .charlie
        .encrypt_text("new peer reads", false)
        .unwrap();
    assert_eq!(
        dave.decrypt_text(&reply).unwrap().content.text(),
        "new peer reads"
    );
    dave.state.set_aad(TEXT_AAD.to_vec());
    let forged = dave
        .state
        .create_message(
            &dave.actor.provider,
            &dave.actor.signer,
            b"\0reader bypasses producer policy",
        )
        .unwrap();
    assert!(matches!(
        fixture.bob.decrypt_text(&encode(&forged).unwrap()),
        Err(Error::Denied)
    ));
}

#[test]
fn cryptographically_valid_unauthorized_commits_and_unresolved_forks_fail_closed() {
    let mut fixture = Fixture::new();
    // Bypass the K&K producer API to emulate a malicious MLS member. Cryptographic
    // membership alone does not authorize Charlie to remove another participant.
    fixture.charlie.state.set_aad(CONTROL_AAD.to_vec());
    let (bad, _, _) = fixture
        .charlie
        .state
        .remove_members(
            &fixture.charlie.actor.provider,
            &fixture.charlie.actor.signer,
            &[fixture.alice.state.own_leaf_index()],
        )
        .unwrap();
    assert!(matches!(
        fixture.bob.apply_ordered(3, &encode(&bad).unwrap()),
        Err(Error::Denied)
    ));
    assert_eq!(fixture.bob.epoch(), 2);

    let mut fixture = Fixture::new();
    let mut dave = fixture.dave(1, Rights::PARTICIPANT);
    let candidate = fixture
        .alice
        .actor
        .validate_package(&dave.key_package().unwrap())
        .unwrap();
    let competing = fixture.alice.prepare_add(candidate).unwrap();
    assert!(fixture.alice.confirm_ordered(3, b"wrong receipt").is_err());
    let accepted = fixture
        .bob
        .prepare_remove(&fixture.identities[0].peer().fingerprint())
        .unwrap();
    assert!(fixture.alice.apply_ordered(3, &accepted).is_err());
    assert_eq!(fixture.alice.epoch(), 2);
    // Only a verified rejection from the future ordered-delivery owner permits this.
    fixture.alice.reject_ordered(&competing).unwrap();
    fixture.alice.apply_ordered(3, &accepted).unwrap();
    fixture.charlie.apply_ordered(3, &accepted).unwrap();
    fixture.bob.confirm_ordered(3, &accepted).unwrap();
    assert!(!fixture.alice.is_active());
    assert!(fixture.charlie.apply_ordered(3, &accepted).is_err());
}
