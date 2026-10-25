#![cfg(target_os = "linux")]

use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};

#[test]
fn two_cli_processes_pair_without_manual_certificate_files_then_enforce_revocation() {
    let root = tempfile::tempdir().unwrap();
    let pa = root.path().join("a");
    let pb = root.path().join("b");
    let invite = pa.join("invite.json");
    let reserved = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let data_port = reserved.local_addr().unwrap().port();
    let mut server = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&pa)
        .args([
            "pair",
            "--listen",
            "127.0.0.1:0",
            "--as",
            "bob",
            "--allow",
            "text,files",
            "--confirm",
            "--invite-out",
        ])
        .arg(&invite)
        .args(["--data-port", &data_port.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut diagnostics = BufReader::new(server.stderr.take().unwrap());
    let mut ready = String::new();
    diagnostics.read_line(&mut ready).unwrap();
    assert!(
        ready.starts_with("PAIRING "),
        "server did not issue an invitation"
    );
    let joined = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&pb)
        .args([
            "pair",
            "--as",
            "alice",
            "--allow",
            "text,files",
            "--confirm",
            "--invitation",
        ])
        .arg(&invite)
        .output()
        .unwrap();
    let accepted = server.wait_with_output().unwrap();
    assert!(
        joined.status.success(),
        "{}",
        String::from_utf8_lossy(&joined.stderr)
    );
    assert!(accepted.status.success());
    drop(reserved);
    let address = format!("127.0.0.1:{data_port}");
    let mut receiver = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&pa)
        .args([
            "receive",
            "--from",
            "bob",
            "--bind",
            &address,
            "--timeout",
            "3",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut receive_diagnostics = BufReader::new(receiver.stderr.take().unwrap());
    ready.clear();
    receive_diagnostics.read_line(&mut ready).unwrap();
    assert!(ready.starts_with("LISTENING "));
    let sent = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&pb)
        .args(["send", "alice", "paired fixture", "--timeout", "3"])
        .output()
        .unwrap();
    assert!(
        sent.status.success(),
        "{}",
        String::from_utf8_lossy(&sent.stderr)
    );
    assert_eq!(
        receiver.wait_with_output().unwrap().stdout,
        b"paired fixture\n"
    );
    let revoked = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&pb)
        .args(["trust", "revoke", "alice"])
        .output()
        .unwrap();
    assert!(revoked.status.success());
    let denied = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&pb)
        .args(["send", "alice", "must not be sent"])
        .output()
        .unwrap();
    assert_eq!(denied.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&denied.stderr).contains("REVOKED"));
    assert!(denied.stdout.is_empty());
}
