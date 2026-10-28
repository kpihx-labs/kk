#![cfg(unix)]

use std::{
    io::{Read, Write},
    process::Command,
    time::Duration,
};

// Python's standard library allocates ONLY owned synthetic PTYs. Terminal
// acquisition/inspection in the SDK child uses safe rustix APIs. No Cargo
// feature change (rustix's separate `pty` feature is not currently enabled).
#[test]
fn owned_terminal_full_stderr_cancellation_restores_attributes_and_exits() {
    let result = Command::new("uv")
        .args([
            "run",
            "--offline",
            "--no-project",
            "python",
            "-c",
            r#"
import os, pty, signal, subprocess, sys, termios, time

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

for mode in ('prompt', 'read', 'newline'):
    master, slave = pty.openpty()
    original = termios.tcgetattr(slave)
    read, write = os.pipe()
    capacity = fill(write)
    if mode != 'prompt':
        remaining = capacity
        while remaining:
            remaining -= len(os.read(read, remaining))
    child = subprocess.Popen([sys.argv[1], '--exact', 'terminal_child', '--nocapture'],
        stdin=slave, stdout=subprocess.PIPE, stderr=write,
        env=dict(os.environ, KK_TEST_TERMINAL=mode))
    try:
        signal.alarm(10)
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
                remaining = capacity
                while remaining:
                    remaining -= os.write(write, b'x' * min(4096, remaining))
                os.write(master, b'owned fixture\n')
                deadline = time.monotonic() + 1
                while termios.tcgetattr(slave) != original:
                    assert child.poll() is None, 'child exited before newline cancellation'
                    assert time.monotonic() < deadline, 'echo held during blocked final newline'
                    time.sleep(.005)
        # A Darwin session leader's exit revokes the controlling PTY, including
        # our still-open slave fd. Inspect restoration AFTER runtime shutdown
        # but BEFORE allowing the session leader to exit, not on a revoked fd.
        output = b''
        while not output.endswith(b'runtime joined\n'):
            line = child.stdout.readline(1024)
            assert line, (mode, 'child exited before runtime joined', output)
            output += line
            assert len(output) <= 4096, (mode, 'unexpected fixture output', output)
        assert termios.tcgetattr(slave) == original, mode + ': attributes changed'
        os.write(master, b'exit\n')
        remaining, _ = child.communicate(timeout=10)
        output += remaining
        assert child.returncode == 0, (mode, child.returncode, output)
        assert b'runtime joined' in output, (mode, output)
    finally:
        signal.alarm(0)
        if child.poll() is None:
            child.kill()
            child.wait()
        for fd in (master, slave, read, write):
            os.close(fd)
print('owned PTY: prompt/read/newline cancellation, attributes and runtime exit passed')
"#,
        ])
        .arg(std::env::current_exe().unwrap())
        .output()
        .expect("owned PTY regression requires uv with an offline Python interpreter");
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    print!("{}", String::from_utf8_lossy(&result.stdout));
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
        assert!(
            tokio::time::timeout(Duration::from_secs(2), kk::input::text(None, true))
                .await
                .is_err(),
            "{mode}: expected cancellation"
        );
    });
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
