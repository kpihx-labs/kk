//! Same-user local commands only. No HTTP, shell execution or network trust bootstrap.

use serde::{Deserialize, Serialize};
use std::{
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};
use zeroize::{Zeroize, Zeroizing};

use super::{Handle, Status};
use crate::{Error, Result, config, files};

const MAX_FRAME: usize = config::MAX_MESSAGE_BYTES * 6 + 8192;

#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
pub enum Request {
    Text {
        target: String,
        text: String,
        sensitive: bool,
        seconds: u64,
    },
    File {
        target: String,
        source: PathBuf,
        sensitive: bool,
        seconds: u64,
    },
    Status,
}
impl Drop for Request {
    fn drop(&mut self) {
        if let Self::Text { text, .. } = self {
            text.zeroize();
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub ok: bool,
    pub code: String,
    pub bytes: u64,
    pub resumed_from: u64,
    pub digest: Option<[u8; 32]>,
    pub sensitive: bool,
    pub status: Option<Status>,
}
impl Reply {
    fn success(bytes: u64, sensitive: bool) -> Self {
        Self {
            ok: true,
            code: String::new(),
            bytes,
            resumed_from: 0,
            digest: None,
            sensitive,
            status: None,
        }
    }
    pub fn result(&self) -> Result<()> {
        if self.ok {
            return Ok(());
        }
        Err(match self.code.as_str() {
            "DENIED" => Error::Denied,
            "REVOKED" => Error::Revoked,
            "TIMEOUT" => Error::Timeout,
            "AUTHENTICATION" => Error::Authentication,
            "INTEGRITY" => Error::Integrity,
            "FILE_TOO_LARGE" => Error::FileTooLarge,
            "MESSAGE_TOO_LARGE" => Error::MessageTooLarge,
            "DESTINATION" => Error::Destination("session file operation refused"),
            _ => Error::Node("local session command failed; inspect its event stream"),
        })
    }
    pub fn receipt(&self) -> files::Receipt {
        files::Receipt {
            info: files::FileInfo {
                bytes: self.bytes,
                sha256: self.digest.unwrap_or([0; 32]),
            },
            resumed_from: self.resumed_from,
            sensitive: self.sensitive,
        }
    }
}

pub(super) struct Listener {
    listener: UnixListener,
    path: PathBuf,
    inode: u64,
    _lock: std::fs::File,
}
impl Listener {
    pub async fn bind(profile: &Path) -> Result<Self> {
        use rustix::fs::{self, Mode, OFlags};
        let lock = std::fs::File::from(fs::open(
            profile.join("node.lock"),
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )?);
        let metadata = lock.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o777 != 0o600
            || metadata.nlink() != 1
        {
            return Err(Error::Node("unsafe node lock"));
        }
        fs::flock(&lock, fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| Error::Node("profile already has an active node"))?;
        let path = profile.join("node.sock");
        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
            safe_socket(&metadata)?;
            if UnixStream::connect(&path).await.is_ok() {
                return Err(Error::Node("active node socket without lock"));
            }
            std::fs::remove_file(&path)?; // Owned stale runtime endpoint, not user data.
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let inode = std::fs::symlink_metadata(&path)?.ino();
        Ok(Self {
            listener,
            path,
            inode,
            _lock: lock,
        })
    }
    pub async fn accept(&self) -> Result<UnixStream> {
        let (stream, _) = self.listener.accept().await?;
        check_user(&stream)?;
        Ok(stream)
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        if let Ok(metadata) = std::fs::symlink_metadata(&self.path)
            && metadata.ino() == self.inode
            && safe_socket(&metadata).is_ok()
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
fn safe_socket(metadata: &std::fs::Metadata) -> Result<()> {
    if !metadata.file_type().is_socket()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
    {
        return Err(Error::Node("unsafe local session socket"));
    }
    Ok(())
}
fn check_user(stream: &UnixStream) -> Result<()> {
    if stream.peer_cred()?.uid() != rustix::process::geteuid().as_raw() {
        return Err(Error::Denied);
    }
    Ok(())
}
async fn read<T: serde::de::DeserializeOwned>(stream: &mut UnixStream) -> Result<T> {
    let length = stream.read_u32().await? as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(Error::Protocol);
    }
    let mut bytes = Zeroizing::new(vec![0; length]);
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|_| Error::Protocol)
}
async fn write<T: Serialize>(stream: &mut UnixStream, value: &T) -> Result<()> {
    let bytes = Zeroizing::new(serde_json::to_vec(value).map_err(|_| Error::Protocol)?);
    if bytes.len() > MAX_FRAME {
        return Err(Error::Protocol);
    }
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .await?;
    stream.write_all(&bytes).await?;
    Ok(())
}

pub(super) async fn serve(
    mut stream: UnixStream,
    handle: Handle,
    admission: tokio::sync::OwnedSemaphorePermit,
    text_slots: std::sync::Arc<tokio::sync::Semaphore>,
    file_slots: std::sync::Arc<tokio::sync::Semaphore>,
) {
    let mut admission = Some(admission);
    let mut _work = None;
    let operation = async {
        let request: Request = tokio::time::timeout(config::DEFAULT_TIMEOUT, read(&mut stream))
            .await
            .map_err(|_| Error::Timeout)??;
        let outcome: Result<Reply> = async {
            _work = match &request {
                Request::Text { .. } => Some(
                    text_slots
                        .try_acquire_owned()
                        .map_err(|_| Error::Node("local text queue is full"))?,
                ),
                Request::File { .. } => Some(
                    file_slots
                        .try_acquire_owned()
                        .map_err(|_| Error::Node("local file queue is full"))?,
                ),
                Request::Status => None,
            };
            // Long work has its own bounded lane. Keep reader/status admission
            // available instead of occupying every control slot with bulk files.
            if _work.is_some() {
                drop(admission.take());
            }
            match &request {
                Request::Text {
                    target,
                    text,
                    sensitive,
                    seconds,
                } => handle
                    .send_text(target, text, *sensitive, Duration::from_secs(*seconds))
                    .await
                    .map(|()| Reply::success(text.len() as u64, *sensitive)),
                Request::File {
                    target,
                    source,
                    sensitive,
                    seconds,
                } => handle
                    .send_file(target, source, *sensitive, Duration::from_secs(*seconds))
                    .await
                    .map(|receipt| {
                        let mut reply = Reply::success(receipt.info.bytes, receipt.sensitive);
                        reply.resumed_from = receipt.resumed_from;
                        if !receipt.sensitive {
                            reply.digest = Some(receipt.info.sha256);
                        }
                        reply
                    }),
                Request::Status => handle.status().await.map(|status| {
                    let mut reply = Reply::success(0, false);
                    reply.status = Some(status);
                    reply
                }),
            }
        }
        .await;
        let reply = outcome.unwrap_or_else(|error| Reply {
            ok: false,
            code: error.code().to_owned(),
            bytes: 0,
            resumed_from: 0,
            digest: None,
            sensitive: false,
            status: None,
        });
        tokio::time::timeout(config::DEFAULT_TIMEOUT, write(&mut stream, &reply))
            .await
            .map_err(|_| Error::Timeout)?
    };
    // Operation deadlines are validated by Handle; this bounds socket shutdown too.
    let _ =
        tokio::time::timeout(config::MAX_FILE_TIMEOUT + Duration::from_secs(5), operation).await;
}

pub async fn request(
    profile: &Path,
    request: &Request,
    deadline: Duration,
) -> Result<Option<Reply>> {
    crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
    crate::profile::private_directory(profile, false)?;
    let path = profile.join("node.sock");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    safe_socket(&metadata)?;
    tokio::time::timeout(deadline, async {
        let mut stream = UnixStream::connect(&path)
            .await
            .map_err(|_| Error::Node("stale/unavailable session; no automatic network retry"))?;
        check_user(&stream)?;
        write(&mut stream, request).await?;
        let reply: Reply = read(&mut stream).await?;
        reply.result()?;
        Ok(Some(reply))
    })
    .await
    .map_err(|_| Error::Timeout)?
}
