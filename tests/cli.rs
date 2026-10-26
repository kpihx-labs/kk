use std::process::Command;

#[test]
fn file_receive_requires_explicit_consent_before_loading_identities() {
    let output = Command::new(env!("CARGO_BIN_EXE_kk"))
        .args([
            "receive",
            "-f",
            "--bind",
            "127.0.0.1:0",
            "--identity",
            "missing",
            "--peer",
            "missing",
            "--output",
            "missing",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("--accept")
    );
    assert!(output.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn file_inspection_hashes_binary_content_without_an_identity() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    std::fs::write(&source, b"abc").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_kk"))
        .args(["inspect"])
        .arg(source)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"FILE 3 sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n"
    );
}

#[test]
fn version_matches_the_library() {
    let output = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--version")
        .output()
        .expect("run kk");
    assert!(output.status.success());
    assert_eq!(output.stdout, format!("kk {}\n", kk::VERSION).as_bytes());
    assert!(output.stderr.is_empty());
}

#[test]
fn help_describes_only_available_behavior() {
    for args in [vec![], vec!["--help"], vec!["-h"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_kk"))
            .args(args)
            .output()
            .expect("run kk");
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).expect("UTF-8 help");
        assert!(text.contains("Usage: kk"));
        assert!(text.contains("receive"));
        assert!(text.contains("identity"));
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn native_preview_requires_explicit_consent_and_rejects_pipeline_syntax_before_capture() {
    let missing = Command::new(env!("CARGO_BIN_EXE_kk"))
        .args(["media", "preview", "camera", "--device", "/dev/video0"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    let injected = Command::new(env!("CARGO_BIN_EXE_kk"))
        .args([
            "media",
            "preview",
            "microphone",
            "--device",
            "device ! filesink location=/tmp/forbidden",
            "--consent",
        ])
        .output()
        .unwrap();
    assert!(!injected.status.success());
    assert!(
        String::from_utf8(injected.stderr)
            .unwrap()
            .contains("CONFIGURATION")
    );
}

#[test]
fn unsupported_arguments_fail_without_claiming_success() {
    for args in [vec!["send"], vec!["identity", "unknown"], vec!["--unknown"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_kk"))
            .args(args)
            .output()
            .expect("run kk");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .expect("UTF-8 diagnostic")
                .contains("error:")
        );
    }
}

#[test]
fn invalid_deadlines_are_syntax_errors_not_network_operations() {
    for timeout in ["0", "86401", "not-a-number"] {
        let output = Command::new(env!("CARGO_BIN_EXE_kk"))
            .args([
                "receive",
                "--bind",
                "127.0.0.1:0",
                "--identity",
                "missing",
                "--peer",
                "missing",
                "--timeout",
                timeout,
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn identity_cli_creates_once_and_public_show_never_needs_the_key() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("identity");
    let created = Command::new(env!("CARGO_BIN_EXE_kk"))
        .args(["identity", "create"])
        .arg(&directory)
        .output()
        .unwrap();
    assert!(created.status.success());
    assert!(created.stdout.starts_with(b"sha256:"));
    let repeated = Command::new(env!("CARGO_BIN_EXE_kk"))
        .args(["identity", "create"])
        .arg(&directory)
        .output()
        .unwrap();
    assert_eq!(repeated.status.code(), Some(1));
    assert!(repeated.stdout.is_empty());
    std::fs::rename(
        directory.join("private-key.der"),
        directory.join("unused-key.der"),
    )
    .unwrap();
    let shown = Command::new(env!("CARGO_BIN_EXE_kk"))
        .args(["identity", "show"])
        .arg(directory.join("certificate.der"))
        .output()
        .unwrap();
    assert!(shown.status.success());
    assert_eq!(shown.stdout, created.stdout);
}

#[cfg(unix)]
#[test]
fn two_cli_processes_exchange_an_authenticated_message() {
    use std::{
        io::{BufRead, BufReader},
        process::Stdio,
    };

    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    kk::Identity::create(&a).unwrap();
    kk::Identity::create(&b).unwrap();
    let mut receiver = Command::new(env!("CARGO_BIN_EXE_kk"))
        .args(["receive", "--bind", "127.0.0.1:0", "--identity"])
        .arg(&b)
        .arg("--peer")
        .arg(a.join("certificate.der"))
        .args(["--timeout", "3"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut readiness = String::new();
    let mut diagnostics = BufReader::new(receiver.stderr.take().unwrap());
    diagnostics.read_line(&mut readiness).unwrap();
    let address = readiness
        .trim()
        .strip_prefix("LISTENING ")
        .expect("listener readiness");
    let sent = Command::new(env!("CARGO_BIN_EXE_kk"))
        .args(["send", address, "--identity"])
        .arg(&a)
        .arg("--peer")
        .arg(b.join("certificate.der"))
        .args(["hello from another process", "--timeout", "3"])
        .output()
        .unwrap();
    let received = receiver.wait_with_output().unwrap();
    assert!(
        sent.status.success(),
        "{}",
        String::from_utf8_lossy(&sent.stderr)
    );
    assert!(received.status.success());
    assert_eq!(sent.stdout, b"ACK 26 bytes\n");
    assert_eq!(received.stdout, b"hello from another process\n");
}

#[cfg(unix)]
#[test]
fn ctrl_c_exits_130_without_a_received_message() {
    use std::{
        io::{BufRead, BufReader},
        process::Stdio,
    };

    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    kk::Identity::create(&a).unwrap();
    kk::Identity::create(&b).unwrap();
    let mut receiver = Command::new(env!("CARGO_BIN_EXE_kk"))
        .args(["receive", "--bind", "127.0.0.1:0", "--identity"])
        .arg(&b)
        .arg("--peer")
        .arg(a.join("certificate.der"))
        .args(["--timeout", "3"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut readiness = String::new();
    let mut diagnostics = BufReader::new(receiver.stderr.take().unwrap());
    diagnostics.read_line(&mut readiness).unwrap();
    assert!(readiness.starts_with("LISTENING "));
    assert!(
        Command::new("kill")
            .arg("-INT")
            .arg(receiver.id().to_string())
            .status()
            .unwrap()
            .success()
    );
    let output = receiver.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(130));
    assert!(output.stdout.is_empty());
}
