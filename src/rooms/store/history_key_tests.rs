use super::*;
use std::{
    fs as stdfs,
    os::unix::fs::{PermissionsExt, symlink},
    sync::{Arc, Barrier},
};

fn fixture() -> (tempfile::TempDir, HistoryKeyAnchor) {
    let dir = tempfile::tempdir().unwrap();
    stdfs::set_permissions(dir.path(), stdfs::Permissions::from_mode(0o700)).unwrap();
    let anchor = HistoryKeyAnchor::open(dir.path(), [1; 32], [2; 32]).unwrap();
    (dir, anchor)
}

#[test]
fn cloning_directory_preserves_retained_descriptor_flags() {
    let (_dir, anchor) = fixture();
    let status = fs::fcntl_getfl(anchor.directory()).unwrap();
    let descriptor = rustix::io::fcntl_getfd(anchor.directory()).unwrap();
    let clone = anchor.directory_file().unwrap();
    assert_eq!(fs::fcntl_getfl(&clone).unwrap(), status);
    assert_eq!(fs::fcntl_getfl(anchor.directory()).unwrap(), status);
    assert_eq!(
        rustix::io::fcntl_getfd(anchor.directory()).unwrap(),
        descriptor
    );
    drop(clone);
    assert_eq!(fs::fcntl_getfl(anchor.directory()).unwrap(), status);
    anchor.verify().unwrap();
}

