use super::*;
use std::{sync::mpsc, thread, time::Duration};

#[test]
fn sharing_defaults_off_for_legacy_settings_and_survives_partial_updates() {
    let legacy: Settings = serde_json::from_str(
        r#"{"display_name":"Legacy","appearance":{"hue":null,"clarity":50,"saturation":20}}"#,
    )
    .unwrap();
    assert!(!legacy.share_display_name);
    assert_eq!(legacy.sidebar_mode, SidebarMode::Expanded);
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("profile");
    assert!(!Settings::load(&directory).unwrap().share_display_name);
    Settings::update_with(&directory, |settings| {
        settings.share_display_name = true;
        Ok(())
    })
    .unwrap();
    assert!(
        Settings::update(&directory, Some("New".into()), None)
            .unwrap()
            .share_display_name
    );
    Settings::update_with(&directory, |settings| {
        settings.share_display_name = false;
        Ok(())
    })
    .unwrap();
    assert!(!Settings::load(&directory).unwrap().share_display_name);
}

#[test]
fn sidebar_modes_are_typed_and_preserve_other_settings() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("profile");
    for mode in [
        SidebarMode::Expanded,
        SidebarMode::Compact,
        SidebarMode::Auto,
    ] {
        Settings::update_with(&directory, |settings| {
            settings.sidebar_mode = mode;
            Ok(())
        })
        .unwrap();
        Settings::update(&directory, Some("Retained".into()), None).unwrap();
        let saved = Settings::load(&directory).unwrap();
        assert_eq!(saved.sidebar_mode, mode);
        assert_eq!(saved.display_name, "Retained");
    }
    assert!(serde_json::from_str::<SidebarMode>("\"invalid\"").is_err());
}

#[test]
fn supplied_fields_update_together_and_absent_fields_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("profile");
    let appearance = Appearance {
        hue: Some(130),
        clarity: 75,
        saturation: 30,
    };
    let saved =
        Settings::update(&directory, Some("First".into()), Some(appearance.clone())).unwrap();
    assert_eq!(saved.display_name, "First");
    assert_eq!(saved.appearance, appearance);
    let renamed = Settings::update(&directory, Some("Second".into()), None).unwrap();
    assert_eq!(renamed.display_name, "Second");
    assert_eq!(renamed.appearance, appearance);
    let unchanged = Settings::update(&directory, None, None).unwrap();
    assert_eq!(unchanged.display_name, renamed.display_name);
    assert_eq!(unchanged.appearance, appearance);
}

#[test]
fn concurrent_independent_field_updates_preserve_both_changes() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("profile");
    let directory = directory.as_path();
    let stale = Settings::load(directory).unwrap();
    let (locked_tx, locked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();

    thread::scope(|scope| {
        let rename = scope.spawn(move || {
            // Hold the shared storage lock to stage an in-flight name update.
            Settings::update_with(directory, |current| {
                current.display_name = "Renamed".into();
                locked_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(())
            })
        });
        locked_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let appearance = scope.spawn(move || {
            started_tx.send(()).unwrap();
            Settings::update(
                directory,
                None,
                Some(Appearance {
                    hue: Some(215),
                    ..Appearance::default()
                }),
            )
        });
        started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        release_tx.send(()).unwrap();
        rename.join().unwrap().unwrap();
        appearance.join().unwrap().unwrap();
    });

    assert_ne!(stale.display_name, "Renamed");
    let saved = Settings::load(directory).unwrap();
    assert_eq!(saved.display_name, "Renamed");
    assert_eq!(saved.appearance.hue, Some(215));
    assert_eq!(saved.appearance.clarity, stale.appearance.clarity);
    assert_eq!(saved.appearance.saturation, stale.appearance.saturation);
    assert!(!directory.join("identity").exists());
}

#[test]
fn concurrent_partial_appearance_callbacks_preserve_distinct_subfields() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("profile");
    let directory = directory.as_path();
    let (locked_tx, locked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    thread::scope(|scope| {
        let hue = scope.spawn(move || {
            Settings::update_with(directory, |current| {
                current.appearance.hue = Some(215);
                locked_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(())
            })
        });
        locked_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let clarity = scope.spawn(move || {
            started_tx.send(()).unwrap();
            Settings::update_with(directory, |current| {
                assert_eq!(current.appearance.hue, Some(215));
                current.appearance.clarity = 80;
                Ok(())
            })
        });
        started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        release_tx.send(()).unwrap();
        hue.join().unwrap().unwrap();
        clarity.join().unwrap().unwrap();
    });
    let saved = Settings::load(directory).unwrap();
    assert_eq!(saved.appearance.hue, Some(215));
    assert_eq!(saved.appearance.clarity, 80);
    assert_eq!(
        saved.appearance.saturation,
        Appearance::default().saturation
    );
}

#[test]
fn callback_errors_and_invalid_results_preserve_saved_bytes() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("profile");
    Settings::update(&directory, Some("Retained".into()), None).unwrap();
    let before = std::fs::read(directory.join("settings.json")).unwrap();
    assert!(
        Settings::update_with(&directory, |current| {
            current.display_name = "Discarded".into();
            Err(Error::Cancelled)
        })
        .is_err()
    );
    assert_eq!(
        std::fs::read(directory.join("settings.json")).unwrap(),
        before
    );
    assert!(
        Settings::update_with(&directory, |current| {
            current.appearance.hue = Some(360);
            Ok(())
        })
        .is_err()
    );
    assert_eq!(
        std::fs::read(directory.join("settings.json")).unwrap(),
        before
    );
}

#[test]
fn invalid_updates_leave_all_saved_fields_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("profile");
    let directory = directory.as_path();
    Settings::update(directory, Some("Retained".into()), None).unwrap();
    let before = std::fs::read(directory.join("settings.json")).unwrap();
    assert!(
        Settings::update(
            directory,
            Some("Discarded".into()),
            Some(Appearance {
                clarity: 101,
                ..Appearance::default()
            })
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(directory.join("settings.json")).unwrap(),
        before
    );
    assert!(
        Settings::update(
            directory,
            Some(String::new()),
            Some(Appearance {
                hue: Some(123),
                ..Appearance::default()
            })
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(directory.join("settings.json")).unwrap(),
        before
    );
}

#[test]
fn invalid_existing_settings_are_not_silently_repaired() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("profile");
    let directory = directory.as_path();
    super::super::storage::update(directory, "settings.json", |current: &mut Settings| {
        current.display_name.clear();
        Ok(())
    })
    .unwrap();
    let before = std::fs::read(directory.join("settings.json")).unwrap();
    assert!(Settings::update(directory, Some("Replacement".into()), None).is_err());
    assert_eq!(
        std::fs::read(directory.join("settings.json")).unwrap(),
        before
    );
}
