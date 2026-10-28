//! One owner for pinned mutual TLS, QUIC bounds and connection lifetime.
mod lifetime;
pub(crate) use lifetime::{DriverOwner, server as owned_server};

use std::{net::SocketAddr, sync::Arc, time::Duration};

use quinn::{
    Endpoint,
    crypto::rustls::{QuicClientConfig, QuicServerConfig},
};

use crate::{Error, Identity, Peer, Result, config, error::connection_error};

#[derive(Clone, Copy)]
pub(crate) enum Policy {
    OneShot,
    #[cfg(unix)]
    Interactive,
    Room,
}

pub(crate) fn server(
    address: SocketAddr,
    identity: &Identity,
    peer: &Peer,
    alpn: &[u8],
) -> Result<Endpoint> {
    Ok(Endpoint::server(
        server_config(
            identity,
            std::slice::from_ref(peer),
            &[alpn],
            Policy::OneShot,
        )?,
        address,
    )?)
}

pub(crate) fn server_config(
    identity: &Identity,
    peers: &[Peer],
    protocols: &[&[u8]],
    policy: Policy,
) -> Result<quinn::ServerConfig> {
    if peers.is_empty() || peers.len() > 64 {
        return Err(Error::Configuration(
            "TLS needs a bounded nonempty set of admitted peers",
        ));
    }
    let mut roots = rustls::RootCertStore::empty();
    for peer in peers {
        roots
            .add(peer.certificate.clone())
            .map_err(|_| Error::Authentication)?;
    }
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
        roots.into(),
        rustls::crypto::ring::default_provider().into(),
    )
    .build()
    .map_err(|_| Error::Authentication)?;
    let mut crypto = rustls::ServerConfig::builder_with_provider(
        rustls::crypto::ring::default_provider().into(),
    )
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(|_| Error::Configuration("TLS 1.3 unavailable"))?
    .with_client_cert_verifier(verifier)
    .with_single_cert(
        vec![identity.public.certificate.clone()],
        identity.key.clone_key(),
    )
    .map_err(|_| Error::Identity("invalid local TLS identity"))?;
    crypto.alpn_protocols = protocols.iter().map(|value| value.to_vec()).collect();
    crypto.max_early_data_size = 0;
    let crypto = QuicServerConfig::try_from(crypto)
        .map_err(|_| Error::Configuration("QUIC TLS unavailable"))?;
    let mut server = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    server.transport_config(settings(policy));
    server
        .max_incoming(if matches!(policy, Policy::OneShot) {
            1
        } else {
            config::NODE_MAX_CONNECTIONS
        })
        .incoming_buffer_size(config::WINDOW_BYTES as u64)
        .incoming_buffer_size_total(config::WINDOW_BYTES as u64);
    Ok(server)
}

pub(crate) fn client(
    destination: SocketAddr,
    identity: &Identity,
    peer: &Peer,
    alpn: &[u8],
) -> Result<Endpoint> {
    let client = client_config(identity, peer, alpn, Policy::OneShot)?;
    let bind = if destination.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let mut endpoint = Endpoint::client(bind.parse().expect("constant IP address"))?;
    endpoint.set_default_client_config(client);
    Ok(endpoint)
}

pub(crate) fn client_config(
    identity: &Identity,
    peer: &Peer,
    alpn: &[u8],
    policy: Policy,
) -> Result<quinn::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(peer.certificate.clone())
        .map_err(|_| Error::Authentication)?;
    let mut crypto = rustls::ClientConfig::builder_with_provider(
        rustls::crypto::ring::default_provider().into(),
    )
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(|_| Error::Configuration("TLS 1.3 unavailable"))?
    .with_root_certificates(roots)
    .with_client_auth_cert(
        vec![identity.public.certificate.clone()],
        identity.key.clone_key(),
    )
    .map_err(|_| Error::Identity("invalid local TLS identity"))?;
    crypto.alpn_protocols = vec![alpn.to_vec()];
    crypto.enable_early_data = false;
    let mut client = quinn::ClientConfig::new(Arc::new(
        QuicClientConfig::try_from(crypto)
            .map_err(|_| Error::Configuration("QUIC TLS unavailable"))?,
    ));
    client.transport_config(settings(policy));
    Ok(client)
}

