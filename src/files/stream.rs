//! The file wire protocol, independent of one-shot or persistent connection lifetime.

use super::progress::{TransferObserver, TransferPhase};
use super::{FileInfo, Receipt, ReceivePolicy, Store, digest};
use crate::{Error, Peer, Result, config};
use sha2::{Digest, Sha256};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
};
use zeroize::Zeroizing;

/// Serialize the atomic publication step with the owner's revocation boundary.
/// This runs on a blocking worker; implementations must not await or retain the
/// publication closure. Preparation and durability work stay outside the gate.
pub(crate) trait Publication: Send + 'static {
    fn publish(self, rename: impl FnOnce() -> Result<()>) -> Result<()>;
}

impl Publication for () {
    fn publish(self, rename: impl FnOnce() -> Result<()>) -> Result<()> {
        rename()
    }
}

pub(crate) async fn receive(
    peer: &Peer,
    policy: &ReceivePolicy,
    request: quinn::RecvStream,
    response: quinn::SendStream,
    authorize: impl Fn() -> Result<()>,
) -> Result<Receipt> {
    receive_with(peer, request, response, authorize, |_| Ok(policy.clone())).await
}

pub(crate) async fn receive_with(
    peer: &Peer,
    request: quinn::RecvStream,
    response: quinn::SendStream,
    authorize: impl Fn() -> Result<()>,
    choose: impl FnOnce(&FileInfo) -> Result<ReceivePolicy>,
) -> Result<Receipt> {
    receive_with_async(
        peer,
        request,
        response,
        || std::future::ready(authorize()),
        choose,
    )
    .await
}

pub(crate) async fn receive_with_async<F, Fut>(
    peer: &Peer,
    request: quinn::RecvStream,
    response: quinn::SendStream,
    authorize: F,
    choose: impl FnOnce(&FileInfo) -> Result<ReceivePolicy>,
) -> Result<Receipt>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    receive_with_publication(peer, request, response, authorize, choose, ()).await
}

pub(crate) async fn receive_with_publication<F, Fut>(
    peer: &Peer,
    request: quinn::RecvStream,
    response: quinn::SendStream,
    authorize: F,
    choose: impl FnOnce(&FileInfo) -> Result<ReceivePolicy>,
    publication: impl Publication,
) -> Result<Receipt>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    receive_with_publication_monitored(
        peer,
        request,
        response,
        authorize,
        choose,
        publication,
        None,
    )
    .await
}

pub(crate) async fn receive_with_publication_monitored<F, Fut>(
    peer: &Peer,
    mut request: quinn::RecvStream,
    mut response: quinn::SendStream,
    mut authorize: F,
    choose: impl FnOnce(&FileInfo) -> Result<ReceivePolicy>,
    publication: impl Publication,
    progress: Option<&TransferObserver>,
) -> Result<Receipt>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    authorize().await?;
    if let Some(progress) = progress {
        progress.handoff();
    }
    let mut offer = [0; 41];
    request.read_exact(&mut offer).await?;
    if offer[0] > 1 {
        return Err(Error::Protocol);
    }
    let sensitive = offer[0] == 1;
    let bytes = u64::from_be_bytes(offer[1..9].try_into().expect("fixed header"));
    if bytes > config::MAX_FILE_BYTES {
        return Err(Error::FileTooLarge);
    }
    if let Some(progress) = progress {
        progress.offered(bytes);
    }
    let expected: [u8; 32] = offer[9..].try_into().expect("fixed digest");
    let policy = choose(&FileInfo {
        bytes,
        sha256: expected,
    })?;
    if bytes > policy.max_bytes {
        return Err(Error::FileTooLarge);
    }
    let mut binding = [0; 73];
    binding[..41].copy_from_slice(&offer);
    binding[41..].copy_from_slice(&Sha256::digest(peer.certificate.as_ref()));
    authorize().await?;
    let mut store = Store::open(&policy.destination, &binding).await?;
    let offset = store.file.metadata().await?.len();
    if offset > bytes {
        return Err(Error::Integrity);
    }
    let prefix = digest(&mut store.file, offset).await?;
    if store.complete && (offset != bytes || prefix != expected) {
        return Err(Error::Destination(
            "existing destination differs from offered content",
        ));
    }
    response.write_all(&offset.to_be_bytes()).await?;
    response.write_all(&prefix).await?;
    if let Some(progress) = progress {
        progress.resumed(offset);
    }
    let mut remaining = bytes - offset;
    let mut since_authorization = 0usize;
    let mut buffer = Zeroizing::new(vec![0; config::FILE_CHUNK_BYTES]);
    while remaining > 0 {
        let count = remaining.min(buffer.len() as u64) as usize;
        let read = request
            .read(&mut buffer[..count])
            .await?
            .ok_or(Error::Protocol)?;
        if read == 0 || store.complete {
            return Err(Error::Protocol);
        }
        store.file.write_all(&buffer[..read]).await?;
        if let Some(progress) = progress {
            progress.advance(read as u64);
        }
        since_authorization += read;
        if since_authorization >= config::FILE_CHUNK_BYTES {
            authorize().await?;
            since_authorization = 0;
        }
        remaining -= read as u64;
    }
    if request.read(&mut [0]).await?.is_some() {
        return Err(Error::Protocol);
    }
    if !store.complete {
        store.file.flush().await?;
    }
    if let Some(progress) = progress {
        progress.phase(TransferPhase::Verifying);
    }
    store.file.sync_all().await?;
    if digest(&mut store.file, bytes).await? != expected
        || store.file.metadata().await?.len() != bytes
    {
        return Err(Error::Integrity);
    }
    authorize().await?;
    if let Some(progress) = progress {
        progress.phase(TransferPhase::Publishing);
    }
    store.commit_with_publication(publication).await?;
    response.write_all(&expected).await?;
    response.finish().map_err(|_| Error::Network)?;
    Ok(Receipt {
        info: FileInfo {
            bytes,
            sha256: expected,
        },
        resumed_from: offset,
        sensitive,
    })
}

