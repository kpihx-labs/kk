//! Bounded bootstrap only. Payload traffic never uses this unauthenticated TCP listener.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{future::Future, net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use super::{Channel, ClientExchange, Invitation};
use crate::{
    Error, Identity, Peer, Result, config,
    contacts::{Contact, Permissions},
    transport,
};

const MAGIC: &[u8; 8] = b"KKPAIR01";
const MAX_FRAME: usize = 16_400;
const PROOF_ALPN: &[u8] = b"kk-pair-proof/1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    certificate: Vec<u8>,
    grants: Permissions,
    data_port: u16,
}

#[derive(Debug, Clone)]
pub struct Proposal {
    pub fingerprint: String,
    pub our_grants: Permissions,
    pub their_grants: Permissions,
    pub endpoint: SocketAddr,
    pub confirmation: String,
}

pub struct PairServer {
    listener: TcpListener,
    invitation: Invitation,
}
impl PairServer {
    pub async fn bind(address: SocketAddr, ttl: Duration) -> Result<Self> {
        let listener = TcpListener::bind(address).await?;
        let invitation = Invitation::generate(ttl)?;
        Ok(Self {
            listener,
            invitation,
        })
    }
    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }
    pub fn id(&self) -> [u8; 16] {
        self.invitation.id()
    }
    pub fn code(&self) -> Result<&age::secrecy::SecretString> {
        self.invitation.code()
    }

    pub async fn run<A, F, C>(
        mut self,
        identity: &Identity,
        grants: Permissions,
        data_port: u16,
        approve: A,
        commit: C,
    ) -> Result<Proposal>
    where
        A: FnOnce(Proposal) -> F,
        F: Future<Output = Result<bool>>,
        C: FnOnce(Contact) -> Result<()>,
    {
        let remaining = self.invitation.remaining()?;
        tokio::time::timeout(remaining, async {
            let (mut stream, address) = self.listener.accept().await?;
            drop(self.listener); // No second connection or per-IP guess budget.
            stream.set_nodelay(true)?;
            stream.write_all(MAGIC).await?;
            stream.write_all(&self.invitation.id()).await?;
            let request = read(&mut stream).await?;
            let (state, response) = self.invitation.start(&request)?;
            write(&mut stream, &response).await?;
            let mut channel = state.finish(&read(&mut stream).await?)?;
            let own = document(identity, grants, data_port)?;
            send(&mut stream, &mut channel, &own).await?;
            let other = receive(&mut stream, &mut channel).await?;
            let (peer, mut offer, contact) = proposal(identity, grants, &other, address)?;
            let binding = binding(self.invitation.id(), &own, &other);
            offer.confirmation = confirmation(&binding);
            if !approve(offer.clone()).await? {
                return Err(Error::Denied);
            }
            confirm(&mut stream, &mut channel, &binding).await?;
            check_confirmation(&mut stream, &mut channel, &binding).await?;
            let proof_address = SocketAddr::new(stream.local_addr()?.ip(), 0);
            let endpoint = transport::server(proof_address, identity, &peer, PROOF_ALPN)?;
            send(
                &mut stream,
                &mut channel,
                &endpoint.local_addr()?.port().to_be_bytes(),
            )
            .await?;
            let connection = transport::accept(&endpoint, &peer).await?;
            let mut exporter = [0; 32];
            connection
                .export_keying_material(&mut exporter, b"EXPORTER-KK-PAIR-1", &binding)
                .map_err(|_| Error::Pairing)?;
            let (mut response, mut request) =
                connection.accept_bi().await.map_err(|_| Error::Pairing)?;
            if request.read_to_end(32).await? != exporter {
                return Err(Error::Pairing);
            }
            response.write_all(&exporter).await?;
            response.finish().map_err(|_| Error::Pairing)?;
            transport::wait_closed(&connection).await?;
            commit(contact)?;
            send(&mut stream, &mut channel, b"stored").await?;
            if receive(&mut stream, &mut channel).await?.as_slice() != b"stored" {
                return Err(Error::Pairing);
            }
            Ok(offer)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
}

pub struct JoinOptions {
    pub address: SocketAddr,
    pub expected_id: Option<[u8; 16]>,
    pub expected_fingerprint: Option<String>,
    pub code: age::secrecy::SecretString,
    pub grants: Permissions,
    pub data_port: u16,
    pub deadline: Duration,
}

pub async fn join<A, F, C>(
    options: JoinOptions,
    identity: &Identity,
    approve: A,
    commit: C,
) -> Result<Proposal>
where
    A: FnOnce(Proposal) -> F,
    F: Future<Output = Result<bool>>,
    C: FnOnce(Contact) -> Result<()>,
{
    let JoinOptions {
        address,
        expected_id,
        expected_fingerprint,
        code,
        grants,
        data_port,
        deadline,
    } = options;
    transport::validate_duration(deadline, Duration::from_secs(300))?;
    tokio::time::timeout(deadline, async {
        let mut stream = TcpStream::connect(address).await?;
        stream.set_nodelay(true)?;
        let mut magic = [0; 8];
        let mut id = [0; 16];
        stream.read_exact(&mut magic).await?;
        stream.read_exact(&mut id).await?;
        if &magic != MAGIC || expected_id.is_some_and(|expected| expected != id) {
            return Err(Error::Pairing);
        }
        let (client, request) = ClientExchange::start(id, code)?;
        write(&mut stream, &request).await?;
        let (mut channel, finish) = client.finish(&read(&mut stream).await?)?;
        write(&mut stream, &finish).await?;
        let other = receive(&mut stream, &mut channel).await?;
        let own = document(identity, grants, data_port)?;
        let (peer, mut offer, contact) = proposal(identity, grants, &other, address)?;
        if expected_fingerprint
            .as_ref()
            .is_some_and(|expected| expected != &offer.fingerprint)
        {
            return Err(Error::Authentication);
        }
        send(&mut stream, &mut channel, &own).await?;
        let binding = binding(id, &other, &own);
        offer.confirmation = confirmation(&binding);
        if !approve(offer.clone()).await? {
            return Err(Error::Denied);
        }
        check_confirmation(&mut stream, &mut channel, &binding).await?;
        confirm(&mut stream, &mut channel, &binding).await?;
        let port = receive(&mut stream, &mut channel).await?;
        let port = u16::from_be_bytes(port.as_slice().try_into().map_err(|_| Error::Pairing)?);
        if port == 0 {
            return Err(Error::Pairing);
        }
        let proof_address = SocketAddr::new(address.ip(), port);
        let endpoint = transport::client(proof_address, identity, &peer, PROOF_ALPN)?;
        let connection = endpoint
            .connect(proof_address, config::TLS_NAME)
            .map_err(|_| Error::Pairing)?
            .await
            .map_err(|_| Error::Pairing)?;
        transport::verify_peer(&connection, &peer)?;
        let mut exporter = [0; 32];
        connection
            .export_keying_material(&mut exporter, b"EXPORTER-KK-PAIR-1", &binding)
            .map_err(|_| Error::Pairing)?;
        let (mut request, mut response) = connection.open_bi().await.map_err(|_| Error::Pairing)?;
        request.write_all(&exporter).await?;
        request.finish().map_err(|_| Error::Pairing)?;
        if response.read_to_end(32).await? != exporter {
            return Err(Error::Pairing);
        }
        transport::finish_send(&endpoint, &connection).await;
        if receive(&mut stream, &mut channel).await?.as_slice() != b"stored" {
            return Err(Error::Pairing);
        }
        commit(contact)?;
        send(&mut stream, &mut channel, b"stored").await?;
        stream.shutdown().await?;
        Ok(offer)
    })
    .await
    .map_err(|_| Error::Timeout)?
}

fn document(identity: &Identity, grants: Permissions, data_port: u16) -> Result<Vec<u8>> {
    if data_port == 0 {
        return Err(Error::Configuration("data port must be nonzero"));
    }
    serde_json::to_vec(&Document {
        version: 1,
        certificate: identity.peer().certificate.to_vec(),
        grants,
        data_port,
    })
    .map_err(|_| Error::Pairing)
}
fn proposal(
    identity: &Identity,
    grants: Permissions,
    bytes: &[u8],
    address: SocketAddr,
) -> Result<(Peer, Proposal, Contact)> {
    let other: Document = serde_json::from_slice(bytes).map_err(|_| Error::Pairing)?;
    if other.version != 1 || other.data_port == 0 {
        return Err(Error::Pairing);
    }
    let peer = Peer::from_der(other.certificate.clone())?;
    let local = identity.peer().fingerprint();
    let fingerprint = peer.fingerprint();
    if local == fingerprint {
        return Err(Error::Identity(
            "pairing requires distinct device identities",
        ));
    }
    let endpoint = SocketAddr::new(address.ip(), other.data_port);
    let offer = Proposal {
        fingerprint,
        our_grants: grants,
        their_grants: other.grants,
        endpoint,
        confirmation: String::new(),
    };
    let contact = Contact {
        certificate: other.certificate,
        local_identity: local,
        incoming: grants,
        outgoing: other.grants,
        endpoints: vec![endpoint],
    };
    contact.validate()?;
    Ok((peer, offer, contact))
}
fn binding(id: [u8; 16], server: &[u8], client: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(MAGIC);
    hash.update(id);
    for bytes in [server, client] {
        hash.update((bytes.len() as u32).to_be_bytes());
        hash.update(bytes);
    }
    hash.finalize().into()
}
fn confirmation(binding: &[u8; 32]) -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(12);
    for byte in &binding[..6] {
        write!(output, "{byte:02x}").expect("String formatting");
    }
    output
}
async fn write(stream: &mut TcpStream, bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_FRAME {
        return Err(Error::Pairing);
    }
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .await?;
    stream.write_all(bytes).await?;
    Ok(())
}
async fn read(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let length = stream.read_u32().await? as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(Error::Pairing);
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await?;
    Ok(bytes)
}
async fn send(stream: &mut TcpStream, channel: &mut Channel, bytes: &[u8]) -> Result<()> {
    write(stream, &channel.seal(bytes)?).await
}
async fn receive(
    stream: &mut TcpStream,
    channel: &mut Channel,
) -> Result<zeroize::Zeroizing<Vec<u8>>> {
    channel.open(&read(stream).await?)
}
async fn confirm(stream: &mut TcpStream, channel: &mut Channel, binding: &[u8; 32]) -> Result<()> {
    send(stream, channel, binding).await
}
async fn check_confirmation(
    stream: &mut TcpStream,
    channel: &mut Channel,
    binding: &[u8; 32],
) -> Result<()> {
    if receive(stream, channel).await?.as_slice() != binding {
        return Err(Error::Pairing);
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn a_code_holder_cannot_register_a_certificate_without_its_private_key() {
        let root = tempfile::tempdir().unwrap();
        let a = Identity::create(&root.path().join("a")).unwrap();
        let attacker = Identity::create(&root.path().join("attacker")).unwrap();
        let victim = Identity::create(&root.path().join("victim")).unwrap();
        let server = PairServer::bind("127.0.0.1:0".parse().unwrap(), Duration::from_secs(5))
            .await
            .unwrap();
        let address = server.local_addr().unwrap();
        let code = server.code().unwrap().clone();
        let stores = AtomicUsize::new(0);
        let attack = async {
            let mut stream = TcpStream::connect(address).await.unwrap();
            let mut magic = [0; 8];
            let mut id = [0; 16];
            stream.read_exact(&mut magic).await.unwrap();
            stream.read_exact(&mut id).await.unwrap();
            let (client, request) = ClientExchange::start(id, code).unwrap();
            write(&mut stream, &request).await.unwrap();
            let (mut channel, finish) = client.finish(&read(&mut stream).await.unwrap()).unwrap();
            write(&mut stream, &finish).await.unwrap();
            let other = receive(&mut stream, &mut channel).await.unwrap();
            // Claim the third identity, but possess only the attacker's TLS key.
            let own = document(&victim, Permissions::default(), 45454).unwrap();
            send(&mut stream, &mut channel, &own).await.unwrap();
            let binding = binding(id, &other, &own);
            check_confirmation(&mut stream, &mut channel, &binding)
                .await
                .unwrap();
            confirm(&mut stream, &mut channel, &binding).await.unwrap();
            let port = receive(&mut stream, &mut channel).await.unwrap();
            let port = u16::from_be_bytes(port.as_slice().try_into().unwrap());
            let destination = SocketAddr::new(address.ip(), port);
            let endpoint =
                transport::client(destination, &attacker, &a.peer(), PROOF_ALPN).unwrap();
            if let Ok(connection) = endpoint
                .connect(destination, config::TLS_NAME)
                .unwrap()
                .await
                && let Ok((mut request, mut response)) = connection.open_bi().await
            {
                let mut exporter = [0; 32];
                connection
                    .export_keying_material(&mut exporter, b"EXPORTER-KK-PAIR-1", &binding)
                    .unwrap();
                let _ = request.write_all(&exporter).await;
                let _ = request.finish();
                let answer = response.read_to_end(32).await;
                assert!(answer.is_err() || answer.unwrap().is_empty());
            }
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(7), async {
            tokio::join!(
                server.run(
                    &a,
                    Permissions::default(),
                    45454,
                    |_| async { Ok(true) },
                    |_| {
                        stores.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                ),
                attack
            )
        })
        .await
        .unwrap();
        assert!(result.is_err());
        assert_eq!(stores.load(Ordering::SeqCst), 0);
    }
}
