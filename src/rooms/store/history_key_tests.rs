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
    fs::mknodat(
        anchor.directory(),
        NAME,
        fs::FileType::Fifo,
        Mode::RUSR | Mode::WUSR,
        0,
    )
    .unwrap();
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
            assert!(anchor.load().is_err());
            assert!(!dir.path().join(NAME).exists());
            let after = stdfs::metadata(&pending).unwrap();
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                (before.dev(), before.ino(), before.len()),
                (after.dev(), after.ino(), after.len())
            );
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
