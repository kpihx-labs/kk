//! OpenRaft control types. Peer IDs bind the entire certificate fingerprint, not
//! a truncated integer or network address. Transport must pin that exact key.
//! This is crash/partition-tolerant consensus plumbing, NOT Byzantine consensus.
use super::Proposal;
mod log;
mod machine;
mod node_intent;
mod records;
mod voters;
pub use records::{CommittedRecord, Position};
pub mod network;
pub mod service;
mod snapshot;
use crate::Peer;
pub use log::LogStore;
pub use machine::StateMachine;
use serde::{Deserialize, Serialize};
pub use snapshot::SnapshotBuffer;
use std::{fmt, net::SocketAddr};

#[derive(
    Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct PeerId(pub [u8; 32]);

// Synthetic identities are used only by the upstream storage conformance suite.
#[cfg(test)]
impl From<u64> for PeerId {
    fn from(value: u64) -> Self {
        let mut fingerprint = [0; 32];
        fingerprint[24..].copy_from_slice(&value.to_be_bytes());
        Self(fingerprint)
    }
}
impl PeerId {
    pub fn from_peer(peer: &Peer) -> Self {
        Self(peer.fingerprint_bytes())
    }
}
impl fmt::Display for PeerId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}
impl From<PeerId> for String {
    fn from(value: PeerId) -> Self {
        value.to_string()
    }
}
impl TryFrom<String> for PeerId {
    type Error = &'static str;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        if text.len() != 64
            || !text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("peer ID must be a canonical full certificate fingerprint");
        }
        let mut value = [0; 32];
        for (index, output) in value.iter_mut().enumerate() {
            *output = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
                .map_err(|_| "invalid peer ID")?;
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub endpoint: Option<SocketAddr>,
    /// Public scoped pin, not an authority claim. Empty is permitted only for
    /// low-level/preapproved storage fixtures; high-level writers fill it.
    #[serde(default)]
    pub certificate: Vec<u8>,
}
impl Node {
    pub fn for_peer(peer: &crate::Peer, endpoint: Option<SocketAddr>) -> Self {
        Self {
            endpoint,
            certificate: peer.certificate_der().to_vec(),
        }
    }
    pub fn peer(&self, expected: PeerId) -> crate::Result<crate::Peer> {
        let peer = crate::Peer::from_der(self.certificate.clone())?;
        if PeerId::from_peer(&peer) != expected {
            return Err(crate::Error::Authentication);
        }
        Ok(peer)
    }
}

/// State-machine output only. A client must obtain it from actual ordered Raft
/// application; constructing/deserializing this DTO is not quorum evidence.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyResult {
    pub accepted: bool,
    pub epoch: u64,
    pub head: [u8; 32],
    pub error: Option<String>,
    #[serde(default)]
    pub replayed: bool,
    #[serde(default)]
    pub first_log: Option<openraft::LogId<PeerId>>,
}

openraft::declare_raft_types!(pub TypeConfig:
    D = Proposal,
    R = ApplyResult,
    NodeId = PeerId,
    Node = Node,
    SnapshotData = SnapshotBuffer,
);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_peer_ids_are_canonical_and_json_map_safe() {
        let id = PeerId([171; 32]);
        let encoded = serde_json::to_string(&id).unwrap();
        assert_eq!(serde_json::from_str::<PeerId>(&encoded).unwrap(), id);
        assert!(PeerId::try_from("AB".repeat(32)).is_err());
        assert!(PeerId::try_from("é".repeat(32)).is_err());
        assert!(PeerId::try_from("ab".repeat(8)).is_err());
        let map = std::collections::BTreeMap::from([(id, Node::default())]);
        assert!(serde_json::to_string(&map).is_ok());
    }
}

#[cfg(all(test, unix))]
mod cluster_tests;
