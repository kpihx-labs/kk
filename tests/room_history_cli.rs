#![cfg(unix)]
//! Actual CLI/backend processes with disposable profiles, never operator data.
mod support;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    net::UdpSocket,
    os::unix::fs::MetadataExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

fn address() -> String {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string()
}
fn output(profile: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(profile)
        .args(args)
        .output()
        .unwrap()
}
fn call(profile: &Path, args: &[&str]) -> String {
    let result = output(profile, args);
    assert!(
        result.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}
fn history(profile: &Path, args: &[&str]) -> Value {
    let mut command = vec!["room", "history", "team"];
    command.extend_from_slice(args);
    serde_json::from_str::<Value>(&call(profile, &command)).unwrap()["result"].take()
}
fn denied(profile: &Path, args: &[&str]) {
    let mut command = vec!["room", "history", "team"];
    command.extend_from_slice(args);
    assert!(
        !output(profile, &command).status.success(),
        "must reject {args:?}"
    );
}
fn send_stdin(profile: &Path, body: &str, sensitive: bool) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kk"));
    command
        .arg("--profile")
        .arg(profile)
        .args(["room", "send", "team", "-"]);
    if sensitive {
        command.arg("--sensitive");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(body.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
struct Process {
    child: Child,
    lines: mpsc::Receiver<String>,
}
impl Process {
    fn start(profile: &Path, args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_kk"))
            .arg("--profile")
            .arg(profile)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (sender, lines) = mpsc::channel();
        for input in [
            Box::new(child.stdout.take().unwrap()) as Box<dyn std::io::Read + Send>,
            Box::new(child.stderr.take().unwrap()),
        ] {
            let sender = sender.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(input).lines() {
                    let Ok(line) = line else {
                        break;
                    };
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        Self { child, lines }
    }
    fn until(&self, prefix: &str) {
        let expires = std::time::Instant::now() + Duration::from_secs(110);
        let mut seen = Vec::new();
        loop {
            match self
                .lines
                .recv_timeout(expires.saturating_duration_since(std::time::Instant::now()))
            {
                Ok(line) if line.starts_with(prefix) => return,
                Ok(line) => seen.push(line),
                Err(error) => panic!("missing {prefix}: {error}; {seen:?}"),
            }
        }
    }
    fn stop(&mut self, profile: &Path) {
        call(profile, &["room", "stop", "team"]);
        assert!(self.child.wait().unwrap().success());
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
fn two_profiles_capture_page_restart_disable_clear_and_exclude_sensitive() {
    let root = support::private_root();
    let alice = root.path().join("alice");
    let bob = root.path().join("bob");
    for profile in [&alice, &bob] {
        call(profile, &["init"]);
    }
    call(&alice, &["room", "create", "team", "--bind", &address()]);
    let mut a = Process::start(&alice, &["room", "serve", "team"]);
    a.until("ROOM SERVING");
    assert_eq!(history(&alice, &["status"]), "Off");
    let invitation = alice.join("invite.json");
    let mut invite = Process::start(
        &alice,
        &[
            "room",
            "invite",
            "team",
            "--bind",
            &address(),
            "--invite-out",
            invitation.to_str().unwrap(),
            "--confirm",
        ],
    );
    invite.until("ROOM INVITATION");
    let mut b = Process::start(
        &bob,
        &[
            "room",
            "join",
            "team",
            "--bind",
            &address(),
            "--invitation",
            invitation.to_str().unwrap(),
            "--confirm",
        ],
    );
    b.until("ROOM SERVING");
    invite.until("{\"ok\":true");
    assert!(invite.child.wait().unwrap().success());
    for profile in [&alice, &bob] {
        assert!(!profile.join("rooms/team/private/history.agekey").exists());
        history(profile, &["enable"]);
    }
    let key = alice.join("rooms/team/private/history.agekey");
    let inode = std::fs::metadata(&key).unwrap().ino();
    let send: Value = serde_json::from_str(&call(
        &alice,
        &["room", "send", "team", "history needle é\n\"one\""],
    ))
    .unwrap();
    assert!(send["result"]["reference"].is_object());
    call(&alice, &["room", "send", "team", "history needle two"]);
    send_stdin(&alice, "excluded secret", true);
    for profile in [&alice, &bob] {
        let first = history(profile, &["search", "needle", "--limit", "1"]);
        assert_eq!(first["rows"].as_array().unwrap().len(), 1);
        assert_eq!(first["has_more"], true);
        let cursor = first["next_after"].as_u64().unwrap().to_string();
        let second = history(
            profile,
            &["search", "needle", "--after-id", &cursor, "--limit", "1"],
        );
        assert_eq!(second["rows"].as_array().unwrap().len(), 1);
        assert!(
            history(profile, &["search", "excluded"])["rows"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    // Restart the owner with the same enrolled key and durable room state.
    a.stop(&alice);
    a = Process::start(&alice, &["room", "serve", "team"]);
    a.until("ROOM SERVING");
    assert_eq!(std::fs::metadata(&key).unwrap().ino(), inode);
    assert_eq!(history(&alice, &["status"])["Enrolled"]["unlocked"], true);
    assert_eq!(
        history(&alice, &["search", "needle"])["rows"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // Worst-case JSON escaping must fit the IPC envelope, not just raw UTF-8.
    let escaped = format!(
        "unicode é{}",
        "\0".repeat(kk::config::MAX_MESSAGE_BYTES - 11)
    );
    send_stdin(&alice, &escaped, false);
    let page = history(&alice, &["search", "unicode"]);
    assert_eq!(page["rows"].as_array().unwrap().len(), 1);
    assert!(serde_json::to_vec(&page).unwrap().len() < kk::config::MAX_RECORD_PLAINTEXT_BYTES);
    history(&alice, &["disable"]);
    call(&alice, &["room", "send", "team", "disabled capture"]);
    assert!(
        history(&alice, &["search", "disabled"])["rows"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    history(&alice, &["clear", "--confirm"]);
    assert!(
        history(&alice, &["search"])["rows"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(std::fs::metadata(&key).unwrap().ino(), inode);
    denied(&alice, &["enable", "--rows", "0"]);
    denied(&alice, &["search", "--limit", "0"]);
    denied(&alice, &["search", "--response-bytes", "999999999"]);
    denied(&alice, &["search", &"x".repeat(257)]);
    denied(&alice, &["clear"]);
    denied(
        &alice,
        &["recover", "--expected-recipient", "not-public", "--confirm"],
    );
    b.stop(&bob);
    a.stop(&alice);
    // Missing enrolled key is a locked history failure, never regeneration.
    let held = root.path().join("held.agekey");
    std::fs::rename(&key, &held).unwrap();
    a = Process::start(&alice, &["room", "serve", "team"]);
    a.until("ROOM SERVING");
    assert_eq!(history(&alice, &["status"])["Enrolled"]["unlocked"], false);
    denied(&alice, &["open"]);
    denied(&alice, &["enable"]);
    denied(&alice, &["search"]);
    assert!(!key.exists());
    a.stop(&alice);
}

#[test]
fn history_ipc_rejects_paths_unknown_fields_and_bypassing_query_bounds() {
    use kk::rooms::local::Command as Ipc;
    for command in [
        json!({"operation":"HistoryOpen", "path":"/tmp/forbidden"}),
        json!({"operation":"HistoryEnable", "policy":{"rows":1,"encrypted_bytes":1024,"retention_seconds":60,"path":"/tmp/forbidden"}}),
        json!({"operation":"HistorySearch", "text":"", "after_id":0,"limit":1,"response_bytes":1024,"source":"private.sqlite3"}),
    ] {
        assert!(serde_json::from_value::<Ipc>(command).is_err());
    }
    let root = support::private_root();
    let profile = root.path().join("raw");
    call(&profile, &["init"]);
    call(&profile, &["room", "create", "team", "--bind", &address()]);
    let mut server = Process::start(&profile, &["room", "serve", "team"]);
    server.until("ROOM SERVING");
    let command = json!({"operation":"HistorySearch","text":"", "after_id":0,"limit":999999,"response_bytes":1024});
    let body = serde_json::to_vec(&command).unwrap();
    let mut socket =
        std::os::unix::net::UnixStream::connect(profile.join("rooms/team/node.sock")).unwrap();
    socket
        .set_read_timeout(Some(kk::config::DEFAULT_TIMEOUT))
        .unwrap();
    socket
        .write_all(&(body.len() as u32).to_be_bytes())
        .unwrap();
    socket.write_all(&body).unwrap();
    use std::io::Read;
    let mut length = [0; 4];
    socket.read_exact(&mut length).unwrap();
    let length = u32::from_be_bytes(length) as usize;
    assert!(length < kk::config::MAX_RECORD_PLAINTEXT_BYTES);
    let mut reply = vec![0; length];
    socket.read_exact(&mut reply).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&reply).unwrap()["ok"],
        false
    );
    assert!(!profile.join("rooms/team/private/history.agekey").exists());
    server.stop(&profile);
}

#[test]
fn orphan_requires_confirmed_exact_public_recipient_without_key_replacement() {
    let root = support::private_root();
    let profile = root.path().join("orphan");
    call(&profile, &["init"]);
    call(&profile, &["room", "create", "team", "--bind", &address()]);
    let mut server = Process::start(&profile, &["room", "serve", "team"]);
    server.until("ROOM SERVING");
    server.stop(&profile);
    // Retain the closed V1 database to model publication-before-enrollment.
    // No SQL is opened by the adapter/test, and no key bytes are read.
    let database = profile.join("rooms/team/private/private.sqlite3");
    let snapshot = root.path().join("v1.sqlite3");
    std::fs::copy(&database, &snapshot).unwrap();
    server = Process::start(&profile, &["room", "serve", "team"]);
    server.until("ROOM SERVING");
    let enrolled = history(&profile, &["enable"]);
    let recipient = enrolled["Enrolled"]["recipient"]
        .as_str()
        .unwrap()
        .to_owned();
    let key = profile.join("rooms/team/private/history.agekey");
    let inode = std::fs::metadata(&key).unwrap().ino();
    server.stop(&profile);
    std::fs::copy(&snapshot, &database).unwrap();
    server = Process::start(&profile, &["room", "serve", "team"]);
    server.until("ROOM SERVING");
    assert_eq!(history(&profile, &["status"]), "Off");
    denied(&profile, &["enable"]);
    denied(&profile, &["recover", "--expected-recipient", &recipient]);
    let wrong = age::x25519::Identity::generate().to_public().to_string();
    denied(
        &profile,
        &["recover", "--expected-recipient", &wrong, "--confirm"],
    );
    let recovered = history(
        &profile,
        &["recover", "--expected-recipient", &recipient, "--confirm"],
    );
    assert_eq!(recovered["Enrolled"]["recipient"], recipient);
    assert_eq!(std::fs::metadata(&key).unwrap().ino(), inode);
    server.stop(&profile);
}
