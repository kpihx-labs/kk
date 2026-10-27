use super::*;

#[test]
fn selection_rechecks_removed_entry_in_locked_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("profile");
    let base = base.as_path();
    let target = base.join("target");
    super::super::private_directory(&target, true).unwrap();
    super::super::storage::update(base, "profiles.json", |catalog: &mut Catalog| {
        catalog.entries.push(Entry {
            id: "target".into(),
            directory: target.clone(),
        });
        Ok(())
    })
    .unwrap();

    select_at(base, "target").unwrap();
    assert_eq!(load(base).unwrap().active.as_deref(), Some("target"));
    select_at(base, "default").unwrap();

    // Deterministically stage the old race: resolution succeeds, then a remover
    // publishes before the selector's locked mutation. The directory still exists.
    let stale = load(base).unwrap();
    assert_eq!(stale.entries[0].directory, target);
    super::super::storage::update(base, "profiles.json", |catalog: &mut Catalog| {
        catalog.entries.clear();
        Ok(())
    })
    .unwrap();
    let before = std::fs::read(base.join("profiles.json")).unwrap();
    let result = super::super::storage::update(base, "profiles.json", |catalog: &mut Catalog| {
        select_locked(base, "target", catalog)
    });
    assert!(matches!(result, Err(Error::Configuration(_))));
    assert_eq!(std::fs::read(base.join("profiles.json")).unwrap(), before);
    assert!(load(base).unwrap().active.is_none());
    assert!(target.is_dir());
    assert!(!target.join("identity").exists());
}

#[test]
fn selection_checks_directory_before_changing_active_profile() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("profile");
    let base = base.as_path();
    super::super::storage::update(base, "profiles.json", |catalog: &mut Catalog| {
        catalog.entries.push(Entry {
            id: "missing".into(),
            directory: base.join("missing"),
        });
        Ok(())
    })
    .unwrap();
    let before = std::fs::read(base.join("profiles.json")).unwrap();
    assert!(select_at(base, "missing").is_err());
    assert_eq!(std::fs::read(base.join("profiles.json")).unwrap(), before);
    assert!(!base.join("missing").exists());
    select_at(base, "default").unwrap();
    assert!(load(base).unwrap().active.is_none());
}

#[test]
fn summary_distinguishes_absent_identity_from_broken_certificate() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("profile");
    let absent = summary("test".into(), directory.clone(), false).unwrap();
    assert!(!absent.initialized);
    assert!(absent.fingerprint.is_none());
    assert!(!directory.exists());

    let identity = directory.join("identity");
    super::super::private_directory(&identity, true).unwrap();
    assert!(matches!(
        summary("test".into(), directory.clone(), false),
        Err(Error::Io(_))
    ));
    assert!(!identity.join("certificate.der").exists());

    let corrupt = b"synthetic invalid public certificate";
    std::fs::write(identity.join("certificate.der"), corrupt).unwrap();
    assert!(matches!(
        summary("test".into(), directory, false),
        Err(Error::Identity(_))
    ));
    assert_eq!(
        std::fs::read(identity.join("certificate.der")).unwrap(),
        corrupt
    );
    assert!(!identity.join("private-key.der").exists());
}
