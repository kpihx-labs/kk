//! Opt-in IPv4 DNS-SD. A candidate is deliberately not an authenticated Peer.

use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, SocketAddrV4, UdpSocket},
    time::Duration,
};

use mdns_sd::{DaemonEvent, IfKind, ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};

use crate::{Error, Identity, Result, config};

const SERVICE: &str = "_kk._udp.local.";
const PAIR_SERVICE: &str = "_kk-pair._tcp.local.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceKind {
    Messages,
    Files,
    Pairing,
}
impl ServiceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Messages => "message",
            Self::Files => "file",
            Self::Pairing => "pair",
        }
    }
    fn service(self) -> &'static str {
        if self == Self::Pairing {
            PAIR_SERVICE
        } else {
            SERVICE
        }
    }
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub kind: ServiceKind,
    pub name: String,
    pub addresses: Vec<SocketAddrV4>,
    pub claimed_fingerprint: String,
}

/// Owns the discovery worker, not a permanent system service.
pub struct Announcement {
    daemon: ServiceDaemon,
    fullname: Option<String>,
    events: mdns_sd::Receiver<DaemonEvent>,
}

impl Announcement {
    fn on_interface(interface: Ipv4Addr) -> Result<Self> {
        if interface.is_unspecified() || interface.is_multicast() || interface.is_broadcast() {
            return Err(Error::Configuration(
                "discovery needs a concrete local IPv4 address",
            ));
        }
        let _local = UdpSocket::bind((interface, 0))
            .map_err(|_| Error::Configuration("discovery interface is not a local address"))?;
        let daemon = ServiceDaemon::new().map_err(|_| Error::Discovery("cannot start worker"))?;
        let events = match daemon.monitor() {
            Ok(events) => events,
            Err(_) => {
                let _ = daemon.shutdown();
                return Err(Error::Discovery("cannot monitor worker"));
            }
        };
        let worker = Self {
            daemon,
            fullname: None,
            events,
        };
        worker
            .daemon
            .disable_interface(IfKind::All)
            .map_err(|_| Error::Discovery("interface selection failed"))?;
        worker
            .daemon
            .enable_interface(IfKind::Addr(interface.into()))
            .map_err(|_| Error::Discovery("interface selection failed"))?;
        Ok(worker)
    }

    pub fn new(
        name: &str,
        address: SocketAddrV4,
        identity: &Identity,
        kind: ServiceKind,
    ) -> Result<Self> {
        if name.is_empty()
            || name.len() > 63
            || name.chars().any(char::is_control)
            || address.port() == 0
        {
            return Err(Error::Configuration("invalid discovery label or port"));
        }
        let mut worker = Self::on_interface(*address.ip())?;
        let fingerprint = identity.peer().fingerprint();
        let hostname = format!("kk-{}.local.", &fingerprint[..16]);
        let properties = [
            ("v", "1"),
            ("kind", kind.as_str()),
            ("fp", fingerprint.as_str()),
        ];
        let service = ServiceInfo::new(
            kind.service(),
            name,
            &hostname,
            std::net::IpAddr::V4(*address.ip()),
            address.port(),
            &properties[..],
        )
        .map_err(|_| Error::Configuration("invalid DNS-SD service"))?;
        worker.fullname = Some(service.get_fullname().to_owned());
        worker
            .daemon
            .register(service)
            .map_err(|_| Error::Discovery("registration failed"))?;
        Ok(worker)
    }

    /// Run alongside the listener so asynchronous socket failures cannot look like success.
    pub async fn failure(&self) -> Error {
        loop {
            match self.events.recv_async().await {
                Ok(DaemonEvent::Error(_)) | Err(_) => {
                    return Error::Discovery("worker or multicast socket failed");
                }
                _ => {}
            }
        }
    }
}

impl Drop for Announcement {
    fn drop(&mut self) {
        // Drop cannot return errors. Normal operation monitors errors; cancellation
        // still requests withdrawal/shutdown rather than leaving a resident daemon.
        if let Some(fullname) = &self.fullname {
            let _ = self.daemon.unregister(fullname);
        }
        let _ = self.daemon.shutdown();
    }
}

