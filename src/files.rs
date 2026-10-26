//! One authenticated, consented stream with verified-prefix resume and no clobber.

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use sha2::{Digest, Sha256};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};

use crate::{Error, Identity, Peer, Result, config, error::connection_error, transport};
use zeroize::Zeroizing;

#[cfg(target_os = "linux")]
mod storage;
#[cfg(target_os = "linux")]
use storage::Store;

pub(crate) const ALPN: &[u8] = b"kk-file/2";
mod inbox;
pub(crate) mod stream;
pub(crate) use inbox::Inbox;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileInfo {
    pub bytes: u64,
    pub sha256: [u8; 32],
}

/// Parse a complete SHA-256 content identifier before slicing UTF-8 input.
pub fn parse_hash(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Configuration(
            "select a complete SHA-256 content digest",
        ));
    }
    let mut output = [0; 32];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| Error::Configuration("invalid content digest"))?;
    }
    Ok(output)
}

impl std::fmt::Debug for FileInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileInfo")
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

impl FileInfo {
    pub fn hash_hex(&self) -> String {
        use std::fmt::Write;
        let mut output = String::with_capacity(64);
        for byte in self.sha256 {
            write!(output, "{byte:02x}").expect("String formatting");
        }
        output
    }
}

#[derive(Debug, Clone)]
pub struct Receipt {
    pub info: FileInfo,
    pub resumed_from: u64,
    pub sensitive: bool,
}

#[derive(Clone)]
pub struct ReceivePolicy {
    destination: PathBuf,
    max_bytes: u64,
}

impl ReceivePolicy {
    /// Explicit authorization to receive one file from the separately pinned peer.
    pub fn accept(destination: PathBuf, max_bytes: u64) -> Result<Self> {
        if max_bytes > config::MAX_FILE_BYTES {
            return Err(Error::FileTooLarge);
        }
        if !cfg!(target_os = "linux") {
            return Err(Error::UnsupportedPlatform);
        }
        Ok(Self {
            destination,
            max_bytes,
        })
    }
}

pub struct FileReceiver {
    endpoint: quinn::Endpoint,
    peer: Peer,
    policy: ReceivePolicy,
}

