//! Cancellable, bounded foreground output; a stalled pipe must not stall the node.

#[cfg(any(unix, windows))]
use crate::config;
use crate::{Error, Result};
#[cfg(unix)]
use std::io::{self, Write};
#[cfg(any(unix, windows))]
use zeroize::Zeroizing;

#[cfg(any(unix, windows))]
pub async fn line(value: String) -> Result<()> {
    let mut bytes = Zeroizing::new(value.into_bytes());
    if bytes.len() > config::MAX_MESSAGE_BYTES * 6 + 8192 {
        return Err(Error::Configuration("output line exceeds limit"));
    }
    bytes.push(b'\n');
    write_bytes(bytes, false).await
}

/// Diagnostic output uses the same cancellation boundary as foreground output.
#[cfg(any(unix, windows))]
pub(crate) async fn diagnostic(bytes: &[u8]) -> Result<()> {
    if bytes.len() > config::MAX_MESSAGE_BYTES * 6 + 8193 {
        return Err(Error::Configuration("output exceeds limit"));
    }
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
        use rustix::fs::{Mode, OFlags};
        use std::os::unix::fs::FileTypeExt;
        let metadata = std::fs::metadata(path)?;
        #[cfg(target_os = "linux")]
        if metadata.file_type().is_fifo() {
            let descriptor = if stderr {
                rustix::io::fcntl_dupfd_cloexec(io::stderr(), 0)?
            } else {
                rustix::io::fcntl_dupfd_cloexec(io::stdout(), 0)?
            };
            let file = std::fs::File::from(descriptor);
            return write_pipe(&bytes, &file, &read);
        }
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
            wait_writable(&file, &read)?;
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

#[cfg(unix)]
fn wait_writable(
    destination: &impl std::os::fd::AsFd,
    cancellation: &impl std::os::fd::AsFd,
) -> Result<()> {
    use rustix::event::{PollFd, PollFlags, poll};
    loop {
        let mut descriptors = [
            PollFd::new(destination, PollFlags::OUT),
            PollFd::new(cancellation, PollFlags::IN),
        ];
        match poll(&mut descriptors, None) {
            Ok(_) if !descriptors[1].revents().is_empty() => return Err(Error::Cancelled),
            Ok(_) => return Ok(()),
            Err(rustix::io::Errno::INTR) => {}
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(target_os = "linux")]
fn write_pipe(
    bytes: &[u8],
    destination: &std::fs::File,
    cancellation: &impl std::os::fd::AsFd,
) -> Result<()> {
    use rustix::pipe::{PipeFlags, SpliceFlags, pipe_with, splice};
    use std::os::unix::fs::FileTypeExt;
    if !destination.metadata()?.file_type().is_fifo() {
        return Err(Error::Configuration("output pipe descriptor changed"));
    }
    // Both splice endpoints are pipes: NONBLOCK covers their pipe operations,
    // unlike the general pipe-to-file case. Never change the inherited OFD.
    let (staged, staging) = pipe_with(PipeFlags::CLOEXEC | PipeFlags::NONBLOCK)?;
    let mut position = 0;
    while position < bytes.len() {
        let buffered = match rustix::io::write(&staging, &bytes[position..]) {
            Ok(0) => return Err(Error::Io(io::ErrorKind::WriteZero.into())),
            Ok(count) => count,
            Err(rustix::io::Errno::INTR) => continue,
            Err(error) => return Err(error.into()),
        };
        let end = position + buffered;
        while position < end {
            wait_writable(destination, cancellation)?;
            match splice(
                &staged,
                None,
                destination,
                None,
                end - position,
                SpliceFlags::NONBLOCK,
            ) {
                Ok(0) => return Err(Error::Io(io::ErrorKind::WriteZero.into())),
                Ok(count) => position += count,
                Err(rustix::io::Errno::INTR | rustix::io::Errno::AGAIN) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
async fn write_bytes(bytes: Zeroizing<Vec<u8>>, stderr: bool) -> Result<()> {
    if bytes.len() > config::MAX_MESSAGE_BYTES * 6 + 8193 {
        return Err(Error::Configuration("output exceeds limit"));
    }
    let worker = crate::stdio_worker::configured()?;
    if stderr {
        worker.write_stderr(bytes).await
    } else {
        worker.write(bytes).await
    }
}

#[cfg(not(any(unix, windows)))]
pub async fn line(_value: String) -> Result<()> {
    Err(Error::UnsupportedPlatform)
}
