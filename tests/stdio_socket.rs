#![cfg(unix)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::Shutdown,
    os::{fd::OwnedFd, unix::net::UnixStream},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Reap(Child);

impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn socket_stdio_preserves_flags_and_cancels_blocked_input_and_output() {
    for mode in [
        "flow",
        "hidden-flow",
        "eof",
        "blocked",
        "hidden-blocked",
        "input-limit",
        "invalid-utf8",
        "output-limit",
        "closed-output",
    ] {
        let (mut input, child_input) = UnixStream::pair().unwrap();
        let (mut output, child_output) = UnixStream::pair().unwrap();
        #[cfg(target_vendor = "apple")]
        rustix::net::sockopt::set_socket_nosigpipe(&child_output, false).unwrap();
        let (diagnostics, child_diagnostics) = UnixStream::pair().unwrap();
        output
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        diagnostics
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        input
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut child = Reap(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "socket_stdio_child", "--nocapture"])
                .env("KK_TEST_SOCKET_STDIO", mode)
                .stdin(Stdio::from(OwnedFd::from(child_input)))
                .stdout(Stdio::from(OwnedFd::from(child_output)))
                .stderr(Stdio::from(OwnedFd::from(child_diagnostics)))
                .spawn()
                .unwrap(),
        );
        if matches!(
            mode,
            "flow" | "hidden-flow" | "eof" | "input-limit" | "invalid-utf8"
        ) {
            let bytes = match mode {
                "input-limit" => vec![b'x'; kk::config::MAX_MESSAGE_BYTES + 1],
                "invalid-utf8" => vec![0xff],
                "eof" => Vec::new(),
                _ => b"socket input fixture".to_vec(),
            };
            input.write_all(&bytes).unwrap();
            input.shutdown(Shutdown::Write).unwrap();
        }
        let mut diagnostics = BufReader::new(diagnostics);
        if mode == "closed-output" {
            // Wait until libtest has finished its initial stdout reporting.
            let mut ready = String::new();
            diagnostics.read_line(&mut ready).unwrap();
            assert_eq!(ready.trim(), "close output now");
            output.shutdown(Shutdown::Read).unwrap();
            input.write_all(b"x").unwrap();
        }
        if mode.starts_with("hidden-") {
            let mut notice = String::new();
            assert!(diagnostics.read_line(&mut notice).is_ok());
            assert_eq!(notice.trim(), "INPUT sensitive stdin");
        }
        let mut report = String::new();
        let reported = diagnostics.read_line(&mut report);
        if reported.is_err() || report.trim() != "fixture finished" {
            let _ = child.0.kill();
            let status = child.0.wait().unwrap();
            panic!("socket {mode} fixture failed: {reported:?}, {report:?}, {status}");
        }
        let mut bytes = Vec::new();
        if let Err(error) = output.read_to_end(&mut bytes) {
            let _ = child.0.kill();
            let _ = child.0.wait();
            panic!("socket {mode} output did not close: {error}");
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success(), "socket {mode}: {status}");
                break;
            }
            assert!(Instant::now() < deadline, "socket {mode} did not exit");
            std::thread::sleep(Duration::from_millis(5));
        }
        if matches!(mode, "flow" | "hidden-flow") {
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
    #[cfg(target_vendor = "apple")]
    kk::stdio_worker::configure(std::path::PathBuf::from(env!("CARGO_BIN_EXE_kk"))).unwrap();
    #[cfg(target_vendor = "apple")]
    let output_nosigpipe = rustix::net::sockopt::socket_nosigpipe(std::io::stdout()).unwrap();
    if matches!(mode.as_str(), "flow" | "hidden-flow" | "eof") {
        assert_eq!(
            kk::input::text(None, mode == "hidden-flow")
                .await
                .unwrap()
                .as_str(),
            if mode == "eof" {
                ""
            } else {
                "socket input fixture"
            }
        );
        kk::output::line("socket output fixture".to_owned())
            .await
            .unwrap();
    } else if matches!(mode.as_str(), "blocked" | "hidden-blocked") {
        assert!(
            tokio::time::timeout(
                Duration::from_millis(100),
                kk::input::text(None, mode == "hidden-blocked")
            )
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
    } else if mode == "input-limit" {
        assert!(matches!(
            kk::input::text(None, false).await,
            Err(kk::Error::MessageTooLarge)
        ));
    } else if mode == "invalid-utf8" {
        assert!(matches!(
            kk::input::text(None, false).await,
            Err(kk::Error::Configuration(_))
        ));
    } else if mode == "output-limit" {
        assert!(matches!(
            kk::output::line("x".repeat(kk::config::MAX_MESSAGE_BYTES * 6 + 8193)).await,
            Err(kk::Error::Configuration(_))
        ));
    } else if mode == "closed-output" {
        writeln!(std::io::stderr(), "close output now").unwrap();
        std::io::stdin().read_exact(&mut [0]).unwrap();
        assert!(matches!(
            kk::output::line("closed socket fixture".to_owned()).await,
            Err(kk::Error::Io(error)) if error.kind() == std::io::ErrorKind::BrokenPipe
        ));
    } else {
        panic!("unknown fixture mode");
    }
    assert_eq!(
        rustix::fs::fcntl_getfl(std::io::stdin()).unwrap(),
        input_flags
    );
    assert_eq!(
        rustix::fs::fcntl_getfl(std::io::stdout()).unwrap(),
        output_flags
    );
    #[cfg(target_vendor = "apple")]
    assert_eq!(
        rustix::net::sockopt::socket_nosigpipe(std::io::stdout()).unwrap(),
        output_nosigpipe
    );
    writeln!(std::io::stderr(), "fixture finished").unwrap();
    if mode == "closed-output" {
        // Do not let libtest's final report write to the deliberately broken
        // stdout. The output worker has already returned and been joined.
        std::process::exit(0);
    }
}
