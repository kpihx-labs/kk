#![cfg(target_os = "linux")]

use age::secrecy::SecretString;
use kk::{
    Error, config,
    envelope::{self, Lock, Unlock},
};
use std::{fs, os::unix::fs::PermissionsExt};

fn directory() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

#[tokio::test]
async fn recipient_envelope_roundtrips_binary_and_never_overwrites() {
    let directory = directory();
    let key = directory.path().join("identity.key");
    let recipient = envelope::keygen(&key).await.unwrap();
    assert!(recipient.starts_with("age1"));
    assert_eq!(
        fs::metadata(&key).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let data: Vec<_> = (0..150_000).map(|index| (index % 251) as u8).collect();
    let cipher = directory.path().join("payload.age");
    envelope::seal_bytes(
        &data,
        &cipher,
        Lock::Recipient(recipient.clone()),
        config::DEFAULT_TIMEOUT,
    )
    .await
    .unwrap();
    assert!(
        fs::read(&cipher)
            .unwrap()
            .starts_with(b"age-encryption.org/v1\n")
    );
    let plain = directory.path().join("plain");
    assert_eq!(
        envelope::open(
            &cipher,
            &plain,
            Unlock::Identity(key.clone()),
            config::DEFAULT_TIMEOUT
        )
        .await
        .unwrap(),
        data.len() as u64
    );
    assert_eq!(fs::read(&plain).unwrap(), data);
    assert!(
        envelope::open(
            &cipher,
            &plain,
            Unlock::Identity(key),
            config::DEFAULT_TIMEOUT
        )
        .await
        .is_err()
    );
    assert!(
        envelope::seal_bytes(
            b"other",
            &cipher,
            Lock::Recipient(recipient),
            config::DEFAULT_TIMEOUT
        )
        .await
        .is_err()
    );
    assert_eq!(fs::read(&plain).unwrap(), data);
}

#[tokio::test]
async fn wrong_keys_and_tampering_never_publish_plaintext() {
    let directory = directory();
    let key = directory.path().join("identity.key");
    let wrong = directory.path().join("wrong.key");
    let recipient = envelope::keygen(&key).await.unwrap();
    envelope::keygen(&wrong).await.unwrap();
    let cipher = directory.path().join("payload.age");
    envelope::seal_bytes(
        b"synthetic-test-fixture",
        &cipher,
        Lock::Recipient(recipient),
        config::DEFAULT_TIMEOUT,
    )
    .await
    .unwrap();
    let plain = directory.path().join("plain");
    assert!(matches!(
        envelope::open(
            &cipher,
            &plain,
            Unlock::Identity(wrong),
            config::DEFAULT_TIMEOUT
        )
        .await,
        Err(Error::Encryption)
    ));
    let mut bytes = fs::read(&cipher).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&cipher, bytes).unwrap();
    assert!(
        envelope::open(
            &cipher,
            &plain,
            Unlock::Identity(key),
            config::DEFAULT_TIMEOUT
        )
        .await
        .is_err()
    );
    assert!(!plain.exists());
}

#[tokio::test]
async fn passphrase_format_roundtrip_and_wrong_passphrase_refusal() {
    let directory = directory();
    let cipher = directory.path().join("payload.age");
    let plain = directory.path().join("plain");
    envelope::seal_bytes(
        b"fixture",
        &cipher,
        Lock::Passphrase(SecretString::from("synthetic passphrase".to_owned())),
        config::DEFAULT_FILE_TIMEOUT,
    )
    .await
    .unwrap();
    assert!(
        envelope::open(
            &cipher,
            &plain,
            Unlock::Passphrase(SecretString::from("wrong fixture".to_owned())),
            config::DEFAULT_FILE_TIMEOUT
        )
        .await
        .is_err()
    );
    assert!(!plain.exists());
    envelope::open(
        &cipher,
        &plain,
        Unlock::Passphrase(SecretString::from("synthetic passphrase".to_owned())),
        config::DEFAULT_FILE_TIMEOUT,
    )
    .await
    .unwrap();
    assert_eq!(fs::read(plain).unwrap(), b"fixture");
}

#[tokio::test]
async fn malformed_headers_and_unsafe_output_directories_fail_closed() {
    let directory = directory();
    let source = directory.path().join("bad.age");
    let output = directory.path().join("output");
    fs::write(&source, vec![b'x'; config::MAX_AGE_HEADER_BYTES + 1]).unwrap();
    assert!(
        envelope::open(
            &source,
            &output,
            Unlock::Passphrase(SecretString::from("fixture".to_owned())),
            config::DEFAULT_TIMEOUT
        )
        .await
        .is_err()
    );
    assert!(!output.exists());
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(envelope::keygen(&output).await.is_err());
}
