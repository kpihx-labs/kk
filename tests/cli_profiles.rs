#![cfg(target_os = "linux")]

use serde_json::Value;
use std::{
    path::Path,
    process::{Command, Output},
};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kk"))
        .env("XDG_DATA_HOME", root.join("data"))
        .args(args)
        .output()
        .unwrap()
}

fn success(root: &Path, args: &[&str]) -> Value {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn fresh_status_and_inventory_are_useful_without_creating_state() {
    let root = tempfile::tempdir().unwrap();
    let status = success(root.path(), &["status", "--json"]);
    assert_eq!(status["node"]["state"], "offline");
    assert_eq!(status["profile"]["identity"], "not_initialized");
    assert_eq!(status["profiles"][0]["id"], "default");
    let output = run(root.path(), &["status"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("My profile"));
    assert_eq!(
        success(root.path(), &["profile", "list"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(!root.path().join("data").exists());
}

#[test]
fn named_profiles_select_rename_and_unregister_without_deleting_identity() {
    let root = tempfile::tempdir().unwrap();
    let created = success(
        root.path(),
        &["profile", "create", "alice", "--name", "Alice"],
    );
    assert_eq!(created["settings"]["display_name"], "Alice");
    assert_eq!(created["settings"]["share_display_name"], false);
    let directory = created["directory"].as_str().unwrap();
    let certificate = std::fs::read(Path::new(directory).join("identity/certificate.der")).unwrap();
    let disguised = format!("{directory}/../alice");
    assert!(
        !run(
            root.path(),
            &["profile", "register", "duplicate", &disguised]
        )
        .status
        .success()
    );
    success(root.path(), &["profile", "select", "alice"]);
    let status = success(root.path(), &["status", "--json"]);
    assert_eq!(status["profile"]["settings"]["display_name"], "Alice");
    assert_eq!(status["profile"]["identity"], "ready");
    assert_eq!(
        success(
            root.path(),
            &["--profile-name", "alice", "profile", "rename", "Alice Work"]
        )["display_name"],
        "Alice Work"
    );
    let appearance = success(
        root.path(),
        &["profile", "appearance", "--hue", "210", "--clarity", "65"],
    );
    assert_eq!(appearance["appearance"]["hue"], 210);
    assert_eq!(appearance["appearance"]["clarity"], 65);
    for invalid in [
        vec!["profile", "share-name"],
        vec!["profile", "share-name", "--enable", "--disable"],
    ] {
        assert!(!run(root.path(), &invalid).status.success());
    }
    let shared = success(root.path(), &["profile", "share-name", "--enable"]);
    assert_eq!(shared["share_display_name"], true);
    assert_eq!(shared["display_name"], "Alice Work");
    assert_eq!(shared["appearance"], appearance["appearance"]);
    assert_eq!(
        success(root.path(), &["profile", "rename", "Renamed"])["share_display_name"],
        true
    );
    let disabled = success(root.path(), &["profile", "share-name", "--disable"]);
    assert_eq!(disabled["share_display_name"], false);
    assert_eq!(disabled["display_name"], "Renamed");
    assert_eq!(disabled["appearance"], appearance["appearance"]);
    assert!(
        !run(root.path(), &["profile", "unregister", "alice"])
            .status
            .success()
    );
    assert!(
        !run(root.path(), &["--profile-name", "missing", "status"])
            .status
            .success()
    );
    assert!(
        !run(
            root.path(),
            &["--profile-name", "alice", "--profile", directory, "status"]
        )
        .status
        .success()
    );
    success(root.path(), &["profile", "select", "default"]);
    success(root.path(), &["profile", "unregister", "alice"]);
    assert_eq!(
        std::fs::read(Path::new(directory).join("identity/certificate.der")).unwrap(),
        certificate
    );
    assert!(Path::new(directory).join("settings.json").is_file());
    assert!(
        run(root.path(), &["--profile-name", "default", "init"])
            .status
            .success()
    );
    let base = root.path().join("data/kk");
    assert!(
        !run(
            root.path(),
            &[
                "profile",
                "register",
                "duplicate-default",
                base.to_str().unwrap()
            ]
        )
        .status
        .success()
    );
}

#[test]
fn explicit_path_status_is_noncreating_and_corrupt_metadata_is_preserved() {
    let root = tempfile::tempdir().unwrap();
    let absent = root.path().join("absent");
    let status = success(
        root.path(),
        &["--profile", absent.to_str().unwrap(), "status", "--json"],
    );
    assert_eq!(status["profile"]["identity"], "not_initialized");
    assert!(!absent.exists());
    let created = success(
        root.path(),
        &["profile", "create", "alice", "--name", "Alice"],
    );
    let directory = Path::new(created["directory"].as_str().unwrap());
    let settings = directory.join("settings.json");
    std::fs::write(&settings, b"{broken").unwrap();
    assert!(
        !run(
            root.path(),
            &["--profile-name", "alice", "status", "--json"]
        )
        .status
        .success()
    );
    assert_eq!(std::fs::read(settings).unwrap(), b"{broken");
    let other = success(
        root.path(),
        &["--profile-name", "default", "status", "--json"],
    );
    assert!(other["profiles"].is_null());
    assert!(
        other["profiles_error"]
            .as_str()
            .is_some_and(|error| error.contains("invalid"))
    );
    assert_eq!(other["profile"]["identity"], "not_initialized");
}

#[test]
fn offline_room_inventory_uses_public_metadata_without_loading_the_private_key() {
    let root = tempfile::tempdir().unwrap();
    let created = success(
        root.path(),
        &["profile", "create", "alice", "--name", "Alice"],
    );
    let address = std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    let output = run(
        root.path(),
        &[
            "--profile-name",
            "alice",
            "room",
            "create",
            "team",
            "--title",
            "Study group",
            "--bind",
            &address,
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let directory = Path::new(created["directory"].as_str().unwrap());
    let member = "11".repeat(32);
    let labeled = success(
        root.path(),
        &[
            "--profile-name",
            "alice",
            "profile",
            "label",
            "team",
            &member,
            "Morgan",
        ],
    );
    assert_eq!(labeled["source"], "local_override");
    assert_eq!(labeled["labels"][&member], "Morgan");
    let labels = success(
        root.path(),
        &["--profile-name", "alice", "profile", "labels", "team"],
    );
    assert_eq!(labels[&member], "Morgan");
    let cleared = success(
        root.path(),
        &[
            "--profile-name",
            "alice",
            "profile",
            "label",
            "team",
            &member,
            "--clear",
        ],
    );
    assert_eq!(cleared["labels"], serde_json::json!({}));
    let key = directory.join("identity/private-key.der");
    let retained = directory.join("identity/retained-private-key.der");
    std::fs::rename(&key, &retained).unwrap();
    let status = success(
        root.path(),
        &["--profile-name", "alice", "status", "--json"],
    );
    assert_eq!(status["rooms"][0]["name"], "team");
    assert_eq!(status["rooms"][0]["title"], "Study group");
    assert_eq!(status["rooms"][0]["observation"]["state"], "offline");
    assert!(!key.exists(), "status must never regenerate an identity");
    assert!(retained.is_file());
}

#[test]
fn catalog_mutations_refuse_invalid_existing_state_without_creating_identity() {
    let root = tempfile::tempdir().unwrap();
    success(
        root.path(),
        &["profile", "create", "alice", "--name", "Alice"],
    );
    let base = root.path().join("data/kk");
    let path = base.join("profiles.json");
    let mut catalog: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    catalog["active"] = Value::String("missing".into());
    let bytes = serde_json::to_vec(&catalog).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    for args in [
        vec!["profile", "select", "default"],
        vec!["profile", "unregister", "alice"],
        vec!["profile", "create", "bob", "--name", "Bob"],
    ] {
        assert!(!run(root.path(), &args).status.success());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    assert!(!base.join("profiles/bob").exists());
}
