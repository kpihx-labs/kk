//! Bounded Windows stdio through a trusted, explicitly registered worker.
//! Hosts must dispatch before application initialization, keep std handles stable
//! and not leave unrelated inheritable handles open while spawning workers.
use crate::{Error, Result, config};
use std::{
    fs::File,
    io::{self, IsTerminal, Read, Write},
    os::windows::io::AsHandle,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Child, Command, ExitCode, Stdio},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::oneshot;
use zeroize::Zeroizing;

#[path = "protocol.rs"]
mod protocol;
use protocol::{MAGIC, OUTPUT_LIMIT, READ, STARTED, WRITE};

const ARGUMENT: &str = "--kk-internal-stdio-worker-v1";
static EXECUTABLE: OnceLock<PathBuf> = OnceLock::new();
static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);

pub fn configure(executable: PathBuf) -> Result<()> {
    let worker = Worker::new(executable)?;
    match EXECUTABLE.set(worker.executable.clone()) {
        Ok(()) => Ok(()),
        Err(path) if EXECUTABLE.get() == Some(&path) => Ok(()),
        Err(_) => Err(Error::Configuration("stdio worker is already configured")),
    }
}

pub fn configured() -> Result<Worker> {
    let executable = EXECUTABLE.get().ok_or(Error::Configuration(
        "Windows bounded stdio requires an explicitly configured compatible worker",
    ))?;
    Ok(Worker {
        executable: executable.clone(),
    })
}

pub struct Worker {
    executable: PathBuf,
}

impl Worker {
    pub fn new(executable: PathBuf) -> Result<Self> {
        if !executable.is_absolute() || !executable.is_file() {
            return Err(Error::Configuration(
                "stdio worker requires a trusted absolute executable path",
            ));
        }
        Ok(Self { executable })
    }

    pub async fn read(&self) -> Result<Zeroizing<Vec<u8>>> {
        if io::stdin().is_terminal() {
            return Err(Error::Configuration(
                "Windows console input is not supported by the raw worker; use redirected UTF-8 stdin",
            ));
        }
        self.exchange(READ, io::stdin(), Zeroizing::new(Vec::new()))
            .await
    }

    pub async fn write(&self, bytes: Zeroizing<Vec<u8>>) -> Result<()> {
        self.write_to(io::stdout(), bytes).await
    }

    pub async fn write_stderr(&self, bytes: Zeroizing<Vec<u8>>) -> Result<()> {
        self.write_to(io::stderr(), bytes).await
    }

    async fn write_to(&self, data: impl AsHandle, bytes: Zeroizing<Vec<u8>>) -> Result<()> {
        if bytes.len() > OUTPUT_LIMIT {
            return Err(Error::Configuration("stdio worker output exceeds limit"));
        }
        self.exchange(WRITE, data, bytes).await.map(|_| ())
    }

