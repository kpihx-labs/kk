#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

use kk::{Identity, Peer};

fn private_root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

#[test]
fn identity_is_private_stable_and_never_replaced() {
    let root = private_root();
    let path = root.path().join("identity");
    let identity = Identity::create(&path)
        .unwrap_or_else(|error| panic!("identity fixture {}: {error}", path.display()));
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
    let root = private_root();
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
    let root = private_root();
    let a = root.path().join("a");
    let b = root.path().join("b");
    Identity::create(&a).unwrap();
    Identity::create(&b).unwrap();
    fs::copy(b.join("private-key.der"), a.join("private-key.der")).unwrap();
    assert!(Identity::load(&a).is_err());
}

#[test]
fn malformed_and_oversized_public_certificates_are_rejected() {
    let root = private_root();
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

#[test]
fn hard_linked_private_keys_and_certificates_are_rejected() {
    for name in ["private-key.der", "certificate.der"] {
        let root = private_root();
        let path = root.path().join("identity");
        let identity = Identity::create(&path).unwrap();
        let alias = root.path().join("alias");
        fs::hard_link(path.join(name), &alias).unwrap();
        assert!(Identity::load(&path).is_err(), "accepted aliased {name}");
        // Refusal leaves both names intact, with the original certificate stable.
        assert!(alias.is_file());
        assert_eq!(
            Peer::load(&path.join("certificate.der"))
                .unwrap()
                .fingerprint(),
            identity.peer().fingerprint()
        );
    }
}

#[test]
fn writable_ancestor_and_user_alias_are_refused_without_creating_keys() {
    let root = private_root();
    let unsafe_parent = root.path().join("shared");
    fs::create_dir(&unsafe_parent).unwrap();
    fs::set_permissions(&unsafe_parent, fs::Permissions::from_mode(0o770)).unwrap();
    let path = unsafe_parent.join("identity");
    assert!(Identity::create(&path).is_err());
    assert!(!path.exists());
    let alias = root.path().join("alias");
    symlink(root.path(), &alias).unwrap();
    // Root-owned system aliases are a separate, explicitly trusted boundary.
    if rustix::process::geteuid().as_raw() != 0 {
        assert!(Identity::create(&alias.join("identity")).is_err());
        assert!(!root.path().join("identity").exists());
    }
}

#[test]
fn unsafe_certificate_permissions_and_special_key_bits_are_rejected() {
    let root = private_root();
    let path = root.path().join("identity");
    Identity::create(&path).unwrap();
    let key = path.join("private-key.der");
    fs::set_permissions(&key, fs::Permissions::from_mode(0o4600)).unwrap();
    assert!(Identity::load(&path).is_err());
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(
        path.join("certificate.der"),
        fs::Permissions::from_mode(0o666),
    )
    .unwrap();
    assert!(Identity::load(&path).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn special_identity_files_are_refused_without_waiting_for_a_writer() {
    let root = private_root();
    let path = root.path().join("identity");
    Identity::create(&path).unwrap();
    let key = path.join("private-key.der");
    fs::rename(&key, path.join("retained-key.der")).unwrap();
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &key,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
        0,
    )
    .unwrap();
    assert!(Identity::load(&path).is_err());
    assert!(Peer::load(&key).is_err());
}
