#![cfg(unix)]
mod support;

use kk::{
    Identity,
    rooms::{Anchor, Grant, MAX_BUNDLE, MAX_CHAIN, Rights, RootBundle},
};

struct Fixture {
    _directory: tempfile::TempDir,
    members: Vec<Identity>,
    anchor: Anchor,
}
impl Fixture {
    fn new(count: usize) -> Self {
        let directory = support::private_root();
        let members: Vec<_> = (0..count)
            .map(|i| Identity::create(&directory.path().join(format!("member-{i}"))).unwrap())
            .collect();
        let root = RootBundle::create(&members[0], "Synthetic room").unwrap();
        let anchor = root.verify(&members[0].peer()).unwrap();
        Self {
            _directory: directory,
            members,
            anchor,
        }
    }
}

#[test]
fn delegated_proof_survives_creator_key_absence_without_exporting_it() {
    let mut fixture = Fixture::new(3);
    let alice = fixture.members.remove(0);
    let bob = &fixture.members[0];
    let charlie = &fixture.members[1];
    let first = Grant::issue(&fixture.anchor, &[], &alice, &bob.peer(), Rights::MODERATOR).unwrap();
    drop(alice);
    let second = Grant::issue(
        &fixture.anchor,
        std::slice::from_ref(&first),
        bob,
        &charlie.peer(),
        Rights::PARTICIPANT,
    )
    .unwrap();
    let bytes = Grant::encode_chain(&[first, second]).unwrap();
    let member = fixture
        .anchor
        .verify_chain(&Grant::decode_chain(&bytes).unwrap())
        .unwrap();
    assert_eq!(member.peer().fingerprint(), charlie.peer().fingerprint());
    assert!(member.rights().can_publish());
    assert!(!member.rights().can_invite());
    // This proves offline authority only, not network room admission or revocation.
}

#[test]
fn inviter_cannot_delegate_authority_or_sign_as_another_member() {
    let fixture = Fixture::new(4);
    let [alice, bob, charlie, dave] = fixture.members.as_slice() else {
        panic!()
    };
    let first = Grant::issue(&fixture.anchor, &[], alice, &bob.peer(), Rights::INVITER).unwrap();
    assert!(
        Grant::issue(
            &fixture.anchor,
            std::slice::from_ref(&first),
            bob,
            &charlie.peer(),
            Rights::MODERATOR
        )
        .is_err()
    );
    assert!(
        Grant::issue(
            &fixture.anchor,
            std::slice::from_ref(&first),
            dave,
            &charlie.peer(),
            Rights::READER
        )
        .is_err()
    );
    let second = Grant::issue(
        &fixture.anchor,
        std::slice::from_ref(&first),
        bob,
        &charlie.peer(),
        Rights::PARTICIPANT,
    )
    .unwrap();
    assert!(
        Grant::issue(
            &fixture.anchor,
            &[first, second],
            charlie,
            &dave.peer(),
            Rights::READER
        )
        .is_err()
    );
}

#[test]
fn altered_roots_grants_domains_cross_room_replay_and_cycles_are_rejected() {
    let fixture = Fixture::new(3);
    let [alice, bob, charlie] = fixture.members.as_slice() else {
        panic!()
    };
    let root = fixture.anchor.bundle();
    assert!(root.verify(&bob.peer()).is_err());
    let other = RootBundle::create(alice, "Synthetic room")
        .unwrap()
        .verify(&alice.peer())
        .unwrap();
    assert_ne!(fixture.anchor.id(), other.id());
    let first = Grant::issue(&fixture.anchor, &[], alice, &bob.peer(), Rights::MODERATOR).unwrap();
    assert!(other.verify_chain(std::slice::from_ref(&first)).is_err());
    assert!(
        Grant::issue(
            &fixture.anchor,
            std::slice::from_ref(&first),
            bob,
            &alice.peer(),
            Rights::READER
        )
        .is_err()
    );
    let mut raw: serde_json::Value =
        serde_json::from_slice(&Grant::encode_chain(std::slice::from_ref(&first)).unwrap())
            .unwrap();
    let raw_root: serde_json::Value = serde_json::from_slice(&root.encode().unwrap()).unwrap();
    raw[0]["signature"] = raw_root["signature"].clone();
    assert!(
        fixture
            .anchor
            .verify_chain(&Grant::decode_chain(&serde_json::to_vec(&raw).unwrap()).unwrap())
            .is_err()
    );
    let second = Grant::issue(
        &fixture.anchor,
        std::slice::from_ref(&first),
        bob,
        &charlie.peer(),
        Rights::PARTICIPANT,
    )
    .unwrap();
    let mut raw: serde_json::Value =
        serde_json::from_slice(&Grant::encode_chain(&[first, second]).unwrap()).unwrap();
    raw[1]["body"]["rights"] = 31.into();
    assert!(
        fixture
            .anchor
            .verify_chain(&Grant::decode_chain(&serde_json::to_vec(&raw).unwrap()).unwrap())
            .is_err()
    );
}

#[test]
fn wire_and_delegation_depth_are_bounded() {
    assert!(RootBundle::decode(&vec![b' '; MAX_BUNDLE + 1]).is_err());
    assert!(Grant::decode_chain(&vec![b' '; MAX_BUNDLE + 1]).is_err());
    let fixture = Fixture::new(MAX_CHAIN + 2);
    let mut chain = Vec::new();
    for index in 0..MAX_CHAIN {
        chain.push(
            Grant::issue(
                &fixture.anchor,
                &chain,
                &fixture.members[index],
                &fixture.members[index + 1].peer(),
                Rights::MODERATOR,
            )
            .unwrap(),
        );
    }
    assert!(fixture.anchor.verify_chain(&chain).is_ok());
    assert!(
        Grant::issue(
            &fixture.anchor,
            &chain,
            &fixture.members[MAX_CHAIN],
            &fixture.members[MAX_CHAIN + 1].peer(),
            Rights::READER
        )
        .is_err()
    );
    let mut unknown: serde_json::Value =
        serde_json::from_slice(&fixture.anchor.bundle().encode().unwrap()).unwrap();
    unknown["extra"] = true.into();
    assert!(RootBundle::decode(&serde_json::to_vec(&unknown).unwrap()).is_err());
}