impl FileReceiver {
    pub fn bind(
        address: SocketAddr,
        identity: &Identity,
        peer: &Peer,
        policy: ReceivePolicy,
    ) -> Result<Self> {
        Ok(Self {
            endpoint: transport::server(address, identity, peer, ALPN)?,
            peer: peer.clone(),
            policy,
        })
    }
    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.endpoint.local_addr()?)
    }

    pub async fn receive(self, deadline: Duration) -> Result<Receipt> {
        self.receive_authorized(deadline, || Ok(())).await
    }

    pub async fn receive_authorized(
        self,
        deadline: Duration,
        authorize: impl Fn() -> Result<()>,
    ) -> Result<Receipt> {
        transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        tokio::time::timeout(deadline, async {
            let connection = transport::accept(&self.endpoint, &self.peer).await?;
            let outcome: Result<Receipt> = async {
                authorize()?;
                let (response, request) = connection.accept_bi().await.map_err(connection_error)?;
                let receipt =
                    stream::receive(&self.peer, &self.policy, request, response, &authorize)
                        .await?;
                transport::wait_closed(&connection).await?;
                Ok(receipt)
            }
            .await;
            if outcome.is_err() {
                transport::reject(&self.endpoint, &connection).await;
            }
            outcome
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
}

pub async fn inspect(source: &Path, deadline: Duration) -> Result<FileInfo> {
    transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
    tokio::time::timeout(deadline, async {
        let mut file = source_file(source)?;
        info(&mut file).await
    })
    .await
    .map_err(|_| Error::Timeout)?
}

/// Explicit private output for an already authenticated, bounded message.
pub async fn save_bytes(
    destination: &Path,
    bytes: &[u8],
    peer: &Peer,
    sensitive: bool,
) -> Result<()> {
    if bytes.len() > config::MAX_MESSAGE_BYTES {
        return Err(Error::MessageTooLarge);
    }
    let hash = Sha256::digest(bytes);
    let mut binding = [0; 73];
    binding[0] = u8::from(sensitive);
    binding[1..9].copy_from_slice(&(bytes.len() as u64).to_be_bytes());
    binding[9..41].copy_from_slice(&hash);
    binding[41..].copy_from_slice(&Sha256::digest(peer.certificate.as_ref()));
    let mut store = Store::open(destination, &binding).await?;
    if store.complete || store.file.metadata().await?.len() != 0 {
        return Err(Error::Destination("message output already exists"));
    }
    store.file.write_all(bytes).await?;
    store.file.flush().await?;
    if digest(&mut store.file, bytes.len() as u64)
        .await?
        .as_slice()
        != hash.as_slice()
    {
        return Err(Error::Integrity);
    }
    store.commit().await
}

pub async fn send_file(
    destination: SocketAddr,
    identity: &Identity,
    peer: &Peer,
    source: &Path,
    deadline: Duration,
) -> Result<Receipt> {
    send_file_with_policy(destination, identity, peer, source, false, deadline).await
}

pub async fn send_file_with_policy(
    destination: SocketAddr,
    identity: &Identity,
    peer: &Peer,
    source: &Path,
    sensitive: bool,
    deadline: Duration,
) -> Result<Receipt> {
    send_file_authorized(
        destination,
        identity,
        peer,
        source,
        sensitive,
        deadline,
        || Ok(()),
    )
    .await
}

pub async fn send_file_authorized(
    destination: SocketAddr,
    identity: &Identity,
    peer: &Peer,
    source: &Path,
    sensitive: bool,
    deadline: Duration,
    authorize: impl Fn() -> Result<()>,
) -> Result<Receipt> {
    transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
    authorize()?;
    tokio::time::timeout(deadline, async {
        let mut file = source_file(source)?;
        let info = info(&mut file).await?;
        let endpoint = transport::client(destination, identity, peer, ALPN)?;
        let connection = endpoint
            .connect(destination, config::TLS_NAME)
            .map_err(|_| Error::Configuration("invalid destination"))?
            .await
            .map_err(connection_error)?;
        let outcome: Result<Receipt> = async {
            transport::verify_peer(&connection, peer)?;
            authorize()?;
            let (request, response) = connection.open_bi().await.map_err(connection_error)?;
            let receipt =
                stream::send(&mut file, info, sensitive, request, response, &authorize).await?;
            transport::finish_send(&endpoint, &connection).await;
            Ok(receipt)
        }
        .await;
        if outcome.is_err() {
            transport::reject(&endpoint, &connection).await;
        }
        outcome
    })
    .await
    .map_err(|_| Error::Timeout)?
}

pub(crate) async fn info(file: &mut File) -> Result<FileInfo> {
    let bytes = file.metadata().await?.len();
    if bytes > config::MAX_FILE_BYTES {
        return Err(Error::FileTooLarge);
    }
    let sha256 = digest(file, bytes).await?;
    if file.metadata().await?.len() != bytes {
        return Err(Error::Integrity);
    }
    Ok(FileInfo { bytes, sha256 })
}

async fn digest(file: &mut File, bytes: u64) -> Result<[u8; 32]> {
    file.seek(std::io::SeekFrom::Start(0)).await?;
    let mut hash = Sha256::new();
    let mut remaining = bytes;
    let mut buffer = Zeroizing::new(vec![0; config::FILE_CHUNK_BYTES]);
    while remaining > 0 {
        let count = remaining.min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..count])
            .await
            .map_err(|_| Error::Integrity)?;
        hash.update(&buffer[..count]);
        remaining -= count as u64;
    }
    Ok(hash.finalize().into())
}

#[cfg(unix)]
pub(crate) fn source_file(path: &Path) -> Result<File> {
    use rustix::fs::{self, Mode, OFlags};
    let file = std::fs::File::from(fs::open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    if !file.metadata()?.is_file() {
        return Err(Error::Configuration("source must be a regular file"));
    }
    Ok(File::from_std(file))
}

#[cfg(not(unix))]
pub(crate) fn source_file(_path: &Path) -> Result<File> {
    Err(Error::UnsupportedPlatform)
}

#[cfg(not(target_os = "linux"))]
struct Store {
    file: File,
    complete: bool,
}
#[cfg(not(target_os = "linux"))]
impl Store {
    async fn open(_destination: &Path, _binding: &[u8; 73]) -> Result<Self> {
        Err(Error::UnsupportedPlatform)
    }
    async fn commit(&mut self) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
