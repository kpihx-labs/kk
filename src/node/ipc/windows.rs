//! Same-owner private Windows IPC. A connected handle is trusted only after
//! handle-bound server security and scoped client-token authentication.
use crate::{
    Error, Result, config,
    profile::windows::{ProtectedDirectory, owner_attributes, validate_security},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{ffi::OsStr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{NamedPipeClient, NamedPipeServer},
};
use windows_namespace_request_sys::private_pipe;
use zeroize::Zeroizing;

const MAX_FRAME: usize = config::MAX_MESSAGE_BYTES * 6 + 8192;
const AUTHENTICATE: u8 = 0x4b;

/// Exactly one retained named-pipe instance. Dropping it closes the namespace;
/// it is never recreated behind an accepted client's back.
pub struct Listener {
    pipe: NamedPipeServer,
    owner: win_authorization_sys::Sid,
}

impl Listener {
    /// Create the first and sole local instance with owner-only DACL.
    pub fn bind(profile: &ProtectedDirectory, name: &OsStr) -> Result<Self> {
        profile.validate()?;
        let owner = profile.owner().clone();
        let security = owner_attributes(&owner)?;
        let pipe = private_pipe::create_server(name, &security)?;
        validate_security(&private_pipe::server_security(&pipe)?, &owner)?;
        Ok(Self { pipe, owner })
    }

    /// Wait for a client, require its first byte, then synchronously impersonate
    /// only long enough to copy its SID. No request bytes are accepted first.
    pub async fn accept(&mut self, deadline: Duration) -> Result<ServerSession<'_>> {
        let expires = tokio::time::Instant::now() + deadline;
        'accept: loop {
            loop {
                let remaining = expires.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    return Err(Error::Timeout);
                }
                match tokio::time::timeout(remaining, self.pipe.connect()).await {
                    Ok(Ok(())) => break,
                    Ok(Err(error)) if matches!(error.raw_os_error(), Some(232) | Some(233)) => {
                        // A cancelled connect can report its stale disconnected state
                        // once before the retained instance can be armed again.
                        let _ = self.pipe.disconnect();
                        tokio::task::yield_now().await;
                    }
                    Ok(Err(error)) => return Err(error.into()),
                    // Tokio guarantees that connect cancellation loses no event;
                    // disconnecting here would instead poison the retained instance.
                    Err(_) => return Err(Error::Timeout),
                }
            }
            let remaining = expires.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                let _ = self.pipe.disconnect();
                return Err(Error::Timeout);
            }
            let accepted = async {
                let mut byte = [0];
                tokio::time::timeout(remaining, self.pipe.read_exact(&mut byte))
                    .await
                    .map_err(|_| Error::Timeout)??;
                if byte != [AUTHENTICATE] {
                    return Err(Error::Authentication);
                }
                let client = private_pipe::client_sid(&self.pipe)?;
                if client != self.owner {
                    return Err(Error::Authentication);
                }
                Ok(())
            }
            .await;
            match accepted {
                Ok(()) => break 'accept,
                Err(Error::Io(error)) if matches!(error.raw_os_error(), Some(232) | Some(233)) => {
                    let _ = self.pipe.disconnect();
                    tokio::task::yield_now().await;
                }
                Err(error) => {
                    let _ = self.pipe.disconnect();
                    return Err(error);
                }
            }
        }
        Ok(ServerSession {
            pipe: &mut self.pipe,
        })
    }
}

/// Authenticated server-side borrow. Drop disconnects before the sole instance
/// can accept another client; no unauthenticated handle escapes.
pub struct ServerSession<'a> {
    pipe: &'a mut NamedPipeServer,
}
impl ServerSession<'_> {
    pub async fn read<T: DeserializeOwned>(&mut self, deadline: Duration) -> Result<T> {
        read_frame(self.pipe, deadline).await
    }
    pub async fn write<T: Serialize>(&mut self, value: &T, deadline: Duration) -> Result<()> {
        write_frame(self.pipe, value, deadline).await
    }
}
impl Drop for ServerSession<'_> {
    fn drop(&mut self) {
        let _ = self.pipe.disconnect();
    }
}

/// Client whose connected server handle was validated before the authentication
/// preamble. This proves server ownership; server independently proves client SID.
pub struct Client {
    pipe: NamedPipeClient,
}
impl Client {
    pub async fn connect(
        profile: &ProtectedDirectory,
        name: &OsStr,
        deadline: Duration,
    ) -> Result<Self> {
        profile.validate()?;
        let expires = tokio::time::Instant::now() + deadline;
        let mut pipe = loop {
            match private_pipe::open_client(name) {
                Ok(pipe) => break pipe,
                Err(error) if error.raw_os_error() == Some(231) => {
                    let remaining = expires.saturating_duration_since(tokio::time::Instant::now());
                    if remaining.is_zero() {
                        return Err(Error::Timeout);
                    }
                    tokio::time::sleep(remaining.min(Duration::from_millis(10))).await;
                }
                Err(error) => return Err(error.into()),
            }
        };
        validate_security(&private_pipe::client_security(&pipe)?, profile.owner())?;
        let remaining = expires.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(Error::Timeout);
        }
        tokio::time::timeout(remaining, pipe.write_all(&[AUTHENTICATE]))
            .await
            .map_err(|_| Error::Timeout)??;
        Ok(Self { pipe })
    }
    pub async fn read<T: DeserializeOwned>(&mut self, deadline: Duration) -> Result<T> {
        read_frame(&mut self.pipe, deadline).await
    }
    pub async fn write<T: Serialize>(&mut self, value: &T, deadline: Duration) -> Result<()> {
        write_frame(&mut self.pipe, value, deadline).await
    }
}

async fn read_frame<T: DeserializeOwned>(
    stream: &mut (impl AsyncReadExt + Unpin),
    deadline: Duration,
) -> Result<T> {
    let length = tokio::time::timeout(deadline, stream.read_u32())
        .await
        .map_err(|_| Error::Timeout)?? as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(Error::MessageTooLarge);
    }
    let mut bytes = Zeroizing::new(vec![0; length]);
    tokio::time::timeout(deadline, stream.read_exact(&mut bytes))
        .await
        .map_err(|_| Error::Timeout)??;
    serde_json::from_slice(&bytes).map_err(|_| Error::Configuration("invalid local IPC frame"))
}

async fn write_frame<T: Serialize>(
    stream: &mut (impl AsyncWriteExt + Unpin),
    value: &T,
    deadline: Duration,
) -> Result<()> {
    let bytes = Zeroizing::new(
        serde_json::to_vec(value)
            .map_err(|_| Error::Configuration("local IPC serialization failed"))?,
    );
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err(Error::MessageTooLarge);
    }
    tokio::time::timeout(deadline, async {
        stream.write_u32(bytes.len() as u32).await?;
        stream.write_all(&bytes).await?;
        stream.flush().await
    })
    .await
    .map_err(|_| Error::Timeout)??;
    Ok(())
}
