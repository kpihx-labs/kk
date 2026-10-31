use super::*;
use std::{
    fs,
    sync::{Arc, Barrier},
};

#[test]
fn revision_strings_are_canonical_lossless_and_legacy_missing_is_zero() {
    let legacy: Settings =
        serde_json::from_value(serde_json::json!({"display_name":"Legacy"})).unwrap();
    assert_eq!(legacy.revision, 0);
    assert!(!legacy.share_display_name);
    assert_eq!(serde_json::to_value(&legacy).unwrap()["revision"], "0");
    let settings = Settings {
        revision: u64::MAX,
        ..Settings::default()
    };
    let value = serde_json::to_value(settings).unwrap();
    assert_eq!(value["revision"], u64::MAX.to_string());
    assert_eq!(
        serde_json::from_value::<Settings>(value).unwrap().revision,
        u64::MAX
    );
    assert_eq!(
        Settings::parse_revision("9007199254740993").unwrap(),
        9_007_199_254_740_993
    );
    for bad in [
        "",
        "00",
        "01",
        "+1",
        "-1",
        " 1",
        "1 ",
        "1.0",
        "1e1",
        "١",
        "18446744073709551616",
    ] {
        assert!(Settings::parse_revision(bad).is_err(), "{bad}");
    }
}

#[test]
fn stale_explicit_opt_in_and_stale_display_changes_conflict_even_after_aba() {
    let root = crate::test_support::tempdir().unwrap();
    let directory = root.path().join("profile");
    let stale = Settings::update(&directory, Some("Original".into()), None).unwrap();
    let on = Settings::update_with(&directory, |settings| {
        settings.share_display_name = true;
        Ok(())
    })
    .unwrap();
    assert_eq!(on.revision, stale.revision + 1);
    let off = Settings::update_with(&directory, |settings| {
        settings.share_display_name = false;
        Ok(())
    })
    .unwrap();
    assert_eq!(off.revision, stale.revision + 2);
    let saved = fs::read(directory.join("settings.json")).unwrap();
    let error = Settings::update_with_revision(&directory, stale.revision, |_| {
        panic!("stale callback ran")
    })
    .unwrap_err();
    assert_eq!(error.code(), "SETTINGS_CONFLICT");
    assert_eq!(fs::read(directory.join("settings.json")).unwrap(), saved);
    Settings::update_with(&directory, |settings| {
        settings.share_display_name = true;
        Ok(())
    })
    .unwrap();
    let saved = fs::read(directory.join("settings.json")).unwrap();
    let error = Settings::update_with_revision(&directory, off.revision, |settings| {
        settings.display_name = "Stale display".into();
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code(), "SETTINGS_CONFLICT");
    assert_eq!(fs::read(directory.join("settings.json")).unwrap(), saved);
    assert_eq!(Settings::load(&directory).unwrap().display_name, "Original");
}

#[test]
fn concurrent_cas_has_one_winner_and_unconditional_updates_increment_actual_changes_only() {
    let root = crate::test_support::tempdir().unwrap();
    let directory = root.path().join("profile");
    let initial = Settings::update(&directory, Some("Original".into()), None).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = [SidebarMode::Compact, SidebarMode::Auto]
        .into_iter()
        .map(|mode| {
            let (directory, barrier) = (directory.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                Settings::update_with_revision(&directory, initial.revision, |settings| {
                    settings.sidebar_mode = mode;
                    Ok(())
                })
            })
        })
        .collect();
    let results: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(Error::SettingsConflict)))
            .count(),
        1
    );
    let current = Settings::load(&directory).unwrap();
    assert_eq!(current.revision, initial.revision + 1);
    let no_op = Settings::update_with(&directory, |_| Ok(())).unwrap();
    assert_eq!(no_op.revision, current.revision);
    let changed = Settings::update(&directory, Some("CLI name".into()), None).unwrap();
    assert_eq!(changed.revision, current.revision + 1);
    assert_eq!(changed.sidebar_mode, current.sidebar_mode);
    assert_eq!(changed.appearance, current.appearance);
    assert_eq!(
        Settings::update(&directory, None, None).unwrap().revision,
        changed.revision
    );
}

#[test]
fn malformed_revisions_overflow_and_callback_override_never_change_saved_bytes() {
    let root = crate::test_support::tempdir().unwrap();
    let directory = root.path().join("profile");
    let current = Settings::update(&directory, Some("Original".into()), None).unwrap();
    let path = directory.join("settings.json");
    let saved = fs::read(&path).unwrap();
    assert!(
        Settings::update_with(&directory, |settings| {
            settings.revision = 0;
            Ok(())
        })
        .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), saved);
    for bad in [
        serde_json::json!(null),
        serde_json::json!(1),
        serde_json::json!(true),
        serde_json::json!("01"),
        serde_json::json!("18446744073709551616"),
    ] {
        let mut value = serde_json::to_value(&current).unwrap();
        value["revision"] = bad;
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(Settings::load(&directory).is_err());
        assert!(
            Settings::update_with(&directory, |_| panic!("corrupt metadata reached callback"))
                .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    let maximum = Settings {
        revision: u64::MAX,
        ..current
    };
    let bytes = serde_json::to_vec(&maximum).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert!(
        Settings::update_with_revision(&directory, u64::MAX, |settings| {
            settings.share_display_name = true;
            Ok(())
        })
        .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(
        Settings::update_with_revision(&directory, u64::MAX, |_| Ok(()))
            .unwrap()
            .revision,
        u64::MAX
    );
}
