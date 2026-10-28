//! Bounded local input. Hidden terminal reads remain cancellable by the runtime.

use crate::{Error, Result, config};
#[cfg(unix)]
use std::io::Read;
use std::io::{self, IsTerminal};
use zeroize::Zeroizing;

pub async fn text(value: Option<&str>, sensitive: bool) -> Result<Zeroizing<String>> {
    if let Some(value) = value.filter(|value| *value != "-") {
        if sensitive {
            return Err(Error::Configuration(
                "sensitive content must come from stdin or hidden input, never argv",
            ));
        }
        if value.len() > config::MAX_MESSAGE_BYTES {
            return Err(Error::MessageTooLarge);
        }
        return Ok(Zeroizing::new(value.to_owned()));
    }
    line(
        sensitive,
        if sensitive {
            "Sensitive input: "
        } else {
            "Input: "
        },
    )
    .await
}

pub async fn passphrase(confirm: bool) -> Result<age::secrecy::SecretString> {
    use age::secrecy::ExposeSecret;
    if !io::stdin().is_terminal() {
        return Err(Error::Configuration(
            "a passphrase requires a local terminal; use recipient encryption unattended",
        ));
    }
    let first = age::secrecy::SecretString::from(
        line(true, "Envelope passphrase: ")
            .await?
            .as_str()
            .to_owned(),
    );
    if first.expose_secret().is_empty() {
        return Err(Error::Configuration("empty passphrase refused"));
    }
    if confirm {
        let second = line(true, "Confirm passphrase: ").await?;
        if first.expose_secret() != second.as_str() {
            return Err(Error::Configuration("passphrases differ"));
        }
    }
    Ok(first)
}

#[cfg(unix)]
async fn line(hidden: bool, prompt: &str) -> Result<Zeroizing<String>> {
    let terminal = io::stdin().is_terminal();
    // Never hold the echo restoration guard across diagnostic IO. Even stderr
    // may be a full pipe; cancellation must join the blocking input worker.
    if terminal {
        crate::output::diagnostic(prompt.as_bytes()).await?;
    } else if hidden {
        crate::output::diagnostic(b"INPUT sensitive stdin\n").await?;
    }
    #[cfg(target_vendor = "apple")]
    {
        use std::os::unix::fs::FileTypeExt;
        let kind = std::fs::metadata("/dev/stdin")?.file_type();
        if !io::stdin().is_terminal() && !kind.is_file() && !kind.is_socket() {
            let worker = crate::stdio_worker::configured()?;
            let bytes = worker.read().await?;
            let value = std::str::from_utf8(&bytes)
                .map_err(|_| Error::Configuration("text input must be UTF-8"))?;
            return Ok(Zeroizing::new(value.to_owned()));
        }
    }
    // std sets CLOEXEC on both owned endpoints (non-atomically on Apple).
    // No data is sent: dropping the future's endpoint wakes the worker's poll
    // with EOF/HUP, including when spawn_blocking itself cannot be cancelled.
    let (cancel_read, cancel_write) = std::os::unix::net::UnixStream::pair()?;
    let result = tokio::task::spawn_blocking(move || blocking_line(hidden, cancel_read.into()))
        .await
        .map_err(|_| Error::Configuration("input worker failed"))?;
    drop(cancel_write);
    if terminal && hidden && result.is_ok() {
        crate::output::diagnostic(b"\n").await?;
    }
    result
}

#[cfg(unix)]
fn blocking_line(hidden: bool, cancel: rustix::fd::OwnedFd) -> Result<Zeroizing<String>> {
    use rustix::event::{PollFd, PollFlags, poll};
    use rustix::{
        fs::{Mode, OFlags},
        termios::{LocalModes, OptionalActions, Termios},
    };
    use std::os::unix::fs::FileTypeExt;
    struct Restore {
        file: std::fs::File,
        original: Termios,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = rustix::termios::tcsetattr(&self.file, OptionalActions::Now, &self.original);
        }
    }
    let terminal = io::stdin().is_terminal();
    let mut bytes = Zeroizing::new(Vec::new());
    let metadata = std::fs::metadata("/dev/stdin")?;
    let socket = metadata.file_type().is_socket();
    let file = if !terminal && (metadata.is_file() || socket) {
        std::fs::File::from(rustix::io::fcntl_dupfd_cloexec(io::stdin(), 0)?)
    } else {
        std::fs::File::from(rustix::fs::open(
            if terminal { "/dev/tty" } else { "/dev/stdin" },
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOCTTY | OFlags::CLOEXEC,
            Mode::empty(),
        )?)
    };
    let _restore = if terminal && hidden {
        let original = rustix::termios::tcgetattr(&file)?;
        // Acquire the restoration descriptor before changing terminal state.
        let restore = Restore {
            file: file.try_clone()?,
            original,
        };
        let mut attributes = restore.original.clone();
        attributes
            .local_modes
            .remove(LocalModes::ECHO | LocalModes::ECHONL);
        rustix::termios::tcsetattr(&file, OptionalActions::Now, &attributes)?;
        Some(restore)
    } else {
        None
    };
    loop {
        let mut descriptors = [
            PollFd::new(&file, PollFlags::IN),
            PollFd::new(&cancel, PollFlags::IN),
        ];
        match poll(&mut descriptors, None) {
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => continue,
            Err(error) => return Err(error.into()),
        }
        if !descriptors[1].revents().is_empty() {
            return Err(Error::Cancelled);
        }
        let mut chunk = Zeroizing::new([0; 1024]);
        let read = if socket {
            rustix::net::recv(&file, &mut chunk[..], rustix::net::RecvFlags::DONTWAIT)
                .map(|(_, count)| count)
                .map_err(io::Error::from)
        } else {
            (&file).read(chunk.as_mut())
        };
        let count = match read {
            Ok(count) => count,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > config::MAX_MESSAGE_BYTES {
            return Err(Error::MessageTooLarge);
        }
        if terminal && bytes.contains(&b'\n') {
            break;
        }
    }
    if terminal {
        // Canonical terminals may truncate very long lines: refuse the boundary.
        if bytes.len() >= 4095 {
            return Err(Error::Configuration(
                "terminal line too long; use bounded stdin",
            ));
        }
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
        }
    }
    let value = std::str::from_utf8(&bytes)
        .map_err(|_| Error::Configuration("text input must be UTF-8"))?;
    Ok(Zeroizing::new(value.to_owned()))
}

#[cfg(not(unix))]
async fn line(_hidden: bool, _prompt: &str) -> Result<Zeroizing<String>> {
    Err(Error::UnsupportedPlatform)
}
