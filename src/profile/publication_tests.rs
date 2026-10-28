use super::*;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

const CHILD: &str = "KK_TEST_PROFILE_PUBLICATION_DIRECTORY";

#[test]
fn settings_opt_out_child() {
    let Some(directory) = std::env::var_os(CHILD) else {
        return;
    };
    Settings::update_with(Path::new(&directory), |settings| {
        settings.share_display_name = false;
        Ok(())
    })
    .unwrap();
}

fn setup(directory: &Path) {
    Settings::update_with(directory, |settings| {
        settings.display_name = "Old shared name".into();
        settings.share_display_name = true;
        settings.sidebar_mode = SidebarMode::Compact;
        settings.appearance.clarity = 37;
        Ok(())
    })
    .unwrap();
}

#[test]
fn completed_cross_process_opt_out_and_opt_in_aba_revoke_the_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("profile");
    setup(&directory);
    let publication = Settings::publication(&directory).unwrap().unwrap();
    assert_eq!(publication.name(), "Old shared name");
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "profile::settings::publication_tests::settings_opt_out_child",
            "--nocapture",
        ])
        .env(CHILD, &directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let called = AtomicBool::new(false);
    assert!(
        publication
            .try_publish(|| called.store(true, Ordering::SeqCst))
            .is_err()
    );
    assert!(!called.load(Ordering::SeqCst));
    assert!(Settings::publication(&directory).unwrap().is_none());
    let current = Settings::load(&directory).unwrap();
    assert!(!current.share_display_name);
    assert_eq!(current.sidebar_mode, SidebarMode::Compact);
    assert_eq!(current.appearance.clarity, 37);
    assert_eq!(current.display_name, "Old shared name");
    Settings::update_with(&directory, |settings| {
        settings.share_display_name = true;
        Ok(())
    })
    .unwrap();
    assert!(
        publication.try_publish(|| ()).is_err(),
        "opting back in cannot revive an old inode snapshot"
    );
    Settings::publication(&directory)
        .unwrap()
        .unwrap()
        .try_publish(|| ())
        .unwrap();
}

#[test]
fn publication_is_noncreating_and_writer_contention_never_waits() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("absent");
    assert!(Settings::publication(&directory).unwrap().is_none());
    assert!(!directory.exists());
    setup(&directory);
    let publication = Settings::publication(&directory).unwrap().unwrap();
    let lock = fs::File::open(directory.join("settings.json.lock")).unwrap();
    rustix::fs::flock(&lock, rustix::fs::FlockOperation::LockExclusive).unwrap();
    let started = std::time::Instant::now();
    assert!(
        publication
            .try_publish(|| panic!("contended writer gate published"))
            .is_err()
    );
    assert!(started.elapsed() < std::time::Duration::from_millis(250));
    drop(lock);
    assert!(matches!(
        publication
            .try_publish(|| std::task::Poll::<()>::Pending)
            .unwrap(),
        std::task::Poll::Pending
    ));
    // Pending is a return value, not a retained lease.
    Settings::update_with(&directory, |settings| {
        settings.share_display_name = false;
        Ok(())
    })
    .unwrap();
    assert!(publication.try_publish(|| ()).is_err());
}

#[test]
fn directory_lock_replacement_and_symlink_ancestors_are_not_adopted() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("profile");
    setup(&directory);
    let publication = Settings::publication(&directory).unwrap().unwrap();
    fs::rename(&directory, root.path().join("old")).unwrap();
    setup(&directory);
    assert!(publication.try_publish(|| ()).is_err());
    let publication = Settings::publication(&directory).unwrap().unwrap();
    fs::rename(
        directory.join("settings.json.lock"),
        directory.join("old.lock"),
    )
    .unwrap();
    fs::write(directory.join("settings.json.lock"), []).unwrap();
    fs::set_permissions(
        directory.join("settings.json.lock"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(publication.try_publish(|| ()).is_err());
    let parent = root.path().join("parent");
    let nested = parent.join("profile");
    setup(&nested);
    let publication = Settings::publication(&nested).unwrap().unwrap();
    fs::rename(&parent, root.path().join("old-parent")).unwrap();
    std::os::unix::fs::symlink(root.path().join("old-parent"), &parent).unwrap();
    assert!(publication.try_publish(|| ()).is_err());
    assert!(Settings::publication(&nested).is_err());
}

#[test]
fn writer_cannot_report_a_completed_update_on_a_replaced_parent_or_lock() {
    for replace in ["parent", "lock"] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("profile");
        setup(&directory);
        let saved = fs::read(directory.join("settings.json")).unwrap();
        let result = Settings::update_with(&directory, |settings| {
            settings.share_display_name = false;
            if replace == "parent" {
                fs::rename(&directory, root.path().join("old-profile"))?;
                setup(&directory);
            } else {
                fs::rename(
                    directory.join("settings.json.lock"),
                    directory.join("old.lock"),
                )?;
                fs::write(directory.join("settings.json.lock"), [])?;
                fs::set_permissions(
                    directory.join("settings.json.lock"),
                    fs::Permissions::from_mode(0o600),
                )?;
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(fs::read(directory.join("settings.json")).unwrap(), saved);
        assert!(Settings::load(&directory).unwrap().share_display_name);
        if replace == "parent" {
            assert_eq!(
                fs::read(root.path().join("old-profile/settings.json")).unwrap(),
                saved
            );
        }
    }
}
