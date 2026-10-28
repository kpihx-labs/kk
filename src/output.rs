//! Cancellable, bounded foreground output; a stalled pipe must not stall the node.

#[cfg(unix)]
use crate::config;
use crate::{Error, Result};
#[cfg(unix)]
use std::io::{self, Write};
#[cfg(unix)]
use zeroize::Zeroizing;

#[cfg(unix)]
pub async fn line(value: String) -> Result<()> {
    let mut bytes = Zeroizing::new(value.into_bytes());
    if bytes.len() > config::MAX_MESSAGE_BYTES * 6 + 8192 {
        return Err(Error::Configuration("output line exceeds limit"));
    }
    bytes.push(b'\n');
    write_bytes(bytes, false).await
}

/// Diagnostic output uses the same cancellation boundary as foreground output.
#[cfg(unix)]
pub(crate) async fn diagnostic(bytes: &[u8]) -> Result<()> {
    write_bytes(Zeroizing::new(bytes.to_vec()), true).await
}

#[cfg(unix)]
async fn write_bytes(bytes: Zeroizing<Vec<u8>>, stderr: bool) -> Result<()> {
    if bytes.len() > config::MAX_MESSAGE_BYTES * 6 + 8193 {
        return Err(Error::Configuration("output exceeds limit"));
    }
    let path = if stderr { "/dev/stderr" } else { "/dev/stdout" };
    #[cfg(target_vendor = "apple")]
    {
        let kind = std::fs::metadata(path)?.file_type();
        if !kind.is_file() {
            let worker = crate::stdio_worker::configured()?;
            return if stderr {
                worker.write_stderr(bytes).await
            } else {
                worker.write(bytes).await
            };
        }
    }
    // Owned CLOEXEC endpoints; std configures CLOEXEC non-atomically on Apple.
    // Closing the future's endpoint wakes poll without a write or SIGPIPE.
    let (read, write) = std::os::unix::net::UnixStream::pair()?;
    let result = tokio::task::spawn_blocking(move || {
        use rustix::{
            event::{PollFd, PollFlags, poll},
            fs::{Mode, OFlags},
        };
        use std::os::unix::fs::FileTypeExt;
        let metadata = std::fs::metadata(path)?;
        let socket = metadata.file_type().is_socket();
        let file = if metadata.is_file() || socket {
            let descriptor = if stderr {
                rustix::io::fcntl_dupfd_cloexec(io::stderr(), 0)?
            } else {
                rustix::io::fcntl_dupfd_cloexec(io::stdout(), 0)?
            };
            std::fs::File::from(descriptor)
        } else {
            std::fs::File::from(rustix::fs::open(
                path,
                OFlags::WRONLY | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )?)
        };
        let mut position = 0;
        while position < bytes.len() {
            let mut descriptors = [
                PollFd::new(&file, PollFlags::OUT),
                PollFd::new(&read, PollFlags::IN),
            ];
            match poll(&mut descriptors, None) {
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(Error::from(error)),
            }
            if !descriptors[1].revents().is_empty() {
                return Err(Error::Cancelled);
            }
            // Socket descriptors cannot be reopened. Per-call flags avoid changing
            // O_NONBLOCK on the caller's shared open-file description.
            let written = if socket {
                #[cfg(target_vendor = "apple")]
                let flags = rustix::net::SendFlags::DONTWAIT;
                #[cfg(not(target_vendor = "apple"))]
                let flags = rustix::net::SendFlags::DONTWAIT | rustix::net::SendFlags::NOSIGNAL;
                rustix::net::send(&file, &bytes[position..], flags).map_err(io::Error::from)
            } else {
                (&file).write(&bytes[position..])
            };
            match written {
                Ok(0) => return Err(Error::Io(io::ErrorKind::WriteZero.into())),
                Ok(count) => position += count,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(Error::Io(error)),
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| Error::Node("output worker failed"))?;
    drop(write);
    result
}

#[cfg(not(unix))]
pub async fn line(_value: String) -> Result<()> {
    Err(Error::UnsupportedPlatform)
}
