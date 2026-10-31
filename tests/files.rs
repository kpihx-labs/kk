#![cfg(any(target_os = "linux", target_os = "macos"))]
mod support;

use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
    path::PathBuf,
};

use kk::{
    Error, Identity, Result, config,
    files::{FileReceiver, Receipt, ReceivePolicy, send_file},
};
use sha2::{Digest, Sha256};

struct Case {
    root: tempfile::TempDir,
    a: Identity,
    b: Identity,
    source: PathBuf,
    output: PathBuf,
}
impl Case {
    fn new(bytes: &[u8]) -> Self {
        let root = support::private_root();
        let a = Identity::create(&root.path().join("a")).unwrap();
        let b = Identity::create(&root.path().join("b")).unwrap();
        let source = root.path().join("source");
        fs::write(&source, bytes).unwrap();
        let directory = root.path().join("received");
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let output = directory.join("result");
        Self {
            root,
            a,
            b,
            source,
            output,
        }
    }
    fn sidecar(&self, suffix: &str) -> PathBuf {
        self.output.with_file_name(format!("result.kk-{suffix}"))
    }
    fn seed(&self, prefix: &[u8]) {
        let data = fs::read(&self.source).unwrap();
        let mut metadata = vec![0];
        metadata.extend((data.len() as u64).to_be_bytes());
        metadata.extend(Sha256::digest(&data));
        metadata.extend(Sha256::digest(
            fs::read(self.root.path().join("a/certificate.der")).unwrap(),
        ));
        private_write(&self.sidecar("part"), prefix);
        private_write(&self.sidecar("meta"), &metadata);
    }
    async fn transfer(&self, limit: u64) -> (Result<Receipt>, Result<Receipt>) {
        let receiver = FileReceiver::bind(
            "127.0.0.1:0".parse().unwrap(),
            &self.b,
            &self.a.peer(),
            ReceivePolicy::accept(self.output.clone(), limit).unwrap(),
        )
        .unwrap();
        let peer = self.b.peer();
        tokio::join!(
            send_file(
                receiver.local_addr().unwrap(),
                &self.a,
                &peer,
                &self.source,
                config::DEFAULT_TIMEOUT
            ),
            receiver.receive(config::DEFAULT_TIMEOUT)
        )
    }
}

