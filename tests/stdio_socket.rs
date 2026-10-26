#![cfg(unix)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::Shutdown,
    os::{fd::OwnedFd, unix::net::UnixStream},
    process::{Command, Stdio},
    time::Duration,
};

#[test]
fn socket_stdio_preserves_flags_and_cancels_blocked_input_and_output() {
    for mode in ["flow", "blocked"] {
        let (mut input, child_input) = UnixStream::pair().unwrap();
        let (mut output, child_output) = UnixStream::pair().unwrap();
        let (diagnostics, child_diagnostics) = UnixStream::pair().unwrap();
        output
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        diagnostics
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "socket_stdio_child", "--nocapture"])
            .env("KK_TEST_SOCKET_STDIO", mode)
            .stdin(Stdio::from(OwnedFd::from(child_input)))
            .stdout(Stdio::from(OwnedFd::from(child_output)))
            .stderr(Stdio::from(OwnedFd::from(child_diagnostics)))
            .spawn()
            .unwrap();
        if mode == "flow" {
            input.write_all(b"socket input fixture").unwrap();
            input.shutdown(Shutdown::Write).unwrap();
        }
        let mut report = String::new();
        let reported = BufReader::new(diagnostics).read_line(&mut report);
        if reported.is_err() || report.trim() != "fixture finished" {
            let _ = child.kill();
            let status = child.wait().unwrap();
            panic!("socket {mode} fixture failed: {reported:?}, {report:?}, {status}");
        }
        let mut bytes = Vec::new();
        if let Err(error) = output.read_to_end(&mut bytes) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("socket {mode} output did not close: {error}");
        }
        assert!(child.wait().unwrap().success());
        if mode == "flow" {
            assert!(
                bytes
                    .windows(b"socket output fixture\n".len())
                    .any(|part| part == b"socket output fixture\n")
            );
        }
    }
}

#[tokio::test]
async fn socket_stdio_child() {
    let Ok(mode) = std::env::var("KK_TEST_SOCKET_STDIO") else {
        return;
    };
    let input_flags = rustix::fs::fcntl_getfl(std::io::stdin()).unwrap();
    let output_flags = rustix::fs::fcntl_getfl(std::io::stdout()).unwrap();
    if mode == "flow" {
        assert_eq!(
            kk::input::text(None, false).await.unwrap().as_str(),
            "socket input fixture"
        );
        kk::output::line("socket output fixture".to_owned())
            .await
            .unwrap();
    } else {
        assert!(
            tokio::time::timeout(Duration::from_millis(100), kk::input::text(None, false))
                .await
                .is_err()
        );
        let writes = async {
            loop {
                kk::output::line("x".repeat(65536)).await.unwrap();
            }
        };
        assert!(
            tokio::time::timeout(Duration::from_millis(100), writes)
                .await
                .is_err()
        );
    }
    assert_eq!(
        rustix::fs::fcntl_getfl(std::io::stdin()).unwrap(),
        input_flags
    );
    assert_eq!(
        rustix::fs::fcntl_getfl(std::io::stdout()).unwrap(),
        output_flags
    );
    writeln!(std::io::stderr(), "fixture finished").unwrap();
}
