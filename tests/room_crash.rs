#![cfg(unix)]
//! Isolate fork/exec crash injection from parallel library lease tests. A child
//! briefly inherits every parent file description until exec (even CLOEXEC).
//! Keeping this in its own test process avoids mistaking inherited leases for
//! a live competing room actor. Production lock checks remain strict.
mod support;
use kk::{
    Identity, Peer,
    rooms::{Anchor, Grant, Rights, RootBundle, group::Group, mls::CryptoMember, store::Provider},
};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn load(root: &Path, identity: &Identity, anchor: &Anchor, index: usize) -> Group {
    Group::restore(
        CryptoMember::open_private(identity, anchor, &root.join(format!("state-{index}"))).unwrap(),
    )
    .unwrap()
}
fn fixture(root: &Path) -> (Vec<Identity>, Anchor, Group, Group) {
    let ids: Vec<_> = (0..2)
        .map(|i| Identity::create(&root.join(format!("identity-{i}"))).unwrap())
        .collect();
    let bundle = RootBundle::create(&ids[0], "Isolated process crash fixture").unwrap();
    fs::write(root.join("anchor.json"), bundle.encode().unwrap()).unwrap();
    let anchor = bundle.verify(&ids[0].peer()).unwrap();
    let grant = Grant::issue(&anchor, &[], &ids[0], &ids[1].peer(), Rights::MODERATOR).unwrap();
    let mut alice = Group::create(
        CryptoMember::create_private(&ids[0], &anchor, vec![], &root.join("state-0")).unwrap(),
    )
    .unwrap();
    let mut bob =
        CryptoMember::create_private(&ids[1], &anchor, vec![grant], &root.join("state-1")).unwrap();
    let package = bob.key_package().unwrap();
    let commit = alice
        .prepare_add(alice.validate_package(&package).unwrap())
        .unwrap();
    let welcome = alice.confirm_ordered(1, &commit).unwrap().unwrap();
    let bob = Group::join(bob, &welcome, &ids[0].peer(), 1).unwrap();
    (ids, anchor, alice, bob)
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
        let mut bob = load(&root, &identity, &anchor, 1);
        assert_eq!(
            bob.decrypt_text(&fs::read(root.join("request.bin")).unwrap())
                .unwrap()
                .content
                .text(),
            "crash fixture request"
        );
        let reply = bob.encrypt_text("committed before kill", true).unwrap();
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("reply.bin"))
            .unwrap();
        output.write_all(&reply).unwrap();
        output.sync_all().unwrap();
        drop(bob);
        let provider = Provider::open(&root.join("state-1")).unwrap();
        let _transaction = provider.transaction().unwrap();
        provider
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
    let directory = support::private_root();
    let (ids, anchor, mut alice, bob) = fixture(directory.path());
    let request = alice.encrypt_text("crash fixture request", true).unwrap();
    fs::write(directory.path().join("request.bin"), &request).unwrap();
    drop(bob);
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "real_process_kill_recovers_committed_ratchets_and_uncommitted_sql",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env(ENV, directory.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut guard = KillOnDrop(Some(child));
    let deadline = Instant::now() + Duration::from_secs(15);
    while !directory.path().join("ready").exists() {
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
    assert!(
        !guard
            .0
            .take()
            .unwrap()
            .wait_with_output()
            .unwrap()
            .status
            .success()
    );
    let provider = Provider::open(&directory.path().join("state-1")).unwrap();
    assert!(provider.metadata::<String>("unfinished").unwrap().is_none());
    drop(provider);
    let mut bob = load(directory.path(), &ids[1], &anchor, 1);
    assert!(bob.decrypt_text(&request).is_err());
    assert_eq!(
        alice
            .decrypt_text(&fs::read(directory.path().join("reply.bin")).unwrap())
            .unwrap()
            .content
            .text(),
        "committed before kill"
    );
    assert_eq!(
        alice
            .decrypt_text(&bob.encrypt_text("after crash", false).unwrap())
            .unwrap()
            .content
            .text(),
        "after crash"
    );
}
