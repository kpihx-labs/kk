use super::*;
use crate::profile;
use openmls_basic_credential::SignatureKeyPair;
use openmls_traits::{signatures::Signer, types::SignatureScheme};
use std::{
    fs,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
};

#[test]
fn initialization_failure_closes_sqlite_before_releasing_the_owner_lease() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("private");
    let mut lease =
        super::super::lease::Lease::open(&path, "private.sqlite3", true, 1024 * 1024).unwrap();
    let inherited = lease.file().try_clone().unwrap();
    let database = lease.path().to_owned();
    let connection = Connection::open(&database).unwrap();
    connection.execute_batch("CREATE TABLE sentinel(value); PRAGMA user_version=99; BEGIN EXCLUSIVE; INSERT INTO sentinel VALUES(1);").unwrap();
    let closed_first = Arc::new(AtomicBool::new(false));
    let observed = closed_first.clone();
    lease.observe_release(move || {
        let check = || -> rusqlite::Result<()> {
            let probe = Connection::open(&database)?;
            probe.busy_timeout(std::time::Duration::ZERO)?;
            probe.execute_batch("BEGIN EXCLUSIVE; ROLLBACK")
        };
        observed.store(check().is_ok(), Ordering::SeqCst);
    });
    assert!(Provider::initialize(connection, Some(lease), None, false).is_err());
    assert!(
        closed_first.load(Ordering::SeqCst),
        "SQLite remained open at lease release"
    );
    let reopened =
        super::super::lease::Lease::open(&path, "private.sqlite3", false, 1024 * 1024).unwrap();
    drop(inherited);
    assert!(
        super::super::lease::Lease::open(&path, "private.sqlite3", false, 1024 * 1024).is_err()
    );
    drop(reopened);
    let probe = Connection::open(path.join("private.sqlite3")).unwrap();
    let count: u64 = probe
        .query_row("SELECT COUNT(*) FROM sentinel", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        count, 0,
        "failed initialization did not roll back its private writes"
    );
}

#[test]
fn official_key_store_and_metadata_commit_and_reopen_together() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("room");
    let provider = Provider::create(&path).unwrap();
    let signer = SignatureKeyPair::new(SignatureScheme::ED25519).unwrap();
    let public = signer.to_public_vec();
    assert!(provider.set_metadata("epoch", &3u64).is_err());
    let transaction = provider.transaction().unwrap();
    signer.store(provider.storage()).unwrap();
    provider.set_metadata("epoch", &3u64).unwrap();
    assert!(provider.transaction().is_err());
    transaction.commit().unwrap();
    assert!(Provider::open(&path).is_err());
    drop(signer);
    drop(provider);
    let reopened = Provider::open(&path).unwrap();
    assert_eq!(reopened.metadata::<u64>("epoch").unwrap(), Some(3));
    let restored =
        SignatureKeyPair::read(reopened.storage(), &public, SignatureScheme::ED25519).unwrap();
    let signature = restored.sign(b"synthetic recovered signer check").unwrap();
    assert!(
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &public)
            .verify(b"synthetic recovered signer check", &signature)
            .is_ok()
    );
    assert_eq!(
        fs::metadata(path.join("private.sqlite3"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(Provider::create(&path).is_err());
}

#[test]
fn dropped_transaction_rolls_back_and_keeps_lease_alive_without_provider() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("room");
    let provider = Provider::create(&path).unwrap();
    let signer = SignatureKeyPair::new(SignatureScheme::ED25519).unwrap();
    let public = signer.to_public_vec();
    let transaction = provider.transaction().unwrap();
    signer.store(provider.storage()).unwrap();
    provider.set_metadata("uncommitted", &true).unwrap();
    drop(provider);
    assert!(Provider::open(&path).is_err());
    drop(transaction);
    let provider = Provider::open(&path).unwrap();
    assert!(provider.metadata::<bool>("uncommitted").unwrap().is_none());
    assert!(
        SignatureKeyPair::read(provider.storage(), &public, SignatureScheme::ED25519).is_none()
    );
    let transaction = provider.transaction().unwrap();
    provider.set_metadata("retry", &true).unwrap();
    transaction.commit().unwrap();
}

#[test]
fn unsafe_files_links_and_corrupt_state_are_refused_without_repair() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("room");
    drop(Provider::create(&path).unwrap());
    let file = path.join("private.sqlite3");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Provider::open(&path).is_err());
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o644
    );
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    let moved = path.join("preserved.sqlite3");
    fs::rename(&file, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &file).unwrap();
    assert!(Provider::open(&path).is_err());
    let linked = directory.path().join("linked");
    profile::private_directory(&linked, true).unwrap();
    fs::hard_link(&moved, linked.join("private.sqlite3")).unwrap();
    assert!(Provider::open(&linked).is_err());
    let corrupt = directory.path().join("corrupt");
    profile::private_directory(&corrupt, true).unwrap();
    let corrupt_file = corrupt.join("private.sqlite3");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&corrupt_file)
        .unwrap();
    assert!(Provider::open(&corrupt).is_err());
    assert_eq!(fs::metadata(&corrupt_file).unwrap().len(), 0);
    assert!(Provider::open(&directory.path().join("missing")).is_err());
}

#[test]
fn failed_sqlite_commit_quarantines_the_handle_and_never_reports_success() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("room");
    let provider = Provider::create(&path).unwrap();
    provider.session.0.connection.execute_batch("CREATE TABLE fault_parent(id INTEGER PRIMARY KEY);
        CREATE TABLE fault_child(parent INTEGER REFERENCES fault_parent(id) DEFERRABLE INITIALLY DEFERRED);").unwrap();
    let transaction = provider.transaction().unwrap();
    provider.set_metadata("must_not_commit", &true).unwrap();
    provider
        .session
        .0
        .connection
        .execute("INSERT INTO fault_child(parent) VALUES(99)", [])
        .unwrap();
    assert!(transaction.commit().is_err());
    assert!(provider.transaction().is_err());
    assert!(provider.metadata::<bool>("must_not_commit").is_err());
    drop(provider);
    let reopened = Provider::open(&path).unwrap();
    assert!(
        reopened
            .metadata::<bool>("must_not_commit")
            .unwrap()
            .is_none()
    );
    assert!(JsonCodec::to_vec(&"x".repeat(config::ROOM_STORE_VALUE_BYTES)).is_err());
}