fn private_write(path: &std::path::Path, bytes: &[u8]) {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

#[tokio::test]
async fn binary_empty_and_chunk_boundaries_roundtrip_and_retry_without_rewriting() {
    for size in [0, 1, 65_536, 65_537, 1_048_576] {
        let data: Vec<_> = (0..size).map(|index| (index % 251) as u8).collect();
        let case = Case::new(&data);
        let (sent, received) = case.transfer(config::DEFAULT_FILE_LIMIT).await;
        assert_eq!(sent.unwrap().resumed_from, 0);
        assert_eq!(received.unwrap().info.bytes, size as u64);
        assert_eq!(fs::read(&case.output).unwrap(), data);
        assert_eq!(
            fs::metadata(&case.output).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!case.sidecar("part").exists() && !case.sidecar("meta").exists());
        let (sent, received) = case.transfer(config::DEFAULT_FILE_LIMIT).await;
        assert_eq!(sent.unwrap().resumed_from, size as u64);
        assert_eq!(received.unwrap().resumed_from, size as u64);
    }
}

#[tokio::test]
async fn a_verified_prefix_is_resumed_not_retransmitted() {
    let data = vec![42; 300_000];
    let case = Case::new(&data);
    case.seed(&data[..70_123]);
    let (sent, received) = case.transfer(config::DEFAULT_FILE_LIMIT).await;
    assert_eq!(sent.unwrap().resumed_from, 70_123);
    assert_eq!(received.unwrap().resumed_from, 70_123);
    assert_eq!(fs::read(case.output).unwrap(), data);
}

#[tokio::test]
async fn corrupt_prefixes_and_changed_offers_preserve_partial_data() {
    for changed_offer in [false, true] {
        let case = Case::new(&vec![42; 100_000]);
        let partial = if changed_offer {
            vec![42; 10_000]
        } else {
            vec![99; 10_000]
        };
        case.seed(&partial);
        if changed_offer {
            fs::write(&case.source, vec![11; 100_000]).unwrap();
        }
        let (sent, received) = case.transfer(config::DEFAULT_FILE_LIMIT).await;
        assert!(sent.is_err() && received.is_err());
        if !changed_offer {
            assert!(matches!(sent, Err(Error::Integrity)));
        } else {
            assert!(matches!(received, Err(Error::Destination(_))));
        }
        assert!(!case.output.exists());
        assert_eq!(fs::read(case.sidecar("part")).unwrap(), partial);
    }
}

#[tokio::test]
async fn oversize_offers_do_not_allocate_destination_state() {
    let case = Case::new(b"too large");
    let (sent, received) = case.transfer(2).await;
    assert!(sent.is_err());
    assert!(matches!(received, Err(Error::FileTooLarge)));
    assert!(
        !case.output.exists() && !case.sidecar("part").exists() && !case.sidecar("meta").exists()
    );
}

#[tokio::test]
async fn existing_destinations_symlinks_hardlinks_and_unsafe_directories_are_not_overwritten() {
    for mode in [
        "existing",
        "symlink",
        "hardlink",
        "directory",
        "meta-symlink",
        "partial-symlink",
        "meta-hardlink",
    ] {
        let case = Case::new(b"new content");
        let victim = case.root.path().join("victim");
        private_write(&victim, b"keep this");
        match mode {
            "existing" => private_write(&case.output, b"keep this"),
            "symlink" => symlink(&victim, &case.output).unwrap(),
            "hardlink" => fs::hard_link(&victim, case.sidecar("part")).unwrap(),
            "meta-symlink" => symlink(&victim, case.sidecar("meta")).unwrap(),
            "partial-symlink" => symlink(&victim, case.sidecar("part")).unwrap(),
            "meta-hardlink" => fs::hard_link(&victim, case.sidecar("meta")).unwrap(),
            _ => fs::set_permissions(
                case.output.parent().unwrap(),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap(),
        }
        let (sent, received) = case.transfer(config::DEFAULT_FILE_LIMIT).await;
        assert!(sent.is_err() && received.is_err(), "{mode}");
        assert_eq!(fs::read(&victim).unwrap(), b"keep this");
        if mode == "existing" {
            assert_eq!(fs::read(case.output).unwrap(), b"keep this");
        }
    }
}

#[tokio::test]
async fn concurrent_partial_writers_are_refused() {
    let case = Case::new(b"abc");
    case.seed(b"a");
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(case.sidecar("part"))
        .unwrap();
    rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive).unwrap();
    let (sent, received) = case.transfer(config::DEFAULT_FILE_LIMIT).await;
    assert!(sent.is_err() && matches!(received, Err(Error::Destination(_))));
    assert_eq!(fs::read(case.sidecar("part")).unwrap(), b"a");
}

#[tokio::test]
async fn another_authorized_peer_cannot_adopt_an_existing_partial() {
    let case = Case::new(b"abc");
    case.seed(b"a");
    let stranger = Identity::create(&case.root.path().join("stranger")).unwrap();
    let receiver = FileReceiver::bind(
        "127.0.0.1:0".parse().unwrap(),
        &case.b,
        &stranger.peer(),
        ReceivePolicy::accept(case.output.clone(), 100).unwrap(),
    )
    .unwrap();
    let peer = case.b.peer();
    let (sent, received) = tokio::join!(
        send_file(
            receiver.local_addr().unwrap(),
            &stranger,
            &peer,
            &case.source,
            config::DEFAULT_TIMEOUT
        ),
        receiver.receive(config::DEFAULT_TIMEOUT)
    );
    assert!(sent.is_err() && matches!(received, Err(Error::Destination(_))));
    assert_eq!(fs::read(case.sidecar("part")).unwrap(), b"a");
}

#[tokio::test]
async fn file_authentication_fails_before_any_staging_write() {
    let case = Case::new(b"fixture");
    let stranger = Identity::create(&case.root.path().join("stranger")).unwrap();
    for wrong_server in [false, true] {
        let expected_client = if wrong_server {
            case.a.peer()
        } else {
            stranger.peer()
        };
        let expected_server = if wrong_server {
            stranger.peer()
        } else {
            case.b.peer()
        };
        let receiver = FileReceiver::bind(
            "127.0.0.1:0".parse().unwrap(),
            &case.b,
            &expected_client,
            ReceivePolicy::accept(case.output.clone(), 100).unwrap(),
        )
        .unwrap();
        let (sent, received) = tokio::join!(
            send_file(
                receiver.local_addr().unwrap(),
                &case.a,
                &expected_server,
                &case.source,
                config::DEFAULT_TIMEOUT
            ),
            receiver.receive(config::DEFAULT_TIMEOUT)
        );
        assert!(sent.is_err() && received.is_err());
        assert!(
            !case.output.exists()
                && !case.sidecar("part").exists()
                && !case.sidecar("meta").exists()
        );
    }
}

#[tokio::test]
async fn nonregular_sources_and_impossible_limits_fail_closed() {
    let case = Case::new(b"abc");
    let link = case.root.path().join("link");
    symlink(&case.source, &link).unwrap();
    assert!(
        kk::files::inspect(&link, config::DEFAULT_TIMEOUT)
            .await
            .is_err()
    );
    assert!(matches!(
        kk::files::inspect(case.root.path(), config::DEFAULT_TIMEOUT).await,
        Err(Error::Configuration(_))
    ));
    assert!(ReceivePolicy::accept(case.output.clone(), config::MAX_FILE_BYTES + 1).is_err());
}
