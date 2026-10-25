#![cfg(target_os = "linux")]

use kk::{Identity, Listener, config};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

#[tokio::test]
async fn sensitive_wire_marker_reaches_receiver_without_debug_content() {
    let root = tempfile::tempdir().unwrap();
    let a = Identity::create(&root.path().join("a")).unwrap();
    let b = Identity::create(&root.path().join("b")).unwrap();
    let listener = Listener::bind("127.0.0.1:0".parse().unwrap(), &b, &a.peer()).unwrap();
    let peer = b.peer();
    let (sent, received) = tokio::join!(
        kk::send_with_policy(
            listener.local_addr().unwrap(),
            &a,
            &peer,
            "synthetic-private-fixture",
            true,
            config::DEFAULT_TIMEOUT
        ),
        listener.receive(config::DEFAULT_TIMEOUT)
    );
    sent.unwrap();
    let received = received.unwrap();
    assert!(received.sensitive);
    assert_eq!(received.text(), "synthetic-private-fixture");
    assert!(!format!("{received:?}").contains("synthetic-private-fixture"));
}

#[test]
fn argv_secret_is_rejected_and_removed_message_flag_is_not_an_alias() {
    let output = Command::new(env!("CARGO_BIN_EXE_kk"))
        .args([
            "send",
            "127.0.0.1:1",
            "synthetic-inline-fixture",
            "-s",
            "--identity",
            "missing",
            "--peer",
            "missing",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-inline-fixture"));
    let old = Command::new(env!("CARGO_BIN_EXE_kk"))
        .args([
            "send",
            "127.0.0.1:1",
            "--message",
            "fixture",
            "--identity",
            "missing",
            "--peer",
            "missing",
        ])
        .output()
        .unwrap();
    assert_eq!(old.status.code(), Some(2));
}

#[test]
fn waiting_for_sensitive_stdin_is_cancellable_without_a_hung_worker() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kk"))
        .args([
            "send",
            "127.0.0.1:1",
            "-",
            "-s",
            "--identity",
            "missing",
            "--peer",
            "missing",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let held = child.stdin.take().unwrap();
    let mut diagnostics = BufReader::new(child.stderr.take().unwrap());
    let mut line = String::new();
    diagnostics.read_line(&mut line).unwrap();
    assert!(line.contains("INPUT sensitive stdin"));
    assert!(
        Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let result = child.wait_with_output().unwrap();
    drop(held);
    assert_eq!(result.status.code(), Some(130));
    assert!(result.stdout.is_empty());
}

#[test]
fn stdin_is_bounded_before_any_identity_or_network_operation() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kk"))
        .args([
            "send",
            "127.0.0.1:1",
            "-",
            "-s",
            "--identity",
            "missing",
            "--peer",
            "missing",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&vec![b'x'; config::MAX_MESSAGE_BYTES + 1])
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("MESSAGE_TOO_LARGE"));
}
