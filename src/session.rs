use std::{net::SocketAddr, time::Duration};

use quinn::Endpoint;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{Error, Identity, Peer, Result, config, error::connection_error, transport};

/// One incoming connection attempt. Dropping it closes its owned UDP endpoint.
pub struct Listener {
    endpoint: Endpoint,
    peer: Peer,
}

/// Debug output deliberately excludes the content, even in ordinary mode.
pub struct ReceivedText {
    text: Zeroizing<String>,
    pub sensitive: bool,
}
impl ReceivedText {
    pub(crate) fn from_bytes(bytes: &[u8], sensitive: bool) -> Result<Self> {
        if bytes.len() > config::MAX_MESSAGE_BYTES {
            return Err(Error::MessageTooLarge);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| Error::Protocol)?;
        Ok(Self {
            text: Zeroizing::new(text.to_owned()),
            sensitive,
        })
    }
    pub fn text(&self) -> &str {
        &self.text
    }
}
impl std::fmt::Debug for ReceivedText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReceivedText")
            .field("bytes", &self.text.len())
            .field("sensitive", &self.sensitive)
            .finish()
    }
}

impl Listener {
    pub fn bind(address: SocketAddr, identity: &Identity, peer: &Peer) -> Result<Self> {
        Ok(Self {
            endpoint: transport::server(address, identity, peer, config::ALPN)?,
            peer: peer.clone(),
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.endpoint.local_addr()?)
    }

    /// Receive one validated message. The deadline includes waiting for a peer.
    pub async fn receive(self, deadline: Duration) -> Result<ReceivedText> {
        self.receive_authorized(deadline, || Ok(())).await
    }

    pub async fn receive_authorized(
        self,
        deadline: Duration,
        authorize: impl Fn() -> Result<()>,
    ) -> Result<ReceivedText> {
        transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        tokio::time::timeout(deadline, async {
            let connection = transport::accept(&self.endpoint, &self.peer).await?;
            authorize()?;
            let (response, request) = connection.accept_bi().await.map_err(connection_error)?;
            let message = receive_stream(request, response, &authorize).await?;
            transport::wait_closed(&connection).await?;
            Ok(message)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
}

pub async fn send(
    destination: SocketAddr,
    identity: &Identity,
    peer: &Peer,
    text: &str,
    deadline: Duration,
) -> Result<()> {
    send_with_policy(destination, identity, peer, text, false, deadline).await
}

/// Ordinary and sensitive payloads use identical transport cryptography.
pub async fn send_with_policy(
    destination: SocketAddr,
    identity: &Identity,
    peer: &Peer,
    text: &str,
    sensitive: bool,
    deadline: Duration,
) -> Result<()> {
    send_authorized(
        destination,
        identity,
        peer,
        text,
        sensitive,
        deadline,
        || Ok(()),
    )
    .await
}

pub async fn send_authorized(
    destination: SocketAddr,
    identity: &Identity,
    peer: &Peer,
    text: &str,
    sensitive: bool,
    deadline: Duration,
    authorize: impl Fn() -> Result<()>,
) -> Result<()> {
    transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
    authorize()?;
    if text.len() > config::MAX_MESSAGE_BYTES {
        return Err(Error::MessageTooLarge);
    }
    let endpoint = transport::client(destination, identity, peer, config::ALPN)?;
    tokio::time::timeout(deadline, async {
        let connection = endpoint
            .connect(destination, config::TLS_NAME)
            .map_err(|_| Error::Configuration("invalid destination"))?
            .await
            .map_err(connection_error)?;
        transport::verify_peer(&connection, peer)?;
        authorize()?;
        let (request, response) = connection.open_bi().await.map_err(connection_error)?;
        send_stream(text, sensitive, request, response, &authorize).await?;
        transport::finish_send(&endpoint, &connection).await;
        Ok(())
    })
    .await
    .map_err(|_| Error::Timeout)?
}

/// Shared frame handler; connection ownership belongs to its caller.
pub(crate) async fn receive_stream(
    mut request: quinn::RecvStream,
    mut response: quinn::SendStream,
    authorize: impl Fn() -> Result<()>,
) -> Result<ReceivedText> {
    authorize()?;
    let mut flags = [0; 1];
    request.read_exact(&mut flags).await?;
    if flags[0] > 1 {
        return Err(Error::Protocol);
    }
    let mut header = [0; 4];
    request.read_exact(&mut header).await?;
    let size = u32::from_be_bytes(header) as usize;
    if size > config::MAX_MESSAGE_BYTES {
        return Err(Error::MessageTooLarge);
    }
    let mut bytes = Zeroizing::new(vec![0; size]);
    request.read_exact(&mut bytes).await?;
    if request.read(&mut [0]).await?.is_some() {
        return Err(Error::Protocol);
    }
    let message = ReceivedText::from_bytes(&bytes, flags[0] == 1)?;
    authorize()?;
    response
        .write_all(&Sha256::digest(message.text().as_bytes()))
        .await?;
    response.finish().map_err(|_| Error::Network)?;
    Ok(message)
}

pub(crate) async fn send_stream(
    text: &str,
    sensitive: bool,
    mut request: quinn::SendStream,
    mut response: quinn::RecvStream,
    authorize: impl Fn() -> Result<()>,
) -> Result<()> {
    authorize()?;
    if text.len() > config::MAX_MESSAGE_BYTES {
        return Err(Error::MessageTooLarge);
    }
    request.write_all(&[u8::from(sensitive)]).await?;
    request
        .write_all(&(text.len() as u32).to_be_bytes())
        .await?;
    request.write_all(text.as_bytes()).await?;
    authorize()?;
    request.finish().map_err(|_| Error::Network)?;
    if response.read_to_end(32).await? != Sha256::digest(text.as_bytes()).as_slice() {
        return Err(Error::Protocol);
    }
    Ok(())
}

#[cfg(all(test, unix))]
fn client_endpoint(destination: SocketAddr, identity: &Identity, peer: &Peer) -> Result<Endpoint> {
    transport::client(destination, identity, peer, config::ALPN)
}

#[cfg(all(test, unix))]
mod tests;
