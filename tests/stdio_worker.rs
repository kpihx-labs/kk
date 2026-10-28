#![cfg(unix)]

#[test]
fn vendored_spawn_rejects_positive_posix_errors() {
    use nix::{
        errno::Errno,
        spawn::{PosixSpawnAttr, PosixSpawnFileActions},
    };
    let attributes = PosixSpawnAttr::init().unwrap().reinit().unwrap();
    assert!(attributes.flags().unwrap().is_empty());
    #[cfg(target_os = "linux")]
    {
        let mut attributes = attributes;
        assert_eq!(
            attributes.set_flags(nix::spawn::PosixSpawnFlags::from_bits_retain(0x8000)),
            Err(Errno::EINVAL)
        );
        assert!(attributes.flags().unwrap().is_empty());
    }
    let mut actions = PosixSpawnFileActions::init().unwrap().reinit().unwrap();
    assert_eq!(actions.add_dup2(-1, 0), Err(Errno::EBADF));
    assert_eq!(actions.add_dup2(0, -1), Err(Errno::EBADF));
    assert_eq!(actions.add_close(-1), Err(Errno::EBADF));
}

#[cfg(not(target_vendor = "apple"))]
#[test]
fn worker_excludes_inheritable_sdk_pipe() {
    use kk::{
        Error,
        stdio_worker::{Worker, configure, configured, dispatch},
    };
    let (read, write) = rustix::pipe::pipe().unwrap();
    assert!(
        !rustix::io::fcntl_getfd(&write)
            .unwrap()
            .contains(rustix::io::FdFlags::CLOEXEC)
    );
    let path = std::path::PathBuf::from(env!("CARGO_BIN_EXE_kk"));
    assert!(
        matches!(Worker::new(path.clone()), Err(Error::Configuration(message)) if message.contains("Darwin-only"))
    );
    assert!(
        matches!(configure(path), Err(Error::Configuration(message)) if message.contains("Darwin-only"))
    );
    assert!(
        matches!(configured(), Err(Error::Configuration(message)) if message.contains("Darwin-only"))
    );
    assert!(dispatch().is_none());
    drop(write);
    let flags = rustix::fs::fcntl_getfl(&read).unwrap();
    rustix::fs::fcntl_setfl(&read, flags | rustix::fs::OFlags::NONBLOCK).unwrap();
    assert_eq!(
        rustix::io::read(&read, &mut [0]),
        Ok(0),
        "refused spawning must not leak an SDK pipe writer"
    );
}

#[cfg(target_vendor = "apple")]
mod darwin {

    use std::{
        io::{Read, Write},
        net::Shutdown,
        os::{fd::OwnedFd, unix::net::UnixStream},
        path::PathBuf,
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };
    use zeroize::Zeroizing;