pub(crate) async fn accept(endpoint: &Endpoint, peer: &Peer) -> Result<quinn::Connection> {
    let incoming = endpoint.accept().await.ok_or(Error::Network)?;
    endpoint.set_server_config(None);
    let connection = incoming.await.map_err(connection_error)?;
    verify_peer(&connection, peer)?;
    Ok(connection)
}

pub(crate) fn verify_peer(connection: &quinn::Connection, expected: &Peer) -> Result<()> {
    if peer_identity(connection)?.certificate != expected.certificate {
        connection.close(1u32.into(), b"");
        return Err(Error::Authentication);
    }
    Ok(())
}

pub(crate) fn peer_identity(connection: &quinn::Connection) -> Result<Peer> {
    let identity = connection.peer_identity().ok_or(Error::Authentication)?;
    let certificates = identity
        .downcast_ref::<Vec<rustls::pki_types::CertificateDer<'static>>>()
        .ok_or(Error::Authentication)?;
    Peer::from_der(certificates.first().ok_or(Error::Authentication)?.to_vec())
}

pub(crate) fn protocol(connection: &quinn::Connection) -> Result<Vec<u8>> {
    let data = connection.handshake_data().ok_or(Error::Protocol)?;
    let data = data
        .downcast_ref::<quinn::crypto::rustls::HandshakeData>()
        .ok_or(Error::Protocol)?;
    data.protocol.clone().ok_or(Error::Protocol)
}

pub(crate) async fn wait_closed(connection: &quinn::Connection) -> Result<()> {
    match connection.closed().await {
        quinn::ConnectionError::ApplicationClosed(close) if close.error_code == 0u32.into() => {
            Ok(())
        }
        error => Err(connection_error(error)),
    }
}

pub(crate) async fn finish_send(endpoint: &Endpoint, connection: &quinn::Connection) {
    connection.close(0u32.into(), b"");
    endpoint.wait_idle().await;
}

pub(crate) async fn reject(endpoint: &Endpoint, connection: &quinn::Connection) {
    // Let the close frame leave the process before returning a local refusal.
    connection.close(1u32.into(), b"");
    endpoint.wait_idle().await;
}

pub(crate) fn validate_duration(duration: Duration, maximum: Duration) -> Result<()> {
    if duration.is_zero() || duration > maximum {
        return Err(Error::Configuration(
            "operation duration is outside the allowed range",
        ));
    }
    Ok(())
}

fn settings(policy: Policy) -> Arc<quinn::TransportConfig> {
    let streams = if matches!(policy, Policy::OneShot) {
        1
    } else {
        config::NODE_MAX_STREAMS as u32
    };
    let window = if matches!(policy, Policy::OneShot) {
        config::WINDOW_BYTES
    } else {
        config::WINDOW_BYTES * 4
    };
    let mut transport = quinn::TransportConfig::default();
    transport
        .max_concurrent_bidi_streams(streams.into())
        .max_concurrent_uni_streams(0u8.into())
        .datagram_receive_buffer_size(if matches!(policy, Policy::Room) {
            Some(config::MEDIA_DATAGRAM_BUFFER_BYTES)
        } else {
            None
        })
        .datagram_send_buffer_size(config::MEDIA_DATAGRAM_BUFFER_BYTES)
        .stream_receive_window(config::WINDOW_BYTES.into())
        .receive_window(window.into())
        .send_window(window as u64)
        .crypto_buffer_size(config::MAX_IDENTITY_BYTES * 2)
        .max_idle_timeout(Some(
            config::MAX_TIMEOUT.try_into().expect("bounded duration"),
        ));
    Arc::new(transport)
}