pub async fn discover(interface: Ipv4Addr, duration: Duration) -> Result<Vec<Candidate>> {
    if duration.is_zero() || duration > config::MAX_DISCOVERY_TIME {
        return Err(Error::Configuration(
            "discovery duration must be positive and at most 15 seconds",
        ));
    }
    let worker = Announcement::on_interface(interface)?;
    let receiver = worker
        .daemon
        .browse(SERVICE)
        .map_err(|_| Error::Discovery("browse failed"))?;
    let pairing = worker
        .daemon
        .browse(PAIR_SERVICE)
        .map_err(|_| Error::Discovery("pair browse failed"))?;
    let deadline = tokio::time::sleep(duration);
    tokio::pin!(deadline);
    let mut found = BTreeMap::new();
    loop {
        let event = tokio::select! {
            _ = &mut deadline => break,
            error = worker.failure() => return Err(error),
            event = receiver.recv_async() => event,
            event = pairing.recv_async() => event,
        };
        match event {
            Ok(ServiceEvent::ServiceResolved(service)) => {
                if let Some(candidate) = candidate(&service) {
                    if !found.contains_key(&service.fullname)
                        && found.len() == config::MAX_DISCOVERED_PEERS
                    {
                        return Err(Error::Discovery("result limit reached"));
                    }
                    found.insert(service.fullname.clone(), candidate);
                }
            }
            Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                found.remove(&fullname);
            }
            Err(_) => return Err(Error::Discovery("browse subscription closed")),
            _ => {}
        }
    }
    Ok(found.into_values().collect())
}

fn candidate(service: &ResolvedService) -> Option<Candidate> {
    let kind = match service.get_property_val_str("kind")? {
        "message" => ServiceKind::Messages,
        "file" => ServiceKind::Files,
        "pair" => ServiceKind::Pairing,
        _ => return None,
    };
    let name = service
        .fullname
        .strip_suffix(kind.service())?
        .strip_suffix('.')?;
    let fingerprint = service.get_property_val_str("fp")?;
    let addresses = service.get_addresses_v4();
    if name.is_empty()
        || name.len() > 63
        || name.chars().any(char::is_control)
        || service.get_property_val_str("v") != Some("1")
        || service.port == 0
        || fingerprint.len() != 64
        || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
        || addresses.is_empty()
        || addresses.len() > 8
    {
        return None;
    }
    let mut addresses: Vec<_> = addresses
        .into_iter()
        .filter(|address| {
            !address.is_unspecified() && !address.is_multicast() && !address.is_broadcast()
        })
        .map(|address| SocketAddrV4::new(address, service.port))
        .collect();
    addresses.sort();
    if addresses.is_empty() {
        return None;
    }
    Some(Candidate {
        kind,
        name: name.to_owned(),
        addresses,
        claimed_fingerprint: fingerprint.to_ascii_lowercase(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_or_unsupported_metadata_never_becomes_a_candidate() {
        for (version, fingerprint) in [
            ("2", "a".repeat(64)),
            ("1", "short".into()),
            ("1", "z".repeat(64)),
        ] {
            let service = ServiceInfo::new(
                SERVICE,
                "peer",
                "peer.local.",
                "192.0.2.1",
                1234,
                &[
                    ("v", version),
                    ("kind", "message"),
                    ("fp", fingerprint.as_str()),
                ][..],
            )
            .unwrap();
            assert!(candidate(&service.as_resolved_service()).is_none());
        }
    }

    #[test]
    fn a_valid_claim_is_only_unverified_metadata() {
        let service = ServiceInfo::new(
            SERVICE,
            "peer",
            "peer.local.",
            "192.0.2.1",
            1234,
            &[
                ("v", "1"),
                ("kind", "message"),
                (
                    "fp",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                ),
            ][..],
        )
        .unwrap();
        let found = candidate(&service.as_resolved_service()).unwrap();
        assert_eq!(found.name, "peer");
        assert_eq!(found.addresses[0].to_string(), "192.0.2.1:1234");
    }

    #[tokio::test]
    async fn invalid_scope_and_duration_are_rejected_before_browsing() {
        assert!(matches!(
            discover(Ipv4Addr::UNSPECIFIED, Duration::from_secs(1)).await,
            Err(Error::Configuration(_))
        ));
        assert!(matches!(
            discover(Ipv4Addr::LOCALHOST, Duration::ZERO).await,
            Err(Error::Configuration(_))
        ));
        assert!(matches!(
            discover(Ipv4Addr::LOCALHOST, Duration::from_secs(16)).await,
            Err(Error::Configuration(_))
        ));
    }
}
