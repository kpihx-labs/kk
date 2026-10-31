//! Explicitly opted-in, one-operation stdio workers. See docs/stdio.md.

#[cfg(any(target_vendor = "apple", test))]
use crate::config;
use crate::{Error, Result};
#[cfg(any(target_vendor = "apple", test))]
use std::fs::File;
#[cfg(all(test, not(target_vendor = "apple")))]
use std::io::{Read, Write};
use std::{io, path::PathBuf, process::ExitCode};
#[cfg(target_vendor = "apple")]
use std::{sync::OnceLock, time::Duration};
#[cfg(target_vendor = "apple")]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeroize::Zeroizing;

#[path = "stdio_worker/protocol.rs"]
mod protocol;
#[cfg(any(target_vendor = "apple", test))]
use protocol::{MAGIC, STARTED};
use protocol::{OUTPUT_LIMIT, READ, WRITE};

#[cfg(target_vendor = "apple")]
const ARGUMENT: &str = "--kk-internal-stdio-worker-v1";
#[cfg(target_vendor = "apple")]
static EXECUTABLE: OnceLock<PathBuf> = OnceLock::new();
#[cfg(target_vendor = "apple")]
static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
#[cfg(not(target_vendor = "apple"))]
const DARWIN_ONLY: &str = "supervised stdio worker spawning is Darwin-only; use the existing native stdio APIs on this platform";

/// Trust a compatible executable explicitly. Never inferred from the SDK host.
/// An opted-in host must call `dispatch` before constructing its runtime.
/// See [`Worker`] for the host's exclusive wait/reap ownership precondition.
pub fn configure(executable: PathBuf) -> Result<()> {
    #[cfg(not(target_vendor = "apple"))]
    {
        let _ = executable;
        Err(Error::Configuration(DARWIN_ONLY))
    }
    #[cfg(target_vendor = "apple")]
    {
        let worker = Worker::new(executable)?;
        match EXECUTABLE.set(worker.executable.clone()) {
            Ok(()) => Ok(()),
            Err(path) if EXECUTABLE.get() == Some(&path) => Ok(()),
            Err(_) => Err(Error::Configuration("stdio worker is already configured")),
        }
    }
}

/// Obtain the explicitly registered worker, or a descriptive configuration error.
pub fn configured() -> Result<Worker> {
    #[cfg(not(target_vendor = "apple"))]
    {
        Err(Error::Configuration(DARWIN_ONLY))
    }
    #[cfg(target_vendor = "apple")]
    {
        let executable = EXECUTABLE.get().ok_or(Error::Configuration(
        "Darwin pipe stdio requires an explicitly configured compatible worker; see docs/stdio.md",
    ))?;
        Ok(Worker {
            executable: executable.clone(),
        })
    }
}

/// A trusted compatible K&K binary, or an explicitly participating SDK host.
/// No discovery, environment override, shell, retries or fallback execution.
/// The host must leave exclusive wait/reap ownership of workers to this API,
/// as with `std::process::Child`: no competing waitpid or automatic SIGCHLD reap.
/// Violating that precondition can permit PID reuse before cancellation signals.
pub struct Worker {
    #[cfg(target_vendor = "apple")]
    executable: PathBuf,
    #[cfg(not(target_vendor = "apple"))]
    _private: (),
}

