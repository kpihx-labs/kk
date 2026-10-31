use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn writable_stage_is_published_exclusively_then_frozen() {
    let temporary = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let parent = Dir(fs::open(
        temporary.path().canonicalize().unwrap(),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .unwrap());
    let stage = parent.child("stage", true).unwrap();
    let nested = stage.child("nested", true).unwrap();
    let cancel = super::super::Cancel::default();
    nested
        .write("notice", b"public fixture", false, &cancel)
        .unwrap();
    stage.freeze_descendants(&cancel).unwrap();
    assert_eq!(fs::fstat(&stage.0).unwrap().st_mode & 0o7777, 0o700);
    assert_eq!(fs::fstat(&nested.0).unwrap().st_mode & 0o7777, 0o500);
    fs::renameat_with(
        &parent.0,
        "stage",
        &parent.0,
        "published",
        fs::RenameFlags::NOREPLACE,
    )
    .unwrap();
    assert_eq!(fs::fstat(&stage.0).unwrap().st_mode & 0o7777, 0o700);
    stage.freeze_root(&cancel).unwrap();
    assert_eq!(fs::fstat(&stage.0).unwrap().st_mode & 0o7777, 0o500);
    let other = parent.child("other", true).unwrap();
    assert!(
        fs::renameat_with(
            &parent.0,
            "other",
            &parent.0,
            "published",
            fs::RenameFlags::NOREPLACE
        )
        .is_err()
    );
    assert_eq!(
        fs::statat(&parent.0, "published", AtFlags::SYMLINK_NOFOLLOW)
            .unwrap()
            .st_ino,
        fs::fstat(&stage.0).unwrap().st_ino
    );
    // Restore only these owned fixture directories for automatic cleanup.
    for dir in [&nested, &stage, &other] {
        fs::fchmod(&dir.0, Mode::from_raw_mode(0o700)).unwrap();
    }
}

#[test]
fn post_publication_finalization_failure_leaves_visible_unfrozen_version() {
    let temporary = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let parent = Dir(fs::open(
        temporary.path().canonicalize().unwrap(),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .unwrap());
    let stage = parent.child("stage", true).unwrap();
    let cancel = super::super::Cancel::default();
    stage.freeze_descendants(&cancel).unwrap();
    fs::renameat_with(
        &parent.0,
        "stage",
        &parent.0,
        "published",
        fs::RenameFlags::NOREPLACE,
    )
    .unwrap();
    cancel.0.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(stage.freeze_root(&cancel).is_err());
    assert_eq!(fs::fstat(&stage.0).unwrap().st_mode & 0o7777, 0o700);
    assert!(parent.open("published").is_ok());
}
