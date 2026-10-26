//! Cancellable, bounded foreground output; a stalled pipe must not stall the node.

use crate::{Error, Result, config};
use std::io::{self, Write};
use zeroize::Zeroizing;

#[cfg(unix)]
pub async fn line(value: String) -> Result<()> {
    let mut bytes = Zeroizing::new(value.into_bytes());
    if bytes.len() > config::MAX_MESSAGE_BYTES * 6 + 8192 {
        return Err(Error::Configuration("output line exceeds limit"));
    }
    bytes.push(b'\n');
    let (read, write) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC)?;
    let result = tokio::task::spawn_blocking(move || {
        use rustix::{
            event::{PollFd, PollFlags, poll},
            fs::{Mode, OFlags},
        };
        use std::os::unix::fs::FileTypeExt;
        let metadata = std::fs::metadata("/dev/stdout")?;
        let socket = metadata.file_type().is_socket();
        let file = if metadata.is_file() || socket {
            std::fs::File::from(rustix::io::dup(io::stdout())?)
        } else {
            std::fs::File::from(rustix::fs::open(
                "/dev/stdout",
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
                rustix::net::send(
                    &file,
                    &bytes[position..],
                    rustix::net::SendFlags::DONTWAIT | rustix::net::SendFlags::NOSIGNAL,
                )
                .map_err(io::Error::from)
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