pub(crate) async fn send(
    file: &mut File,
    info: FileInfo,
    sensitive: bool,
    request: quinn::SendStream,
    response: quinn::RecvStream,
    authorize: impl Fn() -> Result<()>,
) -> Result<Receipt> {
    send_with_async(file, info, sensitive, request, response, || {
        std::future::ready(authorize())
    })
    .await
}

pub(crate) async fn send_with_async<F, Fut>(
    file: &mut File,
    info: FileInfo,
    sensitive: bool,
    request: quinn::SendStream,
    response: quinn::RecvStream,
    authorize: F,
) -> Result<Receipt>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    send_with_async_monitored(file, info, sensitive, request, response, authorize, None).await
}

pub(crate) async fn send_with_async_monitored<F, Fut>(
    file: &mut File,
    info: FileInfo,
    sensitive: bool,
    mut request: quinn::SendStream,
    mut response: quinn::RecvStream,
    mut authorize: F,
    progress: Option<&TransferObserver>,
) -> Result<Receipt>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    authorize().await?;
    if let Some(progress) = progress {
        progress.offered(info.bytes);
        progress.handoff();
    }
    request.write_all(&[u8::from(sensitive)]).await?;
    request.write_all(&info.bytes.to_be_bytes()).await?;
    request.write_all(&info.sha256).await?;
    let mut proposal = [0; 40];
    response.read_exact(&mut proposal).await?;
    let offset = u64::from_be_bytes(proposal[..8].try_into().expect("fixed header"));
    if offset > info.bytes || digest(file, offset).await?.as_slice() != &proposal[8..] {
        return Err(Error::Integrity);
    }
    if let Some(progress) = progress {
        progress.resumed(offset);
    }
    let upload = async {
        let mut remaining = info.bytes - offset;
        let mut buffer = Zeroizing::new(vec![0; config::FILE_CHUNK_BYTES]);
        while remaining > 0 {
            authorize().await?;
            let count = remaining.min(buffer.len() as u64) as usize;
            file.read_exact(&mut buffer[..count])
                .await
                .map_err(|_| Error::Integrity)?;
            request.write_all(&buffer[..count]).await?;
            if let Some(progress) = progress {
                progress.advance(count as u64);
            }
            remaining -= count as u64;
        }
        if file.read(&mut [0]).await? != 0 {
            return Err(Error::Integrity);
        }
        authorize().await?;
        request.finish().map_err(|_| Error::Network)?;
        if let Some(progress) = progress {
            progress.phase(TransferPhase::AwaitingReceipt);
        }
        Ok::<_, Error>(())
    };
    let receipt = async {
        if response.read_to_end(32).await? != info.sha256 {
            return Err(Error::Integrity);
        }
        Ok(())
    };
    // A recipient can reject while our upload is flow-control blocked. Observe
    // its bounded response concurrently; success still requires both full upload
    // (including final authorization/FIN) and the exact receipt, never either alone.
    tokio::try_join!(upload, receipt)?;
    Ok(Receipt {
        info,
        resumed_from: offset,
        sensitive,
    })
}