#[test]
fn pending_file_is_not_adopted_or_changed_by_load() {
    let (dir, anchor) = fixture();
    let pending = dir.path().join(".history-key-synthetic.pending");
    let bytes = b"synthetic incomplete key material";
    stdfs::write(&pending, bytes).unwrap();
    stdfs::set_permissions(&pending, stdfs::Permissions::from_mode(0o600)).unwrap();
    assert!(anchor.load().is_err());
    assert_eq!(stdfs::read(&pending).unwrap(), bytes);
    assert!(!dir.path().join(NAME).exists());
    assert_eq!(stdfs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn empty_partial_and_complete_pending_block_every_initialization_retry() {
    let (source, source_anchor) = fixture();
    source_anchor.initialize().unwrap();
    let complete = zeroize::Zeroizing::new(stdfs::read(source.path().join(NAME)).unwrap());
    for (payload, bytes) in [b"".as_slice(), b"synthetic partial identity", &complete[..]]
        .into_iter()
        .enumerate()
    {
        let (dir, anchor) = fixture();
        let pending = dir
            .path()
            .join(".history-key-0123456789abcdef0123456789abcdef.pending");
        stdfs::write(&pending, bytes).unwrap();
        stdfs::set_permissions(&pending, stdfs::Permissions::from_mode(0o600)).unwrap();
        let before = stdfs::metadata(&pending).unwrap();
        for attempt in 0..3 {
            let fresh = HistoryKeyAnchor::open(dir.path(), [1; 32], [2; 32]).unwrap();
            for (candidate_index, candidate) in [&anchor, &fresh].into_iter().enumerate() {
                match candidate.initialize() {
                    Err(Error::Destination(message))
                        if message.contains("explicit recovery required") => {}
                    Err(error) => panic!(
                        "pending-key refusal: payload={payload}, attempt={attempt}, anchor={candidate_index}: {}: {error}",
                        error.code(),
                    ),
                    Ok(_) => panic!(
                        "pending-key regeneration: payload={payload}, attempt={attempt}, anchor={candidate_index}"
                    ),
                }
                assert!(candidate.load().is_err());
            }
        }
        let after = stdfs::metadata(&pending).unwrap();
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        let after_bytes = zeroize::Zeroizing::new(stdfs::read(&pending).unwrap());
        assert!(after_bytes.as_slice() == bytes); // Never print private bytes on failure.
        assert!(!dir.path().join(NAME).exists());
        assert_eq!(stdfs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn failed_empty_and_partial_writes_require_explicit_recovery() {
    for length in [0, 17] {
        let (dir, anchor) = fixture();
        assert!(
            anchor
                .initialize_with_io(
                    |file, bytes| {
                        file.write_all(&bytes[..length])?;
                        Err(std::io::Error::other("synthetic write failure"))
                    },
                    |_| panic!("failed write must not reach sync or publication"),
                )
                .is_err()
        );
        let pending = stdfs::read_dir(dir.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let before = stdfs::metadata(&pending).unwrap();
        assert_eq!(before.len(), length as u64);
        let bytes = zeroize::Zeroizing::new(stdfs::read(&pending).unwrap());
        for _ in 0..3 {
            assert!(
                matches!(anchor.initialize(), Err(Error::Destination(message)) if message.contains("explicit recovery required"))
            );
        }
        let after = stdfs::metadata(&pending).unwrap();
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        let after_bytes = zeroize::Zeroizing::new(stdfs::read(&pending).unwrap());
        assert!(after_bytes.as_slice() == bytes.as_slice());
        assert!(!dir.path().join(NAME).exists());
        assert_eq!(stdfs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn unsafe_pending_names_types_links_and_modes_are_preserved() {
    for kind in [
        "name",
        "symlink",
        "directory",
        "hardlink",
        "mode",
        "oversize",
        "fifo",
    ] {
        let (dir, anchor) = fixture();
        let name = if kind == "name" {
            ".history-key-malformed.pending"
        } else {
            ".history-key-0123456789abcdef0123456789abcdef.pending"
        };
        let pending = dir.path().join(name);
        match kind {
            "symlink" => symlink("absent", &pending).unwrap(),
            "directory" => stdfs::create_dir(&pending).unwrap(),
            "fifo" => {
                #[cfg(target_os = "linux")]
                fs::mknodat(
                    anchor.directory(),
                    name,
                    fs::FileType::Fifo,
                    Mode::RUSR | Mode::WUSR,
                    0,
                )
                .unwrap();
                #[cfg(not(target_os = "linux"))]
                continue;
            }
            _ => {
                let bytes = vec![b'x'; if kind == "oversize" { LIMIT + 1 } else { 7 }];
                stdfs::write(&pending, bytes).unwrap();
                stdfs::set_permissions(
                    &pending,
                    stdfs::Permissions::from_mode(if kind == "mode" { 0o644 } else { 0o600 }),
                )
                .unwrap();
                if kind == "hardlink" {
                    stdfs::hard_link(&pending, dir.path().join("retained-link")).unwrap();
                }
            }
        }
        let before = stdfs::symlink_metadata(&pending).unwrap();
        assert!(anchor.initialize().is_err());
        let after = stdfs::symlink_metadata(&pending).unwrap();
        assert_eq!(
            (
                before.dev(),
                before.ino(),
                before.mode(),
                before.nlink(),
                before.len()
            ),
            (
                after.dev(),
                after.ino(),
                after.mode(),
                after.nlink(),
                after.len()
            )
        );
        assert!(!dir.path().join(NAME).exists());
    }
}

#[test]
fn initialization_scan_refuses_exhaustion_even_for_unrelated_entries() {
    let (dir, anchor) = fixture();
    for index in 0..INITIALIZE_SCAN_ENTRIES {
        stdfs::write(dir.path().join(format!("unrelated-{index}")), b"").unwrap();
    }
    assert!(
        matches!(anchor.initialize(), Err(Error::Destination(message)) if message.contains("explicit recovery required"))
    );
    assert!(!dir.path().join(NAME).exists());
    assert_eq!(
        stdfs::read_dir(dir.path()).unwrap().count(),
        INITIALIZE_SCAN_ENTRIES
    );
}

#[test]
fn explicit_only_and_round_trip_with_codec() {
    let (dir, anchor) = fixture();
    assert!(anchor.load().is_err());
    assert_eq!(stdfs::read_dir(dir.path()).unwrap().count(), 0);
    let key = anchor.initialize().unwrap();
    let encrypted = crate::envelope::record::seal(
        zeroize::Zeroizing::new(b"synthetic".to_vec()),
        &key.recipient(),
    )
    .unwrap();
    let loaded = anchor.load().unwrap();
    assert_eq!(key.recipient(), loaded.recipient());
    let plaintext = loaded
        .with_identity(|identity| crate::envelope::record::open(&encrypted, identity))
        .unwrap();
    assert_eq!(&plaintext[..], b"synthetic");
    assert!(anchor.initialize().is_err());
    assert_eq!(anchor.load().unwrap().recipient(), key.recipient());
    assert!(anchor.directory_file().is_ok());
}

#[test]
fn public_bindings_and_strict_bounded_format() {
    let (dir, anchor) = fixture();
    anchor.initialize().unwrap();
    for (owner, room) in [([3; 32], [2; 32]), ([1; 32], [3; 32])] {
        let wrong = HistoryKeyAnchor::open(dir.path(), owner, room).unwrap();
        assert!(wrong.load().is_err());
        assert!(wrong.initialize().is_err());
    }
    let path = dir.path().join(NAME);
    // Synthetic material only; keep test-owned key bytes zeroizing too.
    let original = zeroize::Zeroizing::new(stdfs::read(&path).unwrap());
    for bad in [
        b"".as_slice(),
        b"not an age identity",
        &[b'x'; LIMIT + 1],
        b"\xff",
    ] {
        stdfs::write(&path, bad).unwrap();
        assert!(anchor.load().is_err());
        assert!(anchor.initialize().is_err());
        assert_eq!(stdfs::read(&path).unwrap(), bad);
    }
    let mut extra = zeroize::Zeroizing::new(Vec::with_capacity(LIMIT));
    extra.extend_from_slice(&original);
    extra.extend_from_slice(b"\n");
    stdfs::write(&path, &extra).unwrap();
    assert!(anchor.load().is_err());
    stdfs::write(&path, &original).unwrap();
    assert!(anchor.load().is_ok());
}

#[test]
fn rejects_symlinks_hardlinks_and_modes_without_repair() {
    let (dir, anchor) = fixture();
    let key = anchor.initialize().unwrap();
    let path = dir.path().join(NAME);
    let saved = dir.path().join("saved");
    stdfs::rename(&path, &saved).unwrap();
    symlink(&saved, &path).unwrap();
    assert!(anchor.load().is_err());
    assert!(anchor.initialize().is_err());
    stdfs::remove_file(&path).unwrap();
    stdfs::hard_link(&saved, &path).unwrap();
    assert!(anchor.load().is_err());
    assert!(anchor.initialize().is_err());
    stdfs::remove_file(&saved).unwrap();
    for mode in [0o644, 0o400, 0o1600] {
        stdfs::set_permissions(&path, stdfs::Permissions::from_mode(mode)).unwrap();
        assert!(anchor.load().is_err());
    }
    stdfs::set_permissions(&path, stdfs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(anchor.load().unwrap().recipient(), key.recipient());
    stdfs::set_permissions(dir.path(), stdfs::Permissions::from_mode(0o755)).unwrap();
    assert!(anchor.load().is_err());
    assert!(anchor.initialize().is_err());
    stdfs::set_permissions(dir.path(), stdfs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn dangling_link_and_nonregular_file_refuse() {
    let (dir, anchor) = fixture();
    let path = dir.path().join(NAME);
    symlink("absent", &path).unwrap();
    assert!(anchor.initialize().is_err());
    assert!(anchor.load().is_err());
    stdfs::remove_file(&path).unwrap();
    stdfs::create_dir(&path).unwrap();
    assert!(anchor.load().is_err());
    assert!(anchor.initialize().is_err());
    stdfs::remove_dir(&path).unwrap();
    #[cfg(not(target_vendor = "apple"))]
    fs::mknodat(
        anchor.directory(),
        NAME,
        fs::FileType::Fifo,
        Mode::RUSR | Mode::WUSR,
        0,
    )
    .unwrap();
    // Darwin has neither mknodat nor mkfifoat in rustix. Use its native FIFO
    // utility only in this owned temporary directory, with a bounded lifetime.
    #[cfg(target_vendor = "apple")]
    {
        let mut child = std::process::Command::new("mkfifo")
            .args(["-m", "600"])
            .arg(&path)
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "owned FIFO creation failed: {status}");
                break;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("owned FIFO creation exceeded five seconds");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    assert!(anchor.load().is_err()); // NONBLOCK prevents FIFO hangs.
    assert!(anchor.initialize().is_err());
}

#[test]
fn concurrent_initializers_have_exactly_one_winner() {
    let (dir, _) = fixture();
    let gate = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let path = dir.path().to_owned();
            let gate = gate.clone();
            std::thread::spawn(move || {
                let anchor = HistoryKeyAnchor::open(&path, [1; 32], [2; 32]).unwrap();
                gate.wait();
                anchor.initialize().map(|key| key.recipient())
            })
        })
        .collect();
    let winners: Vec<_> = threads
        .into_iter()
        .filter_map(|t| t.join().unwrap().ok())
        .collect();
    assert_eq!(winners.len(), 1);
    let anchor = HistoryKeyAnchor::open(dir.path(), [1; 32], [2; 32]).unwrap();
    assert_eq!(anchor.load().unwrap().recipient(), winners[0]);
    assert_eq!(stdfs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn initialization_lease_ends_before_an_inherited_description_is_closed() {
    let (_dir, anchor) = fixture();
    let lease = anchor.initialization_lease().unwrap();
    let inherited = rustix::io::dup(&lease).unwrap();
    assert!(matches!(
        anchor.initialize(),
        Err(Error::Destination(message)) if message.contains("initialization is busy")
    ));
    drop(lease);
    assert!(
        anchor.initialize().is_ok(),
        "an inactive initializer's duplicated descriptor retained its logical lease"
    );
    drop(inherited);
}

#[test]
fn same_anchor_and_fresh_anchors_cannot_share_the_initialization_lock() {
    let (dir, anchor) = fixture();
    std::thread::scope(|scope| {
        let (staged, staged_rx) = std::sync::mpsc::sync_channel(1);
        let (release, release_rx) = std::sync::mpsc::sync_channel(1);
        let anchor_ref = &anchor;
        let worker = scope.spawn(move || {
            anchor_ref.initialize_with_sync(|_| {
                staged.send(()).unwrap();
                release_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                Err(std::io::Error::other("synthetic retained pending"))
            })
        });
        staged_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let fresh = HistoryKeyAnchor::open(dir.path(), [1; 32], [2; 32]).unwrap();
        for candidate in [&anchor, &fresh] {
            assert!(candidate.initialize().is_err());
        }
        assert_eq!(stdfs::read_dir(dir.path()).unwrap().count(), 1);
        release.send(()).unwrap();
        assert!(worker.join().unwrap().is_err());
    });
    assert!(
        matches!(anchor.initialize(), Err(Error::Destination(message)) if message.contains("explicit recovery required"))
    );
    assert_eq!(stdfs::read_dir(dir.path()).unwrap().count(), 1);
    assert!(!dir.path().join(NAME).exists());
}

#[test]
fn ancestor_and_root_replacement_detected_even_with_same_room_inode() {
    let outer = tempfile::tempdir().unwrap();
    let ancestor = outer.path().join("ancestor");
    let room = ancestor.join("room");
    stdfs::create_dir_all(&room).unwrap();
    stdfs::set_permissions(&room, stdfs::Permissions::from_mode(0o700)).unwrap();
    let anchor = HistoryKeyAnchor::open(&room, [1; 32], [2; 32]).unwrap();
    anchor.initialize().unwrap();
    let old = outer.path().join("old");
    stdfs::rename(&ancestor, &old).unwrap();
    stdfs::create_dir(&ancestor).unwrap();
    stdfs::rename(old.join("room"), &room).unwrap();
    assert!(anchor.verify().is_err());
    assert!(anchor.load().is_err());
    assert!(anchor.initialize().is_err());
    let fresh = HistoryKeyAnchor::open(&room, [1; 32], [2; 32]).unwrap();
    let moved = ancestor.join("moved");
    stdfs::rename(&room, &moved).unwrap();
    symlink(&moved, &room).unwrap();
    assert!(fresh.verify().is_err());
    assert!(HistoryKeyAnchor::open(&room, [1; 32], [2; 32]).is_err());
}

#[test]
fn retained_descriptor_does_not_follow_publication_path_swap() {
    let (dir, anchor) = fixture();
    let parent = dir.path().parent().unwrap();
    let moved = parent.join(format!(
        "{}-moved",
        dir.path().file_name().unwrap().to_string_lossy()
    ));
    // Move and replace only our synthetic directory during the file-sync boundary.
    let result = anchor.initialize_with_sync(|file| {
        file.sync_all()?;
        stdfs::rename(dir.path(), &moved)?;
        stdfs::create_dir(dir.path())?;
        stdfs::set_permissions(dir.path(), stdfs::Permissions::from_mode(0o700))
    });
    assert!(result.is_err());
    assert!(!dir.path().join(NAME).exists());
    assert!(!moved.join(NAME).exists());
    assert_eq!(stdfs::read_dir(&moved).unwrap().count(), 1); // Preserved pending key.
    stdfs::remove_dir(dir.path()).unwrap();
    stdfs::rename(moved, dir.path()).unwrap();
}

#[test]
fn sync_failures_preserve_pending_or_published_key_and_never_overwrite() {
    for fail_at in [1, 2] {
        let (dir, anchor) = fixture();
        let mut count = 0;
        assert!(
            anchor
                .initialize_with_sync(|file| {
                    count += 1;
                    if count == fail_at {
                        return Err(std::io::Error::other("synthetic sync failure"));
                    }
                    file.sync_all()
                })
                .is_err()
        );
        assert_eq!(stdfs::read_dir(dir.path()).unwrap().count(), 1);
        if fail_at == 1 {
            let pending = stdfs::read_dir(dir.path())
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            let before = stdfs::metadata(&pending).unwrap();
            let bytes = zeroize::Zeroizing::new(stdfs::read(&pending).unwrap());
            assert!(anchor.load().is_err());
            assert!(
                matches!(anchor.initialize(), Err(Error::Destination(message)) if message.contains("explicit recovery required"))
            );
            assert!(!dir.path().join(NAME).exists());
            let after = stdfs::metadata(&pending).unwrap();
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                (before.dev(), before.ino(), before.len()),
                (after.dev(), after.ino(), after.len())
            );
            let after_bytes = zeroize::Zeroizing::new(stdfs::read(&pending).unwrap());
            assert!(bytes.as_slice() == after_bytes.as_slice());
            assert_eq!(stdfs::read_dir(dir.path()).unwrap().count(), 1);
        } else {
            let recipient = anchor.load().unwrap().recipient();
            assert!(anchor.initialize().is_err());
            assert_eq!(anchor.load().unwrap().recipient(), recipient);
        }
    }
}

#[test]
fn rejects_relative_parent_paths_and_ancestor_symlinks() {
    let (dir, _) = fixture();
    assert!(HistoryKeyAnchor::open(Path::new("relative"), [1; 32], [2; 32]).is_err());
    assert!(HistoryKeyAnchor::open(&dir.path().join(".."), [1; 32], [2; 32]).is_err());
    let outer = tempfile::tempdir().unwrap();
    let link = outer.path().join("link");
    symlink(dir.path(), &link).unwrap();
    assert!(HistoryKeyAnchor::open(&link, [1; 32], [2; 32]).is_err());
}

#[test]
fn competing_publication_after_existence_check_never_clobbers() {
    let (source, source_anchor) = fixture();
    let winner = source_anchor.initialize().unwrap().recipient();
    let (target, anchor) = fixture();
    assert!(
        anchor
            .initialize_with_sync(|file| {
                file.sync_all()?;
                stdfs::copy(source.path().join(NAME), target.path().join(NAME))?;
                Ok(())
            })
            .is_err()
    );
    assert_eq!(anchor.load().unwrap().recipient(), winner);
    assert_eq!(stdfs::read_dir(target.path()).unwrap().count(), 2);
}

#[test]
fn swapped_staging_name_is_not_published() {
    let (dir, anchor) = fixture();
    assert!(
        anchor
            .initialize_with_sync(|file| {
                file.sync_all()?;
                let pending = stdfs::read_dir(dir.path())?.next().unwrap()?.path();
                stdfs::rename(&pending, dir.path().join("retained-original"))?;
                stdfs::write(&pending, b"synthetic replacement")?;
                stdfs::set_permissions(&pending, stdfs::Permissions::from_mode(0o600))
            })
            .is_err()
    );
    assert!(!dir.path().join(NAME).exists());
    assert_eq!(stdfs::read_dir(dir.path()).unwrap().count(), 2);
}
