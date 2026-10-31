//! Actual native CLI acceptance; compilation alone does not qualify an OS.
#![cfg(any(target_os = "linux", target_os = "macos"))]
mod support;

use serde_json::Value;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output, Stdio},
};

fn run(root: &Path, profile: &Path, arguments: &[&str]) -> Output {
    // macOS ignores XDG_DATA_HOME: isolate HOME too, and always select a path.
    // Keep the OS temporary spelling (including Darwin /var), not a canonical
    // workaround that would hide production ancestor-alias regressions.
    Command::new(env!("CARGO_BIN_EXE_kk"))
        .env_clear()
        .env("HOME", root)
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .arg("--profile")
        .arg(profile)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .expect("execute the native CLI")
}

fn json(root: &Path, profile: &Path, arguments: &[&str]) -> Value {
    let result = run(root, profile, arguments);
    assert!(
        result.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    serde_json::from_slice(&result.stdout).unwrap()
}

fn initialize(root: &Path, profile: &Path) -> Vec<u8> {
    let result = run(root, profile, &["init"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    fs::read(profile.join("identity/certificate.der")).unwrap()
}

#[test]
fn explicit_native_status_never_creates_profile_or_os_catalog() {
    let root = support::private_root();
    let profile = root.path().join("absent");
    let status = json(root.path(), &profile, &["status", "--json"]);
    assert_eq!(status["profile"]["identity"], "not_initialized");
    assert_eq!(status["node"]["state"], "offline");
    assert!(!profile.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn native_profile_metadata_round_trip_preserves_certificate_and_private_modes() {
    let root = support::private_root();
    let profile = root.path().join("profile");
    let certificate = initialize(root.path(), &profile);
    let renamed = json(
        root.path(),
        &profile,
        &["profile", "rename", "Native fixture"],
    );
    assert_eq!(renamed["display_name"], "Native fixture");
    let appearance = json(
        root.path(),
        &profile,
        &["profile", "appearance", "--hue", "210", "--clarity", "65"],
    );
    assert_eq!(appearance["appearance"]["hue"], 210);
    assert_eq!(appearance["appearance"]["clarity"], 65);
    let notifications = json(
        root.path(),
        &profile,
        &["profile", "notifications", "--enable"],
    );
    assert_eq!(notifications["message_notifications"], true);
    let loaded = json(root.path(), &profile, &["profile", "show"]);
    assert_eq!(loaded["settings"]["display_name"], "Native fixture");
    assert_eq!(loaded["settings"]["message_notifications"], true);
    assert_eq!(loaded["settings"]["appearance"], appearance["appearance"]);
    assert_eq!(
        fs::read(profile.join("identity/certificate.der")).unwrap(),
        certificate
    );
    for (path, mode) in [
        (profile.clone(), 0o700),
        (profile.join("identity"), 0o700),
        (profile.join("identity/private-key.der"), 0o600),
    ] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o7777,
            mode
        );
    }
    // No default HOME/Library or XDG catalog may escape this explicit profile.
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn native_corrupt_metadata_and_missing_key_are_preserved_not_regenerated() {
    let root = support::private_root();
    let profile = root.path().join("profile");
    let certificate = initialize(root.path(), &profile);
    let settings = profile.join("settings.json");
    fs::write(&settings, b"{broken").unwrap();
    fs::set_permissions(&settings, fs::Permissions::from_mode(0o600)).unwrap();
    let result = run(root.path(), &profile, &["profile", "show"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert_eq!(fs::read(&settings).unwrap(), b"{broken");
    let key = profile.join("identity/private-key.der");
    let retained = profile.join("identity/retained-private-key.der");
    fs::rename(&key, &retained).unwrap();
    let result = run(root.path(), &profile, &["init"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(!key.exists());
    assert!(retained.is_file());
    assert_eq!(
        fs::read(profile.join("identity/certificate.der")).unwrap(),
        certificate
    );
    assert_eq!(fs::read(settings).unwrap(), b"{broken");
}
