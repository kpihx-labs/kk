//! Public control proposals, never private MLS snapshots. A signature authenticates
//! the author and context; it is NOT current permission, log acceptance or a quorum.

use crate::{Error, Identity, Peer, Result, config, signed};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tls_codec::DeserializeBytes as _;
use tls_codec::{TlsByteVecU16, TlsByteVecU32, TlsDeserializeBytes, TlsSerialize, TlsSize};

const DOMAIN: &[u8] = b"K&K room control proposal v2\0";
pub mod departure;
pub mod raft;
mod state;
pub use state::{Snapshot, View, Welcome};

#[derive(Clone, Serialize, Deserialize, TlsSize, TlsSerialize, TlsDeserializeBytes)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u8,
    room: [u8; 32],
    epoch_before: u64,
    previous: [u8; 32],
    author: [u8; 32],
    nonce: [u8; 16],
    action: u8,
    payload: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Add,
    Remove,
}

#[derive(Clone, Serialize, Deserialize, TlsSize, TlsSerialize, TlsDeserializeBytes)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    header: Header,
    commit: TlsByteVecU32,
    subject: TlsByteVecU32,
    welcome: TlsByteVecU32,
    signature: TlsByteVecU16,
    // Consensus metadata, not a client assertion or part of the signed codec/ID.
    // Client ingress removes it before fresh validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[tls_codec(skip)]
    ordered: Option<OrderedDecision>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderedDecision {
    version: u8,
    accepted: bool,
}

impl std::fmt::Debug for Proposal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ControlProposal")
            .field("epoch_before", &self.header.epoch_before)
            .field("action", &self.action())
            .field("commit_bytes", &self.commit.len())
            .field("subject_bytes", &self.subject.len())
            .field("welcome_bytes", &self.welcome.len())
            .finish()
    }
}

