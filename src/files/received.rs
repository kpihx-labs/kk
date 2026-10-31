//! Read a completed inbox object by content-bound ID, never a client pathname.
use super::{FileInfo, ReceivedFile, config};
use crate::{Error, Result, transport};
use std::{path::Path, time::Duration};
use tokio::{fs::File, io::AsyncSeekExt};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::storage::open_received as open;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn open(_directory: &Path, _id: &str) -> Result<File> {
    Err(Error::UnsupportedPlatform)
}

impl ReceivedFile {
    pub fn into_send_source(self) -> super::SendSource {
        super::SendSource {
            file: self.file,
            expected: Some(self.info),
        }
    }

    pub fn into_file(self) -> File {
        self.file
    }

    /// Consume this verified inbox handle as the existing stream sender's source.
    /// The same read-only descriptor (and its shared flock) moves with the matching
    /// FileInfo. No pathname is opened, and the file is already rewound by
    /// open_received. Keep this descriptor alive for the entire send/receipt future.
    pub fn into_source(self) -> (File, FileInfo) {
        (self.file, self.info)
    }
}

/// `directory` is selected by the local owner, not supplied by a browser. `id`
/// is the opaque ID returned by the inbox listing. No creation or publication is
/// performed, and no unfinished staging object can be selected.
///
/// Hashing is bounded by both size and deadline. Verification concerns the opened
/// inode at access time; it is not a promise against a same-user process mutating
/// an already-open inode while it is subsequently streamed.
pub(crate) async fn open_received(
    directory: &Path,
    id: &str,
    max_bytes: u64,
    deadline: Duration,
) -> Result<ReceivedFile> {
    transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
    if max_bytes > config::MAX_FILE_BYTES {
        return Err(Error::FileTooLarge);
    }
    let expected = received_hash(id)?;
    tokio::time::timeout(deadline, async {
        let mut file = open(directory, id)?;
        let bytes = file.metadata().await?.len();
        if bytes > max_bytes {
            return Err(Error::FileTooLarge);
        }
        let sha256 = super::digest(&mut file, bytes).await?;
        if sha256 != expected || file.metadata().await?.len() != bytes {
            return Err(Error::Integrity);
        }
        file.seek(std::io::SeekFrom::Start(0)).await?;
        Ok(ReceivedFile {
            info: FileInfo { bytes, sha256 },
            file,
        })
    })
    .await
    .map_err(|_| Error::Timeout)?
}

fn received_hash(id: &str) -> Result<[u8; 32]> {
    if id.len() != 133 {
        return Err(Error::Destination("invalid inbox object ID"));
    }
    let (sender, digest) = id
        .strip_suffix(".bin")
        .and_then(|stem| stem.split_once('-'))
        .ok_or(Error::Destination("invalid inbox object ID"))?;
    super::parse_hash(sender)?;
    super::parse_hash(digest)
}

/// Delete only a completed local entry, not remote storage or a budget charge.
pub(crate) async fn delete_received(
    directory: &Path,
    id: &str,
    confirm: bool,
    deadline: Duration,
    lease: impl Send + 'static,
) -> Result<bool> {
    if !confirm {
        return Err(Error::Denied);
    }
    transport::validate_duration(deadline, config::DEFAULT_TIMEOUT)?;
    received_hash(id)?;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (directory, lease);
        Err(Error::UnsupportedPlatform)
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    tokio::time::timeout(
        deadline,
        super::storage::delete_received(directory, id, lease),
    )
    .await
    .map_err(|_| Error::Timeout)?
}

#[derive(serde::Serialize)]
struct ReceivedEntry {
    id: String,
    bytes: u64,
}

/// Owner-local references only, not proof that content still matches its ID.
#[derive(serde::Serialize)]
pub(crate) struct ReceivedInventory {
    files: Vec<ReceivedEntry>,
    truncated: bool,
    unavailable: usize,
    content_verified: bool,
}