    async fn exchange(
        &self,
        operation: u8,
        data: impl AsHandle,
        bytes: Zeroizing<Vec<u8>>,
    ) -> Result<Zeroizing<Vec<u8>>> {
        let slot = SLOTS.acquire().await.map_err(|_| Error::Cancelled)?;
        let data = File::from(data.as_handle().try_clone_to_owned()?);
        let console = data.is_terminal();
        let executable = self.executable.clone();
        let ownership = Arc::new(Ownership {
            cancelled: AtomicBool::new(false),
            child: Mutex::new(None),
        });
        let _cancel = Cancel(ownership.clone());
        let (started, ready) = oneshot::channel();
        // The permit belongs to the blocking job, not its waiter. Cancellation
        // cannot accumulate queued/running IO jobs beyond the four-worker bound.
        let task = tokio::task::spawn_blocking(move || {
            let _slot = slot;
            let _child_owner = Cancel(ownership.clone());
            let (mut control, mut response) = {
                let mut owned = ownership
                    .child
                    .lock()
                    .map_err(|_| Error::Configuration("stdio ownership poisoned"))?;
                if ownership.cancelled.load(Ordering::Acquire) {
                    return Err(Error::Cancelled);
                }
                let mut command = Command::new(executable);
                if !console {
                    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW.
                }
                command
                    .arg(ARGUMENT)
                    .env_clear()
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::from(data));
                if let Some(root) = std::env::var_os("SystemRoot") {
                    command.env("SystemRoot", root);
                }
                *owned = Some(command.spawn()?);
                let child = owned.as_mut().expect("stored child");
                let control = child
                    .stdin
                    .take()
                    .ok_or(Error::Configuration("stdio control pipe missing"))?;
                let response = child
                    .stdout
                    .take()
                    .ok_or(Error::Configuration("stdio response pipe missing"))?;
                (control, response)
            };
            let mut magic = [0; MAGIC.len()];
            response.read_exact(&mut magic)?;
            if magic != MAGIC {
                return Err(Error::Configuration("incompatible stdio worker"));
            }
            let size = if operation == READ {
                config::MAX_MESSAGE_BYTES
            } else {
                bytes.len()
            };
            control.write_all(&[operation])?;
            control.write_all(&(size as u32).to_be_bytes())?;
            control.write_all(&bytes)?;
            control.flush()?;
            let mut acknowledgement = [0];
            response.read_exact(&mut acknowledgement)?;
            if acknowledgement[0] != STARTED {
                return Err(Error::Configuration("invalid stdio worker acknowledgement"));
            }
            let _ = started.send(());
            let mut header = [0; 5];
            response.read_exact(&mut header)?;
            let number = u32::from_be_bytes(header[1..].try_into().expect("fixed header"));
            let result = match header[0] {
                0 if (operation == READ && number as usize <= config::MAX_MESSAGE_BYTES)
                    || (operation == WRITE && number == 0) =>
                {
                    let mut value = Zeroizing::new(vec![0; number as usize]);
                    response.read_exact(&mut value)?;
                    Ok(value)
                }
                1 if number == 0 => Err(Error::MessageTooLarge),
                2 if number > 0 && number <= i32::MAX as u32 => {
                    Err(Error::Io(io::Error::from_raw_os_error(number as i32)))
                }
                _ => Err(Error::Configuration("invalid stdio worker response")),
            };
            // Keep the control writer alive until normal exit. Closing it early
            // would trigger the worker's parent-death supervisor unnecessarily.
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if ownership.cancelled.load(Ordering::Acquire) {
                    return Err(Error::Cancelled);
                }
                let status = {
                    let mut owned = ownership
                        .child
                        .lock()
                        .map_err(|_| Error::Configuration("stdio ownership poisoned"))?;
                    let child = owned.as_mut().ok_or(Error::Cancelled)?;
                    let status = child.try_wait()?;
                    if status.is_some() {
                        owned.take();
                    }
                    status
                };
                if let Some(status) = status {
                    return if status.success() {
                        result
                    } else {
                        Err(Error::Configuration("stdio worker failed"))
                    };
                }
                if Instant::now() >= deadline {
                    return Err(Error::Configuration("stdio worker did not exit"));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        match tokio::time::timeout(Duration::from_secs(5), ready).await {
            Ok(Ok(())) | Ok(Err(_)) => task
                .await
                .map_err(|_| Error::Configuration("stdio worker owner failed"))?,
            Err(_) => Err(Error::Configuration("stdio worker setup timed out")),
        }
    }
}

struct Ownership {
    cancelled: AtomicBool,
    child: Mutex<Option<Child>>,
}

struct Cancel(Arc<Ownership>);

impl Drop for Cancel {
    fn drop(&mut self) {
        self.0.cancelled.store(true, Ordering::Release);
        // Keep cleanup under the lock: another cancellation guard cannot return
        // while a peer guard is still terminating the same owned process.
        let mut owned = self
            .0
            .child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(mut child) = owned.take()
            && !matches!(child.try_wait(), Ok(Some(_)))
        {
            let _ = child.kill(); // Owned process handle, never a recycled PID.
            let _ = child.wait();
        }
    }
}

pub fn dispatch() -> Option<ExitCode> {
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

fn serve() -> Result<()> {
    let control = File::from(io::stdin().as_handle().try_clone_to_owned()?);
    let response = File::from(io::stdout().as_handle().try_clone_to_owned()?);
    let data = File::from(io::stderr().as_handle().try_clone_to_owned()?);
    protocol::serve(control, response, data)
}