impl Proposal {
    pub fn add(
        identity: &Identity,
        room: [u8; 32],
        epoch_before: u64,
        previous: [u8; 32],
        commit: &[u8],
        package: &[u8],
    ) -> Result<Self> {
        Self::prepare(
            identity,
            (room, epoch_before, previous),
            1,
            commit,
            package,
            &[],
        )
    }
    /// Encrypted SDK Welcome travels only on the authorized control plane until
    /// its admission commits. This replicates NO private MLS database or keys.
    pub(crate) fn add_with_welcome(
        identity: &Identity,
        room: [u8; 32],
        epoch_before: u64,
        previous: [u8; 32],
        commit: &[u8],
        package: &[u8],
        welcome: &[u8],
    ) -> Result<Self> {
        if welcome.is_empty() {
            return Err(Error::Protocol);
        }
        Self::prepare(
            identity,
            (room, epoch_before, previous),
            1,
            commit,
            package,
            welcome,
        )
    }
    pub fn remove(
        identity: &Identity,
        room: [u8; 32],
        epoch_before: u64,
        previous: [u8; 32],
        commit: &[u8],
        removed: &Peer,
    ) -> Result<Self> {
        Self::prepare(
            identity,
            (room, epoch_before, previous),
            2,
            commit,
            &removed.fingerprint_bytes(),
            &[],
        )
    }
    fn prepare(
        identity: &Identity,
        context: ([u8; 32], u64, [u8; 32]),
        action: u8,
        commit: &[u8],
        subject: &[u8],
        welcome: &[u8],
    ) -> Result<Self> {
        let (room, epoch_before, previous) = context;
        payload_bounds(action, commit, subject, welcome)?;
        let mut nonce = [0; 16];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| Error::Authentication)?;
        let header = Header {
            version: 2,
            room,
            epoch_before,
            previous,
            author: identity.peer().fingerprint_bytes(),
            nonce,
            action,
            payload: payload_digest(commit, subject, welcome),
        };
        let signature = signed::sign(identity, DOMAIN, &header)?;
        Ok(Self {
            header,
            commit: commit.to_vec().into(),
            subject: subject.to_vec().into(),
            welcome: welcome.to_vec().into(),
            signature: signature.into(),
            ordered: None,
        })
    }
    pub(crate) fn clear_ordered_decision(&mut self) {
        self.ordered = None;
    }
    /// Leader-only after fresh preflight, or local recovery with an independently
    /// checked journal hash. Never trust this metadata from a client request.
    pub(crate) fn with_ordered_decision(mut self, accepted: bool) -> Self {
        self.ordered = Some(OrderedDecision {
            version: 1,
            accepted,
        });
        self
    }
    pub(crate) fn ordered_decision(&self) -> Result<Option<bool>> {
        match &self.ordered {
            None => Ok(None),
            Some(decision) if decision.version == 1 => Ok(Some(decision.accepted)),
            Some(_) => Err(Error::Protocol),
        }
    }
    pub fn room(&self) -> [u8; 32] {
        self.header.room
    }
    pub fn epoch_before(&self) -> u64 {
        self.header.epoch_before
    }
    pub fn previous(&self) -> [u8; 32] {
        self.header.previous
    }
    pub fn author(&self) -> [u8; 32] {
        self.header.author
    }
    pub fn action(&self) -> Result<Action> {
        match self.header.action {
            1 => Ok(Action::Add),
            2 => Ok(Action::Remove),
            _ => Err(Error::Protocol),
        }
    }
    pub fn commit_bytes(&self) -> &[u8] {
        self.commit.as_slice()
    }
    pub fn package_bytes(&self) -> Option<&[u8]> {
        (self.header.action == 1).then(|| self.subject.as_slice())
    }
    /// Ciphertext for trusted control replication, NOT an admission receipt.
    pub(crate) fn encrypted_welcome(&self) -> Option<&[u8]> {
        (!self.welcome.is_empty()).then(|| self.welcome.as_slice())
    }
    pub fn removed(&self) -> Option<[u8; 32]> {
        (self.header.action == 2)
            .then(|| self.subject.as_slice().try_into().ok())
            .flatten()
    }
    pub fn id(&self) -> Result<[u8; 32]> {
        self.shape()?;
        Ok(Sha256::digest(signed::preimage(DOMAIN, &self.header)?).into())
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.shape()?;
        let bytes =
            tls_codec::Serialize::tls_serialize_detached(self).map_err(|_| Error::Protocol)?;
        if bytes.len() > config::ROOM_CONTROL_WIRE_BYTES {
            return Err(Error::Limit {
                resource: "control proposal",
                max: config::ROOM_CONTROL_WIRE_BYTES,
            });
        }
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > config::ROOM_CONTROL_WIRE_BYTES {
            return Err(Error::Limit {
                resource: "control proposal",
                max: config::ROOM_CONTROL_WIRE_BYTES,
            });
        }
        let proposal = Self::tls_deserialize_exact_bytes(bytes).map_err(|_| Error::Protocol)?;
        proposal.shape()?;
        Ok(proposal)
    }
    fn shape(&self) -> Result<()> {
        if self.header.version != 2 || self.signature.is_empty() || self.signature.len() > 80 {
            return Err(Error::Protocol);
        }
        payload_bounds(
            self.header.action,
            self.commit.as_slice(),
            self.subject.as_slice(),
            self.welcome.as_slice(),
        )?;
        if self.header.payload
            != payload_digest(
                self.commit.as_slice(),
                self.subject.as_slice(),
                self.welcome.as_slice(),
            )
        {
            return Err(Error::Authentication);
        }
        Ok(())
    }
    // The public state owner selects the CURRENT member and exact expected head.
    // Do not turn this cryptographic check alone into a successful commit receipt.
    pub(crate) fn verify(
        &self,
        room: [u8; 32],
        epoch: u64,
        previous: [u8; 32],
        author: &Peer,
    ) -> Result<()> {
        self.shape()?;
        if self.header.room != room
            || self.header.epoch_before != epoch
            || self.header.previous != previous
            || self.header.author != author.fingerprint_bytes()
        {
            return Err(Error::Authentication);
        }
        signed::verify(author, DOMAIN, &self.header, self.signature.as_slice())
    }
}

fn payload_bounds(action: u8, commit: &[u8], subject: &[u8], welcome: &[u8]) -> Result<()> {
    if commit.is_empty()
        || commit.len() > config::ROOM_MAX_WIRE_BYTES
        || subject.len() > config::ROOM_MAX_WIRE_BYTES
        || welcome.len() > config::ROOM_MAX_WIRE_BYTES
    {
        return Err(Error::Limit {
            resource: "control payload",
            max: config::ROOM_MAX_WIRE_BYTES,
        });
    }
    match action {
        1 if !subject.is_empty() => Ok(()),
        2 if subject.len() == 32 && welcome.is_empty() => Ok(()),
        _ => Err(Error::Protocol),
    }
}
fn payload_digest(commit: &[u8], subject: &[u8], welcome: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update((commit.len() as u32).to_be_bytes());
    digest.update(commit);
    digest.update((subject.len() as u32).to_be_bytes());
    digest.update(subject);
    digest.update((welcome.len() as u32).to_be_bytes());
    digest.update(welcome);
    digest.finalize().into()
}

#[cfg(all(test, unix))]
mod tests;