impl Worker {
    pub fn new(executable: PathBuf) -> Result<Self> {
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = executable;
            Err(Error::Configuration(DARWIN_ONLY))
        }
        #[cfg(target_vendor = "apple")]
        {
            if !executable.is_absolute() || !executable.is_file() {
                return Err(Error::Configuration(
                    "stdio worker requires a trusted absolute executable path",
                ));
            }
            Ok(Self { executable })
        }
    }

    /// Read bounded raw input from this process's stdin, through a worker.
    pub async fn read(&self) -> Result<Zeroizing<Vec<u8>>> {
        self.exchange(READ, io::stdin(), Zeroizing::new(Vec::new()))
            .await
    }

    /// Write already-framed bytes to this process's stdout, through a worker.
    pub async fn write(&self, bytes: Zeroizing<Vec<u8>>) -> Result<()> {
        self.write_to(io::stdout(), bytes).await
    }

    /// Write bounded diagnostic bytes to stderr without blocking the host runtime.
    pub async fn write_stderr(&self, bytes: Zeroizing<Vec<u8>>) -> Result<()> {
        self.write_to(io::stderr(), bytes).await
    }

    async fn write_to(
        &self,
        data: impl std::os::fd::AsFd,
        bytes: Zeroizing<Vec<u8>>,
    ) -> Result<()> {
        if bytes.len() > OUTPUT_LIMIT {
            return Err(Error::Configuration("stdio worker output exceeds limit"));
        }
        self.exchange(WRITE, data, bytes).await.map(|_| ())
    }

    async fn exchange(
        &self,
        operation: u8,
        data: impl std::os::fd::AsFd,
        bytes: Zeroizing<Vec<u8>>,
    ) -> Result<Zeroizing<Vec<u8>>> {
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = (operation, data, bytes);
            Err(Error::Configuration(DARWIN_ONLY))
        }
        #[cfg(target_vendor = "apple")]
        {
            let _slot = SLOTS.acquire().await.map_err(|_| Error::Cancelled)?;
            // Acquire the data source before creating sockets, and keep every
            // spawn action source above fd 2 to avoid dup-order collisions.
            let descriptor = rustix::io::fcntl_dupfd_cloexec(data, 3)?;
            let (parent, child_control) = std::os::unix::net::UnixStream::pair()?;
            parent.set_nonblocking(true)?;
            let mut control = tokio::net::UnixStream::from_std(parent)?;
            // Both control aliases belong to the child endpoint, never the parent
            // endpoint. Parent death therefore produces EOF even during blocked IO.
            let mut child = spawn(&self.executable, &child_control, &descriptor)?;
            drop(child_control);
            drop(descriptor);
            let mut magic = [0; MAGIC.len()];
            // A trusted but incompatible executable cannot indefinitely stall setup.
            tokio::time::timeout(Duration::from_secs(5), control.read_exact(&mut magic))
                .await
                .map_err(|_| Error::Configuration("stdio worker handshake timed out"))??;
            if magic != MAGIC {
                return Err(Error::Configuration("incompatible stdio worker"));
            }
            let size = if operation == READ {
                config::MAX_MESSAGE_BYTES
            } else {
                bytes.len()
            };
            tokio::time::timeout(Duration::from_secs(5), async {
                control.write_all(&[operation]).await?;
                control.write_all(&(size as u32).to_be_bytes()).await?;
                control.write_all(&bytes).await?;
                if control.read_u8().await? != STARTED {
                    return Err(Error::Configuration("invalid stdio worker acknowledgement"));
                }
                Ok(())
            })
            .await
            .map_err(|_| Error::Configuration("stdio worker setup timed out"))??;
            let mut header = [0; 5];
            control.read_exact(&mut header).await?;
            let number = u32::from_be_bytes(header[1..].try_into().expect("fixed header"));
            let result = match header[0] {
                0 if (operation == READ && number as usize <= config::MAX_MESSAGE_BYTES)
                    || (operation == WRITE && number == 0) =>
                {
                    let mut result = Zeroizing::new(vec![0; number as usize]);
                    control.read_exact(&mut result).await?;
                    Ok(result)
                }
                1 if number == 0 => Err(Error::MessageTooLarge),
                2 if number > 0 && number <= i32::MAX as u32 => {
                    Err(Error::Io(io::Error::from_raw_os_error(number as i32)))
                }
                _ => Err(Error::Configuration("invalid stdio worker response")),
            };
            // Reap successful and failed operations alike. Drop is the cancellation
            // path and kills/reaps synchronously, independent of the Tokio runtime.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
            loop {
                if let Some(success) = child.try_wait()? {
                    if !success {
                        return Err(Error::Configuration("stdio worker failed"));
                    }
                    return result;
                }
                if tokio::time::Instant::now() >= deadline {
                    return Err(Error::Configuration("stdio worker did not exit"));
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
}

#[cfg(any(target_vendor = "apple", test))]
struct Reap(Option<rustix::process::Pid>);

#[cfg(any(target_vendor = "apple", test))]
impl Reap {
    fn try_wait(&mut self) -> Result<Option<bool>> {
        let Some(pid) = self.0 else {
            return Ok(Some(true));
        };
        match rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG) {
            Ok(Some((_, status))) => {
                self.0 = None;
                Ok(Some(status.exit_status() == Some(0)))
            }
            Ok(None) | Err(rustix::io::Errno::INTR) => Ok(None),
            Err(rustix::io::Errno::CHILD) => {
                // Disarm Drop BEFORE returning the host-contract error. Never
                // signal a PID after learning that we no longer own its child.
                self.0 = None;
                Err(Error::Configuration(
                    "stdio worker was reaped by another host component",
                ))
            }
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(any(target_vendor = "apple", test))]
impl Drop for Reap {
    fn drop(&mut self) {
        if matches!(self.try_wait(), Ok(Some(_))) {
            return;
        }
        if let Some(pid) = self.0.take() {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
            while matches!(
                rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::empty()),
                Err(rustix::io::Errno::INTR)
            ) {}
        }
    }
}

#[cfg(target_vendor = "apple")]
fn spawn(
    executable: &std::path::Path,
    control: &std::os::unix::net::UnixStream,
    data: &impl std::os::fd::AsFd,
) -> Result<Reap> {
    use nix::spawn::{PosixSpawnAttr, PosixSpawnFileActions, PosixSpawnFlags, posix_spawn};
    use std::{
        ffi::{CStr, CString},
        os::{fd::AsRawFd, unix::ffi::OsStrExt},
    };
    let io_error = |error: nix::errno::Errno| Error::Io(io::Error::from_raw_os_error(error as i32));
    let control = rustix::io::fcntl_dupfd_cloexec(control, 3)?;
    let data = rustix::io::fcntl_dupfd_cloexec(data, 3)?;
    let mut actions = PosixSpawnFileActions::init().map_err(io_error)?;
    actions.add_dup2(control.as_raw_fd(), 0).map_err(io_error)?;
    actions.add_dup2(control.as_raw_fd(), 1).map_err(io_error)?;
    actions.add_dup2(data.as_raw_fd(), 2).map_err(io_error)?;
    // Close only these owned originals, AFTER all dup actions. The default
    // policy excludes every other descriptor, including non-CLOEXEC SDK fds.
    actions.add_close(control.as_raw_fd()).map_err(io_error)?;
    actions.add_close(data.as_raw_fd()).map_err(io_error)?;
    let mut attributes = PosixSpawnAttr::init().map_err(io_error)?;
    attributes
        .set_flags(PosixSpawnFlags::from_bits_retain(
            nix::libc::POSIX_SPAWN_CLOEXEC_DEFAULT,
        ))
        .map_err(io_error)?;
    let path = CString::new(executable.as_os_str().as_bytes())
        .map_err(|_| Error::Configuration("stdio worker executable path contains NUL"))?;
    let argument = CString::new(ARGUMENT).expect("fixed worker argument");
    // The worker needs no ambient application environment or credentials.
    let environment: [&CStr; 0] = [];
    let pid = posix_spawn(
        path.as_c_str(),
        &actions,
        &attributes,
        &[path.as_c_str(), argument.as_c_str()],
        &environment,
    )
    .map_err(io_error)?;
    Ok(Reap(Some(
        rustix::process::Pid::from_raw(pid.as_raw()).expect("spawn returned positive child PID"),
    )))
}

/// Return `Some` only for the reserved worker invocation. Call before Tokio,
/// application initialization, logging or argument parsing. No recursive spawn.
pub fn dispatch() -> Option<ExitCode> {
    #[cfg(not(target_vendor = "apple"))]
    {
        None
    }
    #[cfg(target_vendor = "apple")]
    {
        let mut arguments = std::env::args_os().skip(1);
        if arguments.next().as_deref() != Some(std::ffi::OsStr::new(ARGUMENT)) {
            return None;
        }
        Some(if arguments.next().is_none() && serve().is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        })
    }
}

#[cfg(any(target_vendor = "apple", test))]
fn serve() -> Result<()> {
    let control = File::from(rustix::io::fcntl_dupfd_cloexec(io::stdin(), 0)?);
    let response = File::from(rustix::io::fcntl_dupfd_cloexec(io::stdout(), 0)?);
    let data = File::from(rustix::io::fcntl_dupfd_cloexec(io::stderr(), 0)?);
    protocol::serve(control, response, data)
}

#[cfg(test)]
mod ownership_tests {
    use super::*;

    #[test]
    fn external_reaping_clears_ownership_before_drop() {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = rustix::process::Pid::from_raw(child.id() as i32).unwrap();
        child.wait().unwrap(); // Deliberate violation of the documented contract.
        let mut owner = Reap(Some(pid));
        assert!(matches!(owner.try_wait(), Err(Error::Configuration(_))));
        assert!(owner.0.is_none(), "ECHILD must disarm cancellation signals");
        drop(owner);
    }
}

// Controlled fixture code exists ONLY in the library test executable. Linux
// production dispatch/configuration cannot reach it or enable worker spawning.
#[cfg(all(test, not(target_vendor = "apple")))]
mod tests {
    use super::*;
    use std::{
        io::Seek,
        net::Shutdown,
        os::fd::OwnedFd,
        os::unix::net::UnixStream,
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn wait(child: &mut Child) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "controlled fixture exceeded deadline"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn command(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "stdio_worker::tests::controlled_fixture",
                "--nocapture",
            ])
            .env("KK_CONTROLLED_STDIO_FIXTURE", mode);
        command
    }
    fn raw(data: Stdio) -> (UnixStream, ChildGuard) {
        let (mut parent, child) = UnixStream::pair().unwrap();
        parent
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        parent
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        rustix::net::sockopt::set_socket_send_buffer_size(&child, 1024).unwrap();
        assert!(
            rustix::net::sockopt::socket_send_buffer_size(&child).unwrap()
                < config::MAX_MESSAGE_BYTES
        );
        let child = ChildGuard(
            command("worker")
                .stdin(Stdio::from(OwnedFd::from(child.try_clone().unwrap())))
                .stdout(Stdio::from(OwnedFd::from(child)))
                .stderr(data)
                .spawn()
                .unwrap(),
        );
        // libtest emits its bounded initial report before entering the fixture.
        let mut prefix = Vec::new();
        while !prefix.ends_with(MAGIC) {
            assert!(prefix.len() < 1024, "fixture did not enter protocol");
            let mut byte = [0];
            parent.read_exact(&mut byte).unwrap();
            prefix.push(byte[0]);
        }
        (parent, child)
    }
    fn request(control: &mut UnixStream, operation: u8, size: usize, bytes: &[u8]) {
        control.write_all(&[operation]).unwrap();
        control.write_all(&(size as u32).to_be_bytes()).unwrap();
        control.write_all(bytes).unwrap();
        let mut started = [0];
        control.read_exact(&mut started).unwrap();
        assert_eq!(started, [STARTED]);
    }
    fn response(control: &mut UnixStream) -> (u8, u32, Vec<u8>) {
        let mut header = [0; 5];
        control.read_exact(&mut header).unwrap();
        let number = u32::from_be_bytes(header[1..].try_into().unwrap());
        let mut bytes = if header[0] == 0 {
            assert!(number as usize <= config::MAX_MESSAGE_BYTES);
            vec![0; number as usize]
        } else {
            Vec::new()
        };
        control.read_exact(&mut bytes).unwrap();
        (header[0], number, bytes)
    }
    #[test]
    fn protocol_flow_eof_input_bound_and_broken_pipe() {
        for size in [0, config::MAX_MESSAGE_BYTES, config::MAX_MESSAGE_BYTES + 1] {
            let mut file = tempfile::tempfile().unwrap();
            // Raw worker input accepts arbitrary bytes; text UTF-8 belongs to
            // input::text and is covered by the pipe/socket integration tests.
            file.write_all(&vec![0xff; size]).unwrap();
            file.rewind().unwrap();
            let (mut control, mut child) = raw(Stdio::from(file));
            request(&mut control, READ, config::MAX_MESSAGE_BYTES, &[]);
            let (code, count, bytes) = response(&mut control);
            if size > config::MAX_MESSAGE_BYTES {
                assert_eq!((code, count), (1, 0));
            } else {
                assert_eq!((code, count as usize), (0, size));
                assert_eq!(bytes, vec![0xff; size]);
            }
            assert!(wait(&mut child.0).success());
        }
        let mut file = tempfile::tempfile().unwrap();
        let (mut control, mut child) = raw(Stdio::from(file.try_clone().unwrap()));
        request(&mut control, WRITE, 8, b"fixture\n");
        assert_eq!(response(&mut control), (0, 0, Vec::new()));
        assert!(wait(&mut child.0).success());
        file.rewind().unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"fixture\n");
        let (read, write) = rustix::pipe::pipe().unwrap();
        drop(read);
        let (mut control, mut child) = raw(Stdio::from(write));
        request(&mut control, WRITE, 1, b"x");
        let (code, errno, _) = response(&mut control);
        assert_eq!(code, 2);
        assert_eq!(
            io::Error::from_raw_os_error(errno as i32).kind(),
            io::ErrorKind::BrokenPipe
        );
        assert!(wait(&mut child.0).success());
    }
    #[test]
    fn protocol_rejects_invalid_limits_and_incomplete_payload() {
        for (operation, size) in [(0, 0), (READ, 0), (READ, u32::MAX), (WRITE, u32::MAX)] {
            let (mut control, mut child) = raw(Stdio::null());
            control.write_all(&[operation]).unwrap();
            control.write_all(&size.to_be_bytes()).unwrap();
            assert!(!wait(&mut child.0).success());
        }
        let (mut control, mut child) = raw(Stdio::null());
        control.write_all(&[WRITE]).unwrap();
        control.write_all(&10u32.to_be_bytes()).unwrap();
        control.shutdown(Shutdown::Write).unwrap();
        assert!(!wait(&mut child.0).success());
    }
    #[test]
    fn supervisor_eof_terminates_blocked_input_output_and_response() {
        let (source, _keep_open) = UnixStream::pair().unwrap();
        let (mut control, mut child) = raw(Stdio::from(OwnedFd::from(source)));
        request(&mut control, READ, config::MAX_MESSAGE_BYTES, &[]);
        control.shutdown(Shutdown::Write).unwrap();
        assert_eq!(wait(&mut child.0).code(), Some(125));
        let (_read, write) = rustix::pipe::pipe().unwrap();
        let (mut control, mut child) = raw(Stdio::from(write));
        let bytes = vec![b'x'; OUTPUT_LIMIT];
        request(&mut control, WRITE, bytes.len(), &bytes);
        control.shutdown(Shutdown::Write).unwrap();
        assert_eq!(wait(&mut child.0).code(), Some(125));
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&vec![0; config::MAX_MESSAGE_BYTES]).unwrap();
        file.rewind().unwrap();
        let (mut control, mut child) = raw(Stdio::from(file));
        request(&mut control, READ, config::MAX_MESSAGE_BYTES, &[]);
        std::thread::sleep(Duration::from_millis(100));
        control.shutdown(Shutdown::Write).unwrap();
        assert_eq!(wait(&mut child.0).code(), Some(125));
    }
    #[test]
    fn controlled_cancellation_kills_and_reaps() {
        let (source, _keep_open) = UnixStream::pair().unwrap();
        let (mut control, child) = raw(Stdio::from(OwnedFd::from(source)));
        request(&mut control, READ, config::MAX_MESSAGE_BYTES, &[]);
        let pid = rustix::process::Pid::from_raw(child.0.id() as i32).unwrap();
        drop(child);
        assert_eq!(
            rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG).unwrap_err(),
            rustix::io::Errno::CHILD
        );
    }
    #[test]
    fn abrupt_parent_death_terminates_controlled_worker() {
        for mode in ["parent-read", "parent-write"] {
            let (diagnostics, child_diagnostics) = UnixStream::pair().unwrap();
            diagnostics
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut parent = ChildGuard(
                command(mode)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::from(OwnedFd::from(child_diagnostics)))
                    .spawn()
                    .unwrap(),
            );
            let mut line = String::new();
            std::io::BufRead::read_line(&mut std::io::BufReader::new(diagnostics), &mut line)
                .unwrap();
            let pid: u32 = line.trim().parse().unwrap();
            parent.0.kill().unwrap();
            assert!(!wait(&mut parent.0).success());
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let status = Command::new("ps")
                    .args(["-o", "stat=", "-p", &pid.to_string()])
                    .output()
                    .unwrap();
                let state = String::from_utf8(status.stdout).unwrap();
                if state.trim().is_empty() || state.trim().starts_with('Z') {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "controlled orphan survived parent death"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    #[test]
    fn controlled_fixture() {
        let Ok(mode) = std::env::var("KK_CONTROLLED_STDIO_FIXTURE") else {
            return;
        };
        if mode == "worker" {
            std::process::exit(if serve().is_ok() { 0 } else { 1 });
        }
        let descriptor = if mode == "parent-read" {
            rustix::io::fcntl_dupfd_cloexec(io::stdin(), 3).unwrap()
        } else {
            rustix::io::fcntl_dupfd_cloexec(io::stdout(), 3).unwrap()
        };
        let (mut control, mut worker) = raw(Stdio::from(descriptor));
        if mode == "parent-read" {
            request(&mut control, READ, config::MAX_MESSAGE_BYTES, &[]);
        } else {
            let bytes = vec![0; OUTPUT_LIMIT];
            request(&mut control, WRITE, bytes.len(), &bytes);
        }
        writeln!(io::stderr(), "{}", worker.0.id()).unwrap();
        let _ = worker.0.wait();
        panic!("controlled worker unexpectedly completed");
    }
}