pub(crate) async fn received_inventory(
    directory: &Path,
    deadline: Duration,
) -> Result<ReceivedInventory> {
    transport::validate_duration(deadline, config::DEFAULT_TIMEOUT)?;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = directory;
        Err(Error::UnsupportedPlatform)
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    tokio::time::timeout(deadline, async {
        let directory = super::storage::received_directory(directory)?;
        let entries = rustix::fs::Dir::read_from(&directory)?;
        let limit = config::NODE_FILE_COUNT_BUDGET as usize;
        // Final object, staging, binding, lock; plus dot directory entries.
        let scan_limit = limit.saturating_mul(4).saturating_add(2);
        let mut inventory = ReceivedInventory {
            files: Vec::new(),
            truncated: false,
            unavailable: 0,
            content_verified: false,
        };
        for (index, entry) in entries.take(scan_limit.saturating_add(1)).enumerate() {
            if index == scan_limit {
                inventory.truncated = true;
                break;
            }
            let entry = entry?;
            let Ok(id) = entry.file_name().to_str() else {
                continue;
            };
            if received_hash(id).is_err() {
                continue;
            }
            if inventory.files.len() == limit {
                inventory.truncated = true;
                break;
            }
            match super::storage::received_at(&directory, id) {
                Ok(file) => inventory.files.push(ReceivedEntry {
                    id: id.to_owned(),
                    bytes: file.metadata().await?.len(),
                }),
                Err(_) => inventory.unavailable += 1,
            }
        }
        inventory
            .files
            .sort_by(|left, right| left.id.cmp(&right.id));
        Ok(inventory)
    })
    .await
    .map_err(|_| Error::Timeout)?
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
    use tokio::io::AsyncReadExt;

    fn fixture(bytes: &[u8]) -> (tempfile::TempDir, String) {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let info = FileInfo {
            bytes: bytes.len() as u64,
            sha256: Sha256::digest(bytes).into(),
        };
        let id = format!("{}-{}.bin", "a".repeat(64), info.hash_hex());
        std::fs::write(root.path().join(&id), bytes).unwrap();
        std::fs::set_permissions(
            root.path().join(&id),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        (root, id)
    }

    #[tokio::test]
    async fn explicit_delete_refuses_in_use_aliases_and_staging_without_touching_other_data() {
        isolated_lock_case("explicit_delete_lock_child").await;
    }

    #[tokio::test]
    #[ignore = "subprocess-only: immediate writer release must not share unrelated fork aliases"]
    async fn explicit_delete_lock_child() {
        let (root, id) = fixture(b"saved copy");
        let deadline = config::DEFAULT_TIMEOUT;
        let remove = |id: String, confirm| {
            let directory = root.path().to_owned();
            async move { delete_received(&directory, &id, confirm, deadline, ()).await }
        };
        let other = root.path().join("unrelated.bin");
        std::fs::write(&other, b"keep this").unwrap();
        assert!(matches!(
            remove(id.clone(), false).await,
            Err(Error::Denied)
        ));

        let writer = std::fs::File::open(root.path().join(&id)).unwrap();
        rustix::fs::flock(
            &writer,
            rustix::fs::FlockOperation::NonBlockingLockExclusive,
        )
        .unwrap();
        assert!(remove(id.clone(), true).await.is_err());
        drop(writer);
        let reader = open_received(root.path(), &id, 64, deadline).await.unwrap();
        assert!(remove(id.clone(), true).await.is_err());
        drop(reader.into_file().into_std().await);

        let linked = format!("{}-{}.bin", "b".repeat(64), "c".repeat(64));
        symlink(root.path().join(&id), root.path().join(&linked)).unwrap();
        assert!(remove(linked.clone(), true).await.is_err());
        assert!(
            std::fs::symlink_metadata(root.path().join(&linked))
                .unwrap()
                .is_symlink()
        );
        std::fs::remove_file(root.path().join(&linked)).unwrap();
        std::fs::hard_link(root.path().join(&id), root.path().join(&linked)).unwrap();
        assert!(remove(id.clone(), true).await.is_err());
        assert!(remove(linked.clone(), true).await.is_err());
        std::fs::remove_file(root.path().join(&linked)).unwrap();

        let partial_id = format!("{id}.kk-part");
        std::fs::write(root.path().join(&partial_id), b"staging").unwrap();
        assert!(remove(partial_id.clone(), true).await.is_err());
        assert!(remove(id.clone(), true).await.is_err());
        assert_eq!(std::fs::read(root.path().join(&id)).unwrap(), b"saved copy");
        assert_eq!(
            std::fs::read(root.path().join(&partial_id)).unwrap(),
            b"staging"
        );
        std::fs::remove_file(root.path().join(partial_id)).unwrap();

        // Removing a corrupt local copy must not require laundering it through
        // the verified-read/send API or trusting a replacement content hash.
        std::fs::write(root.path().join(&id), b"corrupt copy").unwrap();
        assert!(remove(id.clone(), true).await.unwrap());
        assert!(!root.path().join(&id).exists());
        assert!(!remove(id, true).await.unwrap());
        assert_eq!(std::fs::read(other).unwrap(), b"keep this");
    }

    #[tokio::test]
    async fn inventory_is_bounded_metadata_only_and_excludes_staging_and_links() {
        let (root, id) = fixture(b"unchanged");
        std::fs::write(root.path().join(&id), b"tampered!").unwrap();
        std::fs::write(root.path().join(format!("{id}.kk-part")), b"staging").unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let outside = elsewhere.path().join("not-an-inbox-object");
        std::fs::write(&outside, b"outside content").unwrap();
        let linked_id = format!("{}-{}.bin", "a".repeat(64), "b".repeat(64));
        symlink(&outside, root.path().join(linked_id)).unwrap();

        let inventory = received_inventory(root.path(), config::DEFAULT_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(inventory.files.len(), 1);
        assert_eq!(inventory.files[0].id, id);
        assert_eq!(inventory.files[0].bytes, 9);
        assert_eq!(inventory.unavailable, 1);
        assert!(!inventory.content_verified);
        assert!(!inventory.truncated);
        assert!(matches!(
            open_received(root.path(), &id, 64, config::DEFAULT_TIMEOUT).await,
            Err(Error::Integrity)
        ));

        let limit = config::NODE_FILE_COUNT_BUDGET as usize;
        for index in 0..limit + 1 {
            let path = root
                .path()
                .join(format!("{}-{index:064x}.bin", "a".repeat(64)));
            std::fs::write(&path, b"").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let bounded = received_inventory(root.path(), config::DEFAULT_TIMEOUT)
            .await
            .unwrap();
        assert!(bounded.files.len() <= limit);
        assert!(bounded.truncated);
        assert!(!bounded.content_verified);
        assert_eq!(std::fs::read(root.path().join(id)).unwrap(), b"tampered!");
        assert_eq!(std::fs::read(outside).unwrap(), b"outside content");
    }

    #[tokio::test]
    async fn verified_handle_is_rewound_and_survives_path_replacement() {
        let (root, id) = fixture(b"verified image bytes");
        let received = open_received(root.path(), &id, 64, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(received.info.bytes, 20);
        std::fs::rename(root.path().join(&id), root.path().join("retained-original")).unwrap();
        std::fs::write(root.path().join(&id), b"replacement").unwrap();
        let mut bytes = Vec::new();
        received.into_file().read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes, b"verified image bytes");
    }

    async fn isolated_lock_case(case: &str) {
        // Physical flock release must not inherit unrelated concurrent tests'
        // transient fork aliases. Keep every original immediate-release assertion
        // in an isolated executable, just like the socket-lifetime qualification.
        let qualified = format!("files::received::tests::{case}");
        let output = tokio::time::timeout(
            config::DEFAULT_TIMEOUT,
            tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", qualified.as_str(), "--ignored", "--nocapture"])
                .kill_on_drop(true)
                .output(),
        )
        .await
        .expect("isolated inbox lock lifetime deadline")
        .unwrap();
        assert!(
            output.status.success(),
            "isolated inbox lock lifetime failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test]
    async fn forwarding_source_retains_verified_inode_info_and_shared_lock() {
        isolated_lock_case("forwarding_source_lock_child").await;
    }

    #[tokio::test]
    #[ignore = "subprocess-only: immediate flock release must not share unrelated fork aliases"]
    async fn forwarding_source_lock_child() {
        let bytes = b"verified forwarding bytes";
        let (root, id) = fixture(bytes);
        let received = open_received(root.path(), &id, 64, Duration::from_secs(2))
            .await
            .unwrap();
        let original = std::fs::metadata(root.path().join(&id)).unwrap();
        let expected_hash: [u8; 32] = Sha256::digest(bytes).into();
        let retained = root.path().join("retained-original");
        std::fs::rename(root.path().join(&id), &retained).unwrap();
        std::fs::write(root.path().join(&id), b"replacement is not the source").unwrap();

        let (mut source, info) = received.into_source();
        let opened = source.metadata().await.unwrap();
        assert_eq!(
            (opened.dev(), opened.ino()),
            (original.dev(), original.ino())
        );
        assert_eq!(info.bytes, bytes.len() as u64);
        assert_eq!(info.sha256, expected_hash);
        let probe = std::fs::File::open(&retained).unwrap();
        assert!(
            rustix::fs::flock(&probe, rustix::fs::FlockOperation::NonBlockingLockExclusive,)
                .is_err()
        );
        let mut actual = Vec::new();
        source.read_to_end(&mut actual).await.unwrap();
        assert_eq!(actual, bytes);
        assert_eq!(
            std::fs::read(root.path().join(&id)).unwrap(),
            b"replacement is not the source"
        );

        // into_std waits for pending Tokio file work before releasing the actual
        // opened descriptor, so the lifetime assertion is independent of scheduling.
        drop(source.into_std().await);
        rustix::fs::flock(&probe, rustix::fs::FlockOperation::NonBlockingLockExclusive).unwrap();
        assert_eq!(std::fs::read(retained).unwrap(), bytes);
    }

    #[tokio::test]
    async fn tampering_size_limits_and_unfinished_objects_are_refused() {
        let (root, id) = fixture(b"verified");
        assert!(matches!(
            open_received(root.path(), &id, 1, Duration::from_secs(2)).await,
            Err(Error::FileTooLarge)
        ));
        for invalid in ["../escape.bin", "partial.kk-part", "/absolute.bin", ""] {
            assert!(
                open_received(root.path(), invalid, 64, Duration::from_secs(2))
                    .await
                    .is_err()
            );
        }
        std::fs::write(root.path().join(&id), b"tampered").unwrap();
        assert!(matches!(
            open_received(root.path(), &id, 64, Duration::from_secs(2)).await,
            Err(Error::Integrity)
        ));
        assert_eq!(std::fs::read(root.path().join(&id)).unwrap(), b"tampered");
    }

    #[tokio::test]
    async fn aliases_and_unsafe_inboxes_are_refused_without_changing_bytes() {
        let (root, id) = fixture(b"verified");
        let retained = root.path().join("retained-original");
        std::fs::rename(root.path().join(&id), &retained).unwrap();
        symlink(&retained, root.path().join(&id)).unwrap();
        assert!(
            open_received(root.path(), &id, 64, Duration::from_secs(2))
                .await
                .is_err()
        );
        let (other, other_id) = fixture(b"verified");
        std::fs::hard_link(other.path().join(&other_id), other.path().join("alias")).unwrap();
        assert!(
            open_received(other.path(), &other_id, 64, Duration::from_secs(2))
                .await
                .is_err()
        );
        let (unsafe_root, unsafe_id) = fixture(b"verified");
        std::fs::set_permissions(unsafe_root.path(), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        assert!(
            open_received(unsafe_root.path(), &unsafe_id, 64, Duration::from_secs(2))
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(retained).unwrap(), b"verified");
    }

    #[tokio::test]
    async fn active_writer_lock_prevents_read_access() {
        isolated_lock_case("active_writer_lock_child").await;
    }

    #[tokio::test]
    #[ignore = "subprocess-only: immediate writer release must not share unrelated fork aliases"]
    async fn active_writer_lock_child() {
        let (root, id) = fixture(b"verified");
        let writer = std::fs::File::open(root.path().join(&id)).unwrap();
        rustix::fs::flock(
            &writer,
            rustix::fs::FlockOperation::NonBlockingLockExclusive,
        )
        .unwrap();
        assert!(
            open_received(root.path(), &id, 64, Duration::from_secs(2))
                .await
                .is_err()
        );
        drop(writer);
        assert!(
            open_received(root.path(), &id, 64, Duration::from_secs(2))
                .await
                .is_ok()
        );
    }
}