    struct Reap(Child);

    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn wait(child: &mut Child) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "child {} exceeded deadline",
                child.id()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    // Only process IDs, parent IDs and state, never command arguments or environment.
    fn processes() -> Vec<(u32, u32, String)> {
        let output = Command::new("ps")
            .args(["-axo", "pid=,ppid=,stat="])
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| {
                let mut fields = line.split_whitespace();
                (
                    fields.next().unwrap().parse().unwrap(),
                    fields.next().unwrap().parse().unwrap(),
                    fields.next().unwrap().to_owned(),
                )
            })
            .collect()
    }

    fn worker_pid(parent: u32) -> u32 {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some((pid, _, _)) = processes().into_iter().find(|(_, ppid, _)| *ppid == parent)
            {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "worker of {parent} never appeared"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn fixture(mode: &str) -> Reap {
        Reap(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "darwin::worker_sdk_child", "--nocapture"])
                .env("KK_TEST_WORKER_STDIO", mode)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        )
    }

    fn direct(data: Stdio) -> (UnixStream, Reap) {
        let (mut parent, control) = UnixStream::pair().unwrap();
        rustix::net::sockopt::set_socket_send_buffer_size(&control, 1024).unwrap();
        assert!(
            rustix::net::sockopt::socket_send_buffer_size(&control).unwrap()
                < kk::config::MAX_MESSAGE_BYTES
        );
        parent
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        parent
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let child = Reap(
            Command::new(env!("CARGO_BIN_EXE_kk"))
                .arg("--kk-internal-stdio-worker-v1")
                .stdin(Stdio::from(OwnedFd::from(control.try_clone().unwrap())))
                .stdout(Stdio::from(OwnedFd::from(control)))
                .stderr(data)
                .spawn()
                .unwrap(),
        );
        let mut magic = [0; 11];
        parent.read_exact(&mut magic).unwrap();
        assert_eq!(&magic, b"KK-STDIO-1\n");
        (parent, child)
    }

    fn request(control: &mut UnixStream, operation: u8, size: u32, bytes: &[u8]) {
        control.write_all(&[operation]).unwrap();
        control.write_all(&size.to_be_bytes()).unwrap();
        control.write_all(bytes).unwrap();
        let mut started = [0];
        control.read_exact(&mut started).unwrap();
        assert_eq!(started, [127]);
    }

    fn ready(child: &mut Child) {
        let diagnostics = child.stderr.as_mut().unwrap();
        let flags = rustix::fs::fcntl_getfl(&*diagnostics).unwrap();
        rustix::fs::fcntl_setfl(&*diagnostics, flags | rustix::fs::OFlags::NONBLOCK).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"worker ready\n") {
            let mut chunk = [0; 128];
            match diagnostics.read(&mut chunk) {
                Ok(0) => panic!("fixture exited before worker setup"),
                Ok(count) => bytes.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("fixture diagnostics: {error}"),
            }
            assert!(Instant::now() < deadline, "worker setup deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn assert_fixture(child: &mut Child) {
        let status = wait(child);
        let mut diagnostics = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut diagnostics)
            .unwrap();
        assert!(status.success(), "{status}: {diagnostics}");
        assert!(diagnostics.contains("fixture finished"), "{diagnostics}");
    }

    #[test]
    fn explicit_sdk_path_and_unconfigured_sdk() {
        for mode in ["explicit", "eof", "unconfigured", "local-limit"] {
            let mut child = fixture(mode);
            if mode == "explicit" {
                child
                    .0
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(b"worker fixture")
                    .unwrap();
            } else if mode == "eof" {
                drop(child.0.stdin.take());
            }
            assert_fixture(&mut child.0);
            if mode == "explicit" {
                let mut bytes = String::new();
                child
                    .0
                    .stdout
                    .take()
                    .unwrap()
                    .read_to_string(&mut bytes)
                    .unwrap();
                assert!(bytes.contains("worker output\n"));
            }
        }
    }

    #[test]
    fn cancellation_reaps_blocked_read_and_write_workers() {
        for mode in ["cancel-input", "cancel-output"] {
            let mut child = fixture(mode);
            let worker = worker_pid(child.0.id());
            assert_fixture(&mut child.0);
            assert!(
                !processes().iter().any(|(pid, _, _)| *pid == worker),
                "cancelled worker {worker} was not reaped"
            );
        }
    }

    #[test]
    fn abrupt_parent_death_terminates_blocked_workers() {
        for mode in [
            "parent-death-input",
            "parent-death-output",
            "production-death-input",
            "production-death-output",
        ] {
            let mut child = fixture(mode);
            ready(&mut child.0);
            let worker = worker_pid(child.0.id());
            // The acknowledgement proves setup/supervision, not merely exec. Allow
            // scheduling into the read/write syscall before killing the parent.
            std::thread::sleep(Duration::from_millis(100));
            child.0.kill().unwrap();
            assert!(!wait(&mut child.0).success());
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let state = processes().into_iter().find(|(pid, _, _)| *pid == worker);
                // Orphans are reaped by the OS's subreaper/init, not by the dead
                // parent. A zombie is terminated, not a surviving IO worker.
                if state.is_none_or(|(_, _, state)| state.starts_with('Z')) {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "orphan worker {worker} survived control EOF"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    #[test]
    fn worker_rejects_malformed_limits_before_data_io() {
        for (operation, length) in [(0, 0), (1, u32::MAX), (1, 0), (2, u32::MAX)] {
            let (mut parent, control) = UnixStream::pair().unwrap();
            parent
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            parent
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut child = Reap(
                Command::new(env!("CARGO_BIN_EXE_kk"))
                    .arg("--kk-internal-stdio-worker-v1")
                    .stdin(Stdio::from(OwnedFd::from(control.try_clone().unwrap())))
                    .stdout(Stdio::from(OwnedFd::from(control)))
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let mut magic = [0; 11];
            parent.read_exact(&mut magic).unwrap();
            assert_eq!(&magic, b"KK-STDIO-1\n");
            parent.write_all(&[operation]).unwrap();
            parent.write_all(&length.to_be_bytes()).unwrap();
            // Keep control alive: exit must result from validation, not parent EOF.
            assert!(!wait(&mut child.0).success());
        }
    }

    #[test]
    fn worker_bounds_real_input_and_reports_broken_pipe() {
        // A regular file is an owned synthetic fixture, not operator data. This
        // isolates the worker's input-bound protocol from the pipe transport tests.
        let mut data = tempfile::tempfile().unwrap();
        data.write_all(&vec![b'x'; kk::config::MAX_MESSAGE_BYTES + 1])
            .unwrap();
        std::io::Seek::rewind(&mut data).unwrap();
        let (mut control, mut child) = direct(Stdio::from(data));
        request(&mut control, 1, kk::config::MAX_MESSAGE_BYTES as u32, &[]);
        let mut header = [0; 5];
        control.read_exact(&mut header).unwrap();
        assert_eq!(header, [1, 0, 0, 0, 0]);
        assert!(wait(&mut child.0).success());

        let (read, write) = rustix::pipe::pipe().unwrap();
        drop(read);
        let (mut control, mut child) = direct(Stdio::from(write));
        request(&mut control, 2, 1, b"x");
        control.read_exact(&mut header).unwrap();
        assert_eq!(header[0], 2);
        let errno = u32::from_be_bytes(header[1..].try_into().unwrap());
        assert_eq!(
            std::io::Error::from_raw_os_error(errno as i32).kind(),
            std::io::ErrorKind::BrokenPipe
        );
        assert!(wait(&mut child.0).success());
    }

    #[test]
    fn control_eof_terminates_worker_even_with_blocked_response() {
        let mut data = tempfile::tempfile().unwrap();
        data.write_all(&vec![b'x'; kk::config::MAX_MESSAGE_BYTES])
            .unwrap();
        std::io::Seek::rewind(&mut data).unwrap();
        let (mut control, mut child) = direct(Stdio::from(data));
        request(&mut control, 1, kk::config::MAX_MESSAGE_BYTES as u32, &[]);
        // Leave the receive side open without draining a response larger than the
        // helper's send buffer. Half-close ONLY the request side: data/result IO
        // cannot finish or obtain EPIPE just because of this EOF.
        std::thread::sleep(Duration::from_millis(100));
        control.shutdown(Shutdown::Write).unwrap();
        assert_eq!(wait(&mut child.0).code(), Some(125));
    }

    #[test]
    fn worker_excludes_inheritable_sdk_pipe() {
        let mut child = fixture("inherited-pipe");
        assert_fixture(&mut child.0);
    }

    #[test]
    fn full_stderr_notice_remains_cancellable() {
        let (read, write) = rustix::pipe::pipe().unwrap();
        let flags = rustix::fs::fcntl_getfl(&write).unwrap();
        rustix::fs::fcntl_setfl(&write, flags | rustix::fs::OFlags::NONBLOCK).unwrap();
        loop {
            match rustix::io::write(&write, &[b'x'; 4096]) {
                Ok(_) => {}
                Err(rustix::io::Errno::AGAIN) => break,
                Err(error) => panic!("fill owned fixture pipe: {error}"),
            }
        }
        // Restore BEFORE passing the owned writer as borrowed child stderr.
        rustix::fs::fcntl_setfl(&write, flags).unwrap();
        let mut child = Reap(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "darwin::worker_sdk_child", "--nocapture"])
                .env("KK_TEST_WORKER_STDIO", "full-stderr")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::from(write))
                .spawn()
                .unwrap(),
        );
        assert!(
            wait(&mut child.0).success(),
            "full stderr prevented cancellation/runtime shutdown"
        );
        drop(read);
    }

    #[test]
    fn worker_sdk_child() {
        let Ok(mode) = std::env::var("KK_TEST_WORKER_STDIO") else {
            return;
        };
        let input_flags = rustix::fs::fcntl_getfl(std::io::stdin()).unwrap();
        let output_flags = rustix::fs::fcntl_getfl(std::io::stdout()).unwrap();
        let stderr_flags = rustix::fs::fcntl_getfl(std::io::stderr()).unwrap();
        if mode.starts_with("parent-death-") {
            let descriptor = if mode.ends_with("input") {
                rustix::io::fcntl_dupfd_cloexec(std::io::stdin(), 0).unwrap()
            } else {
                rustix::io::fcntl_dupfd_cloexec(std::io::stdout(), 0).unwrap()
            };
            let (mut control, mut worker) = direct(Stdio::from(descriptor));
            if mode.ends_with("input") {
                request(&mut control, 1, kk::config::MAX_MESSAGE_BYTES as u32, &[]);
            } else {
                let bytes = vec![b'x'; kk::config::MAX_MESSAGE_BYTES * 6 + 8193];
                request(&mut control, 2, bytes.len() as u32, &bytes);
            }
            writeln!(std::io::stderr(), "worker ready").unwrap();
            // The outer test abruptly kills this process while the supervised
            // worker waits on IO. Its Drop guard therefore cannot help the orphan.
            let _ = worker.0.wait();
            panic!("worker unexpectedly completed");
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
        if mode == "unconfigured" {
            assert!(matches!(kk::stdio_worker::configured(), Err(kk::Error::Configuration(message)) if message.contains("explicitly configured")));
            #[cfg(target_vendor = "apple")]
            {
                assert!(matches!(kk::input::text(None, false).await, Err(kk::Error::Configuration(message)) if message.contains("explicitly configured")));
                assert!(matches!(kk::output::line("x".into()).await, Err(kk::Error::Configuration(message)) if message.contains("explicitly configured")));
            }
            return;
        }
        kk::stdio_worker::configure(PathBuf::from(env!("CARGO_BIN_EXE_kk"))).unwrap();
        let worker = kk::stdio_worker::configured().unwrap();
        if mode.starts_with("production-death-") {
            let operation = async {
                if mode.ends_with("input") {
                    worker.read().await.map(|_| ())
                } else {
                    worker.write(Zeroizing::new(vec![b'x'; kk::config::MAX_MESSAGE_BYTES * 6 + 8193])).await
                }
            };
            tokio::pin!(operation);
            tokio::select! {
                result = &mut operation => panic!("production worker unexpectedly completed: {result:?}"),
                () = tokio::time::sleep(Duration::from_millis(250)) => {},
            }
            writeln!(std::io::stderr(), "worker ready").unwrap();
            // Keep the production Worker future (and its private control socket)
            // alive until the outer fixture kills this SDK host without Drop.
            let result = operation.await;
            panic!("production worker unexpectedly completed: {result:?}");
        }
        match mode.as_str() {
            "explicit" => {
                assert_eq!(worker.read().await.unwrap().as_slice(), b"worker fixture");
                worker.write(Zeroizing::new(b"worker output\n".to_vec())).await.unwrap();
            }
            "eof" => assert!(worker.read().await.unwrap().is_empty()),
            "inherited-pipe" => {
                let (read, write) = rustix::pipe::pipe().unwrap();
                assert!(!rustix::io::fcntl_getfd(&write).unwrap().contains(rustix::io::FdFlags::CLOEXEC));
                let flags = rustix::fs::fcntl_getfl(&read).unwrap();
                rustix::fs::fcntl_setfl(&read, flags | rustix::fs::OFlags::NONBLOCK).unwrap();
                let mut pending = Box::pin(worker.read());
                tokio::select! {
                    result = &mut pending => panic!("input worker unexpectedly completed: {result:?}"),
                    () = tokio::time::sleep(Duration::from_millis(100)) => {},
                }
                drop(write);
                let result = rustix::io::read(&read, &mut [0]);
                drop(pending);
                assert_eq!(result, Ok(0), "worker inherited unrelated SDK pipe writer");
            }
            "full-stderr" => {
                #[cfg(target_vendor = "apple")]
                let notice = kk::input::text(None, true);
                #[cfg(not(target_vendor = "apple"))]
                let notice = worker.write_stderr(Zeroizing::new(b"INPUT sensitive stdin\n".to_vec()));
                assert!(tokio::time::timeout(Duration::from_millis(100), notice).await.is_err());
            }
            "local-limit" => {
                assert!(matches!(worker.write(Zeroizing::new(vec![0; kk::config::MAX_MESSAGE_BYTES * 6 + 8194])).await, Err(kk::Error::Configuration(_))));
            }
            "cancel-input" => {
                assert!(tokio::time::timeout(Duration::from_secs(2), worker.read()).await.is_err());
            }
            "cancel-output" => {
                assert!(tokio::time::timeout(Duration::from_secs(2), worker.write(Zeroizing::new(vec![b'x'; kk::config::MAX_MESSAGE_BYTES * 6 + 8193]))).await.is_err());
            }
            _ => panic!("unknown fixture mode"),
        }
    });
        drop(runtime);
        assert_eq!(
            rustix::fs::fcntl_getfl(std::io::stdin()).unwrap(),
            input_flags
        );
        assert_eq!(
            rustix::fs::fcntl_getfl(std::io::stdout()).unwrap(),
            output_flags
        );
        assert_eq!(
            rustix::fs::fcntl_getfl(std::io::stderr()).unwrap(),
            stderr_flags
        );
        if mode == "full-stderr" {
            writeln!(std::io::stdout(), "fixture finished").unwrap();
        } else {
            writeln!(std::io::stderr(), "fixture finished").unwrap();
        }
        std::process::exit(0);
    }
}
