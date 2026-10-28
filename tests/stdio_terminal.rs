#![cfg(unix)]

use std::{
    io::{Read, Seek, Write},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

// Python's standard library allocates ONLY owned synthetic PTYs. Terminal
// acquisition/inspection in the SDK child uses safe rustix APIs. No Cargo
// feature change (rustix's separate `pty` feature is not currently enabled).
#[test]
fn owned_terminal_full_stderr_cancellation_restores_attributes_and_exits() {
    for mode in ["prompt", "read", "newline"] {
        let mut diagnostics = tempfile::tempfile().unwrap();
        let mut child = Command::new("uv")
            .args([
                "run",
                "--offline",
                "--no-project",
                "python",
                "-c",
                r#"
import os, pty, signal, subprocess, sys, termios, time

def phase(message):
    print(sys.argv[2] + ': ' + message, file=sys.stderr, flush=True)

def expired(signum, frame):
    raise TimeoutError('owned PTY fixture deadline')

signal.signal(signal.SIGALRM, expired)

def fill(fd):
    count = 0
    os.set_blocking(fd, False)
    try:
        while True:
            count += os.write(fd, b'x' * 4096)
    except BlockingIOError:
        try:
            while True:
                count += os.write(fd, b'x')
        except BlockingIOError:
            pass
    finally:
        os.set_blocking(fd, True)
    return count

for mode in (sys.argv[2],):
    phase('setup')
    signal.alarm(5)
    master, slave = pty.openpty()
    original = termios.tcgetattr(slave)
    read, write = os.pipe()
    capacity = fill(write)
    phase('pipe filled: ' + str(capacity))
    if mode != 'prompt':
        remaining = capacity
        while remaining:
            remaining -= len(os.read(read, remaining))
    child = subprocess.Popen([sys.argv[1], '--exact', 'terminal_child', '--nocapture'],
        stdin=slave, stdout=subprocess.PIPE, stderr=write,
        env=dict(os.environ, KK_TEST_TERMINAL=mode))
    phase('child pid=' + str(child.pid))
    try:
        phase('waiting for echo/join')
        if mode != 'prompt':
            deadline = time.monotonic() + 5
            while termios.tcgetattr(slave)[3] & termios.ECHO:
                assert child.poll() is None, 'child exited before disabling echo'
                assert time.monotonic() < deadline, 'echo was never disabled'
                time.sleep(.005)
            if mode == 'newline':
                # Prompt completed and echo is now disabled. Block the final
                # newline, then deliver a canonical line on the owned master.
                # Measure capacity before spawning, then refill exactly that
                # capacity. Never change the child's borrowed stderr flags.
                prompt = b'Sensitive input: '
                received = b''
                while len(received) < len(prompt):
                    received += os.read(read, len(prompt) - len(received))
                assert received == prompt, received
                phase('prompt received; refilling stderr')
                remaining = capacity
                while remaining:
                    remaining -= os.write(write, b'x' * min(4096, remaining))
                os.write(master, b'owned fixture\n')
                phase('line delivered; waiting for echo restoration')
                deadline = time.monotonic() + 1
                while termios.tcgetattr(slave) != original:
                    assert child.poll() is None, 'child exited before newline cancellation'
                    assert time.monotonic() < deadline, 'echo held during blocked final newline'
                    time.sleep(.005)
        # A Darwin session leader's exit revokes the controlling PTY, including
        # our still-open slave fd. Inspect restoration AFTER runtime shutdown
        # but BEFORE allowing the session leader to exit, not on a revoked fd.
        output = b''
        phase('waiting for runtime joined')
        while not output.endswith(b'runtime joined\n'):
            line = child.stdout.readline(1024)
            assert line, (mode, 'child exited before runtime joined', output)
            phase('child stdout: ' + repr(line))
            output += line
            assert len(output) <= 4096, (mode, 'unexpected fixture output', output)
        assert termios.tcgetattr(slave) == original, mode + ': attributes changed'
        phase('attributes restored; releasing stdin')
        os.write(master, b'exit\n')
        phase('stdin released; draining release echo')
        # Echo is restored above, so the release line produces terminal output.
        # Darwin's session-leader exit drains the controlling terminal (XNU
        # exit1 -> ttywait) before revoking it. A fixture that never reads its
        # master deadlocks that exit even though every SDK worker has joined.
        # Keep stderr full: only consume this separate, owned PTY's echo.
        echoed = b''
        while not echoed.endswith(b'exit\r\n') and not echoed.endswith(b'exit\n'):
            chunk = os.read(master, 1024)
            assert chunk, (mode, 'PTY closed before release echo', echoed)
            echoed += chunk
            assert len(echoed) <= 4096, (mode, 'unexpected terminal output', echoed)
        phase('release echo drained; waiting for exit')
        remaining, _ = child.communicate(timeout=5)
        output += remaining
        assert child.returncode == 0, (mode, child.returncode, output)
        assert b'runtime joined' in output, (mode, output)
    finally:
        signal.alarm(0)
        # Also unblock Darwin's terminal drain when an assertion/deadline fails
        # before the normal echo read. Never wait on a session leader while
        # retaining an unread master that can hold its exit in ttywait.
        os.close(master)
        if child.poll() is None:
            child.kill()
            child.wait(timeout=5)
        phase('child reaped')
        for fd in (slave, read, write):
            os.close(fd)
print('owned PTY ' + mode + ': cancellation, attributes and runtime exit passed')
"#,
            ])
            .arg(std::env::current_exe().unwrap())
            .arg(mode)
            .stdin(Stdio::null())
            // Files, not pipes: a leaked descendant cannot keep output() waiting
            // for EOF after the supervisor has exited.
            .stdout(diagnostics.try_clone().unwrap())
            .stderr(diagnostics.try_clone().unwrap())
            .process_group(0)
            .spawn()
            .expect("owned PTY regression requires uv with an offline Python interpreter");
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        diagnostics.rewind().unwrap();
        let mut output = String::new();
        diagnostics.read_to_string(&mut output).unwrap();
        if status.is_none() {
            // terminal_child owns a separate session/PTY, so kill that session too.
            if let Some(pid) = output
                .lines()
                .filter(|_| !output.contains(&format!("{mode}: child reaped")))
                .find_map(|line| {
                    line.strip_prefix(&format!("{mode}: child pid="))?
                        .parse::<i32>()
                        .ok()
                })
                .and_then(rustix::process::Pid::from_raw)
            {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
                let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
            }
            if let Some(group) = rustix::process::Pid::from_raw(child.id() as i32) {
                let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
            }
            let _ = child.kill();
            child.wait().unwrap();
        }
        assert!(
            status.is_some_and(|status| status.success()),
            "owned PTY {mode}: supervisor status {status:?} (10s deadline)\n{output}"
        );
        print!("{output}");
    }
}

#[test]
fn terminal_child() {
    let Ok(mode) = std::env::var("KK_TEST_TERMINAL") else {
        return;
    };
    rustix::process::setsid().unwrap();
    rustix::process::ioctl_tiocsctty(std::io::stdin()).unwrap();
    let original = rustix::termios::tcgetattr(std::io::stdin()).unwrap();
    let input_flags = rustix::fs::fcntl_getfl(std::io::stdin()).unwrap();
    let stderr_flags = rustix::fs::fcntl_getfl(std::io::stderr()).unwrap();
    #[cfg(target_vendor = "apple")]
    kk::stdio_worker::configure(std::path::PathBuf::from(env!("CARGO_BIN_EXE_kk"))).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        writeln!(std::io::stdout(), "input started").unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(2), kk::input::text(None, true))
                .await
                .is_err(),
            "{mode}: expected cancellation"
        );
    });
    writeln!(std::io::stdout(), "input cancelled; joining runtime").unwrap();
    // An ordinary drop must join ALL spawn_blocking work. A shutdown timeout
    // would hide precisely the stuck diagnostic/echo worker this tests.
    drop(runtime);
    assert_eq!(
        format!("{original:?}"),
        format!(
            "{:?}",
            rustix::termios::tcgetattr(std::io::stdin()).unwrap()
        )
    );
    assert_eq!(
        rustix::fs::fcntl_getfl(std::io::stdin()).unwrap(),
        input_flags
    );
    assert_eq!(
        rustix::fs::fcntl_getfl(std::io::stderr()).unwrap(),
        stderr_flags
    );
    writeln!(std::io::stdout(), "runtime joined").unwrap();
    std::io::stdout().flush().unwrap();
    // Keep the controlling session alive until the parent has inspected the
    // restored attributes. Darwin revokes the PTY when this session leader exits.
    let mut release = [0; 5];
    std::io::stdin().read_exact(&mut release).unwrap();
    assert_eq!(&release, b"exit\n");
    std::process::exit(0);
}
