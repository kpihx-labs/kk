#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

use kk::{Identity, Peer};

#[test]
fn identity_is_private_stable_and_never_replaced() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("identity");
    let identity = Identity::create(&path).unwrap();
    let fingerprint = identity.peer().fingerprint();
    assert_eq!(fingerprint.len(), 64);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(path.join("private-key.der"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(Identity::create(&path).is_err());
    assert_eq!(
        Identity::load(&path).unwrap().peer().fingerprint(),
        fingerprint
    );
    assert_eq!(
        Peer::load(&path.join("certificate.der"))
            .unwrap()
            .fingerprint(),
        fingerprint
    );
}

#[test]
fn unsafe_modes_and_symlinks_are_rejected_not_repaired() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("identity");
    Identity::create(&path).unwrap();
    let key = path.join("private-key.der");
    fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Identity::load(&path).is_err());
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Identity::load(&path).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let alias = root.path().join("alias");
    symlink(&path, &alias).unwrap();
    assert!(Identity::load(&alias).is_err());
    symlink(
        path.join("certificate.der"),
        root.path().join("public-link"),
    )
    .unwrap();
    assert!(Peer::load(&root.path().join("public-link")).is_err());
    fs::rename(&key, path.join("real-key.der")).unwrap();
    symlink(path.join("real-key.der"), &key).unwrap();
    assert!(Identity::load(&path).is_err());
}

#[test]
fn mismatched_private_keys_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    Identity::create(&a).unwrap();
    Identity::create(&b).unwrap();
    fs::copy(b.join("private-key.der"), a.join("private-key.der")).unwrap();
    assert!(Identity::load(&a).is_err());
}

#[test]
fn malformed_and_oversized_public_certificates_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let certificate = root.path().join("certificate.der");
    for bytes in [
        vec![],
        b"not a certificate".to_vec(),
        vec![0; kk::config::MAX_IDENTITY_BYTES + 1],
    ] {
        fs::write(&certificate, bytes).unwrap();
        assert!(Peer::load(&certificate).is_err());
    }
}
