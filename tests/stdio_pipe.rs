#![cfg(unix)]

use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

// The child deliberately waits for runtime shutdown before exiting. A cancelled
// future alone is not proof: spawn_blocking workers must actually return too.
struct Reap(Child);

impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn pipe_stdio_flow_limits_eof_and_cancellation() {
    for mode in [
        "flow",
        "hidden-flow",
        "eof",
        "input-limit",
        "invalid-utf8",
        "output-limit",
        "input-wait",
        "hidden-input-wait",
        "output-backpressure",
        "unreopenable-output",
        "unreopenable-backpressure",
    ] {
        let mut child = Reap(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "pipe_stdio_child", "--nocapture"])
                .env("KK_TEST_PIPE_STDIO", mode)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut input = child.0.stdin.take().unwrap();
        // These are parent-owned pipe ends, never the child's borrowed stdio.
        let flags = rustix::fs::fcntl_getfl(&input).unwrap();
        rustix::fs::fcntl_setfl(&input, flags | rustix::fs::OFlags::NONBLOCK).unwrap();
        let bytes = match mode {
            "flow" | "hidden-flow" => b"pipe input fixture".to_vec(),
            "input-limit" => vec![b'x'; kk::config::MAX_MESSAGE_BYTES + 1],
            "invalid-utf8" => vec![0xff],
            _ => Vec::new(),
        };
        let mut position = 0;
        while position < bytes.len() {
            assert!(Instant::now() < deadline, "pipe {mode} input stalled");
            match input.write(&bytes[position..]) {
                Ok(0) => panic!("pipe {mode} input made no progress"),
                Ok(count) => position += count,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("pipe {mode} input failed: {error}"),
            }
        }
        if !mode.ends_with("input-wait") {
            drop(input);
        }
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.0.kill().unwrap();
                let status = child.0.wait().unwrap();
                let mut diagnostics = String::new();
                child
                    .0
                    .stderr
                    .take()
                    .unwrap()
                    .read_to_string(&mut diagnostics)
                    .unwrap();
                panic!("pipe {mode} exceeded parent deadline: {status}; {diagnostics}");
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let mut diagnostics = String::new();
        child
            .0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut diagnostics)
            .unwrap();
        assert!(status.success(), "pipe {mode}: {status}; {diagnostics}");
        assert!(
            diagnostics.contains("fixture finished"),
            "pipe {mode}: {diagnostics}"
        );
        if matches!(mode, "flow" | "hidden-flow" | "unreopenable-output") {
            let mut output = String::new();
            child
                .0
                .stdout
                .take()
                .unwrap()
                .read_to_string(&mut output)
                .unwrap();
            assert!(output.contains("pipe output fixture\n"));
        }
    }
}

#[test]
fn pipe_stdio_child() {
    let Ok(mode) = std::env::var("KK_TEST_PIPE_STDIO") else {
        return;
    };
    let input_flags = rustix::fs::fcntl_getfl(std::io::stdin()).unwrap();
    let output_flags = rustix::fs::fcntl_getfl(std::io::stdout()).unwrap();
    // Only this fixture's newly created pipe is changed. The inherited write
    // capability remains valid, but Linux cannot reopen its pathname as a user.
    #[cfg(target_os = "linux")]
    let output_mode = if mode.starts_with("unreopenable-") {
        let old = rustix::fs::Mode::from_bits_truncate(
            rustix::fs::fstat(std::io::stdout()).unwrap().st_mode & 0o7777,
        );
        rustix::fs::fchmod(std::io::stdout(), rustix::fs::Mode::empty()).unwrap();
        Some(old)
    } else {
        None
    };
    #[cfg(target_vendor = "apple")]
    kk::stdio_worker::configure(std::path::PathBuf::from(env!("CARGO_BIN_EXE_kk"))).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        match mode.as_str() {
            "flow" | "hidden-flow" | "eof" => {
                let value = kk::input::text(None, mode == "hidden-flow").await.unwrap();
                assert_eq!(
                    value.as_str(),
                    if mode == "eof" {
                        ""
                    } else {
                        "pipe input fixture"
                    }
                );
                kk::output::line("pipe output fixture".to_owned())
                    .await
                    .unwrap();
            }
            "input-limit" => assert!(matches!(
                kk::input::text(None, false).await,
                Err(kk::Error::MessageTooLarge)
            )),
            "invalid-utf8" => assert!(matches!(
                kk::input::text(None, false).await,
                Err(kk::Error::Configuration(_))
            )),
            "output-limit" => assert!(matches!(
                kk::output::line("x".repeat(kk::config::MAX_MESSAGE_BYTES * 6 + 8193)).await,
                Err(kk::Error::Configuration(_))
            )),
            "input-wait" | "hidden-input-wait" => {
                assert!(
                    tokio::time::timeout(
                        Duration::from_millis(100),
                        kk::input::text(None, mode == "hidden-input-wait")
                    )
                    .await
                    .is_err()
                );
            }
            "unreopenable-output" => kk::output::line("pipe output fixture".to_owned())
                .await
                .unwrap(),
            "output-backpressure" | "unreopenable-backpressure" => {
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
            _ => panic!("unknown fixture mode"),
        }
    });
    // No shutdown_timeout: it would conceal a detached, blocked worker.
    drop(runtime);
    #[cfg(target_os = "linux")]
    if let Some(old) = output_mode {
        rustix::fs::fchmod(std::io::stdout(), old).unwrap();
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
    // Backpressure intentionally leaves stdout full; bypass libtest's final
    // stdout report only AFTER the runtime and its workers have joined.
    std::process::exit(0);
}
