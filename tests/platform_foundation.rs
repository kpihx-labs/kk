//! Narrow native contracts, not a replacement for Linux product integration tests.
use std::process::Command;

#[test]
fn binary_smoke_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--version")
        .output()
        .expect("execute the native binary");
    assert!(output.status.success());
    assert_eq!(output.stdout, format!("kk {}\n", kk::VERSION).as_bytes());
    assert!(output.stderr.is_empty());
}

#[test]
fn binary_smoke_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--help")
        .output()
        .expect("execute the native binary");
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("Usage: kk")
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn binary_smoke_invalid_argument_is_not_success() {
    let output = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--unknown-foundation-option")
        .output()
        .expect("execute the native binary");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr).unwrap().contains("error:"));
}

#[test]
fn public_file_hash_contract_is_platform_independent() {
    let hash = "0123456789abcdef".repeat(4);
    let parsed = kk::files::parse_hash(&hash).expect("valid public digest");
    let info = kk::files::FileInfo {
        bytes: 3,
        sha256: parsed,
    };
    assert_eq!(info.hash_hex(), hash);
    assert!(kk::files::parse_hash("invalid").is_err());
}

#[cfg(unix)]
#[test]
fn unix_protected_identity_round_trip() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("profile");
    kk::profile::private_directory(&profile, true).unwrap();
    let identity_path = profile.join("identity");
    let identity = kk::Identity::create(&identity_path).unwrap();
    let loaded = kk::Identity::load(&identity_path).unwrap();
    assert_eq!(identity.peer().fingerprint(), loaded.peer().fingerprint());
    assert_eq!(
        std::fs::metadata(&identity_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(identity_path.join("private-key.der"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(kk::Identity::create(&identity_path).is_err());
}

#[cfg(not(unix))]
#[tokio::test(flavor = "current_thread")]
async fn non_unix_protected_operations_refuse_without_creating_state() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("must-not-exist");
    assert!(matches!(
        kk::profile::private_directory(&destination, true),
        Err(kk::Error::UnsupportedPlatform)
    ));
    assert!(matches!(
        kk::Identity::create(&destination),
        Err(kk::Error::UnsupportedPlatform)
    ));
    assert!(matches!(
        kk::Identity::load(&destination),
        Err(kk::Error::UnsupportedPlatform)
    ));
    assert!(matches!(
        kk::output::line("synthetic foundation text".into()).await,
        Err(kk::Error::UnsupportedPlatform)
    ));
    assert!(!destination.exists());
    assert_eq!(
        kk::Error::UnsupportedPlatform.code(),
        "UNSUPPORTED_PLATFORM"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn file_inspection_obeys_the_platform_contract() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    std::fs::write(&source, b"abc").unwrap();
    let result = kk::files::inspect(&source, std::time::Duration::from_secs(5)).await;
    #[cfg(unix)]
    {
        let info = result.unwrap();
        assert_eq!(info.bytes, 3);
        assert_eq!(
            info.hash_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
    #[cfg(not(unix))]
    assert!(matches!(result, Err(kk::Error::UnsupportedPlatform)));
    assert_eq!(std::fs::read(source).unwrap(), b"abc");
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn atomic_message_publication_is_linux_only() {
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("profile");
    kk::profile::private_directory(&profile, true).unwrap();
    let identity = kk::Identity::create(&profile.join("identity")).unwrap();
    let destination = profile.join("message");
    let result = kk::files::save_bytes(&destination, b"abc", &identity.peer(), false).await;
    #[cfg(target_os = "linux")]
    {
        result.unwrap();
        assert_eq!(std::fs::read(destination).unwrap(), b"abc");
    }
    #[cfg(not(target_os = "linux"))]
    {
        assert!(matches!(result, Err(kk::Error::UnsupportedPlatform)));
        assert!(!destination.exists());
    }
}
