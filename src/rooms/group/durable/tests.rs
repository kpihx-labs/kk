use super::*;
use crate::{
    Identity,
    rooms::{Anchor, Grant, Rights, RootBundle},
};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

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
impl Pair {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
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
    let directory = tempfile::tempdir().unwrap();
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

struct KillOnDrop(Option<Child>);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn real_process_kill_recovers_committed_ratchets_and_uncommitted_sql() {
    const ENV: &str = "KK_TEST_ROOM_CRASH_DIRECTORY";
    if let Some(root) = std::env::var_os(ENV) {
        let root = std::path::PathBuf::from(root);
        let creator = Peer::load(&root.join("identity-0/certificate.der")).unwrap();
        let anchor = RootBundle::decode(&fs::read(root.join("anchor.json")).unwrap())
            .unwrap()
            .verify(&creator)
            .unwrap();
        let identity = Identity::load(&root.join("identity-1")).unwrap();
        let actor = CryptoMember::open_private(&identity, &anchor, &root.join("state-1")).unwrap();
        let mut bob = Group::restore(actor).unwrap();
        assert_eq!(
            bob.decrypt_text(&fs::read(root.join("request.bin")).unwrap())
                .unwrap()
                .content
                .text(),
            "crash fixture request"
        );
        let wire = bob.encrypt_text("committed before kill", true).unwrap();
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("reply.bin"))
            .unwrap();
        output.write_all(&wire).unwrap();
        output.sync_all().unwrap();
        let _transaction = bob.actor.provider.transaction().unwrap();
        bob.actor
            .provider
            .set_metadata(
                "unfinished",
                &"synthetic-uncommitted-padding".repeat(40_000),
            )
            .unwrap();
        fs::write(root.join("ready"), b"ready").unwrap();
        loop {
            std::thread::park();
        }
    }
    let mut pair = Pair::new();
    let request = pair
        .alice
        .encrypt_text("crash fixture request", true)
        .unwrap();
    fs::write(pair.directory.path().join("request.bin"), &request).unwrap();
    drop(pair.bob);
    let test_name = format!(
        "{}::real_process_kill_recovers_committed_ratchets_and_uncommitted_sql",
        module_path!().split_once("::").unwrap().1
    );
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test_name, "--nocapture", "--test-threads", "1"])
        .env(ENV, pair.directory.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut guard = KillOnDrop(Some(child));
    let deadline = Instant::now() + Duration::from_secs(15);
    while !pair.directory.path().join("ready").exists() {
        if let Some(status) = guard.0.as_mut().unwrap().try_wait().unwrap() {
            panic!("crash probe exited early: {status}");
        }
        assert!(
            Instant::now() < deadline,
            "crash probe did not reach its transaction"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    guard.0.as_mut().unwrap().kill().unwrap();
    let output = guard.0.take().unwrap().wait_with_output().unwrap();
    assert!(!output.status.success());
    let mut bob = load(pair.directory.path(), &pair.identities, &pair.anchor, 1);
    assert!(
        bob.actor
            .provider
            .metadata::<String>("unfinished")
            .unwrap()
            .is_none()
    );
    assert!(bob.decrypt_text(&request).is_err());
    let reply = fs::read(pair.directory.path().join("reply.bin")).unwrap();
    assert_eq!(
        pair.alice.decrypt_text(&reply).unwrap().content.text(),
        "committed before kill"
    );
    let next = bob.encrypt_text("after crash", false).unwrap();
    assert_eq!(
        pair.alice.decrypt_text(&next).unwrap().content.text(),
        "after crash"
    );
}
