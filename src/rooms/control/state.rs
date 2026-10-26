//! Deterministic PUBLIC state transition rules. The caller must provide verified
//! ordered acceptance; this module alone is not a replicated/quorum authority.
use super::*;
use crate::rooms::{
    Anchor, Member,
    mls::{PublicIdentity, validate_package},
};
use openmls_libcrux_crypto::CryptoProvider;
use std::collections::{BTreeMap, BTreeSet};

pub struct View {
    anchor: Anchor,
    crypto: CryptoProvider,
    epoch: u64,
    head: [u8; 32],
    members: BTreeMap<[u8; 32], (PublicIdentity, Member)>,
    consumed: BTreeSet<Vec<u8>>,
    welcomes: BTreeMap<[u8; 32], Welcome>,
}

/// Recipient-encrypted MLS artifact in committed PUBLIC state. No plaintext
/// epoch material or provider/private signing key is exported into this record.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Welcome {
    pub proposal: [u8; 32],
    pub epoch: u64,
    pub author: [u8; 32],
    pub recipient: [u8; 32],
    pub wire: Vec<u8>,
}

/// Explicit public fields only. No provider, signer, private database or ratchet.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    version: u8,
    room: [u8; 32],
    epoch: u64,
    head: [u8; 32],
    members: Vec<PublicIdentity>,
    consumed: Vec<Vec<u8>>,
    #[serde(default)]
    welcomes: Vec<Welcome>,
}
impl Snapshot {
    pub fn head(&self) -> [u8; 32] {
        self.head
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self).map_err(|_| Error::Protocol)?;
        if bytes.len() > config::ROOM_CONTROL_SNAPSHOT_BYTES {
            return Err(Error::Limit {
                resource: "public control snapshot",
                max: config::ROOM_CONTROL_SNAPSHOT_BYTES,
            });
        }
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > config::ROOM_CONTROL_SNAPSHOT_BYTES {
            return Err(Error::Limit {
                resource: "public control snapshot",
                max: config::ROOM_CONTROL_SNAPSHOT_BYTES,
            });
        }
        serde_json::from_slice(bytes).map_err(|_| Error::Protocol)
    }
    pub fn digest(&self) -> Result<[u8; 32]> {
        Ok(Sha256::digest(self.encode()?).into())
    }
}
impl View {
    pub fn genesis(anchor: &Anchor, creator: PublicIdentity) -> Result<Self> {
        let member = creator.verify(anchor)?;
        if member.peer().fingerprint_bytes() != anchor.creator.fingerprint_bytes() {
            return Err(Error::Denied);
        }
        let members = BTreeMap::from([(member.peer().fingerprint_bytes(), (creator, member))]);
        Ok(Self {
            anchor: anchor.clone(),
            crypto: CryptoProvider::new().map_err(|_| Error::Authentication)?,
            epoch: 0,
            head: anchor.id(),
            members,
            consumed: BTreeSet::new(),
            welcomes: BTreeMap::new(),
        })
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn head(&self) -> [u8; 32] {
        self.head
    }
    pub fn membership_digest(&self) -> Result<[u8; 32]> {
        crate::rooms::membership_digest(
            self.anchor.id(),
            self.epoch,
            self.members
                .iter()
                .map(|(fingerprint, (_, member))| (*fingerprint, member.grant_id())),
        )
    }
    pub fn peers(&self) -> Vec<Peer> {
        self.members
            .values()
            .map(|(_, member)| member.peer())
            .collect()
    }
    pub fn member_rights(&self, fingerprint: &[u8; 32]) -> Option<super::super::Rights> {
        self.members
            .get(fingerprint)
            .map(|(_, member)| member.rights())
    }
    /// Only a committed artifact. Callers must enforce fresh control/current
    /// recipient identity before delivering it over a room-scoped transport.
    pub fn welcome_for(&self, recipient: &[u8; 32]) -> Option<Welcome> {
        self.welcomes.get(recipient).cloned()
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            version: 1,
            room: self.anchor.id(),
            epoch: self.epoch,
            head: self.head,
            members: self
                .members
                .values()
                .map(|(identity, _)| identity.clone())
                .collect(),
            consumed: self.consumed.iter().cloned().collect(),
            welcomes: self.welcomes.values().cloned().collect(),
        }
    }
    /// expected digest/head must come from authenticated ordered delivery, not the
    /// snapshot itself. Matching a self-supplied hash proves no authority.
    pub fn restore_verified(
        anchor: &Anchor,
        snapshot: Snapshot,
        expected_head: [u8; 32],
        expected_digest: [u8; 32],
    ) -> Result<Self> {
        if snapshot.version != 1
            || snapshot.room != anchor.id()
            || snapshot.head != expected_head
            || snapshot.digest()? != expected_digest
            || snapshot.members.is_empty()
            || snapshot.members.len() > config::ROOM_MAX_MEMBERS
            || snapshot.consumed.len() > config::ROOM_PACKAGE_REPLAY_LIMIT
            || snapshot.consumed.iter().any(|hash| hash.len() != 32)
        {
            return Err(Error::Authentication);
        }
        let mut members = BTreeMap::new();
        for identity in snapshot.members {
            let member = identity.verify(anchor)?;
            if members
                .insert(member.peer().fingerprint_bytes(), (identity, member))
                .is_some()
            {
                return Err(Error::Protocol);
            }
        }
        let consumed: BTreeSet<_> = snapshot.consumed.iter().cloned().collect();
        if consumed.len() != snapshot.consumed.len() {
            return Err(Error::Protocol);
        }
        let welcomes: BTreeMap<_, _> = snapshot
            .welcomes
            .iter()
            .cloned()
            .map(|welcome| (welcome.recipient, welcome))
            .collect();
        if welcomes.len() != snapshot.welcomes.len()
            || welcomes.len() > config::ROOM_MAX_MEMBERS
            || welcomes.values().any(|welcome| {
                welcome.epoch == 0
                    || welcome.epoch > snapshot.epoch
                    || welcome.wire.is_empty()
                    || welcome.wire.len() > config::ROOM_MAX_WIRE_BYTES
                    || !members.contains_key(&welcome.recipient)
            })
            || welcomes
                .values()
                .map(|welcome| welcome.wire.len())
                .sum::<usize>()
                > config::ROOM_WELCOME_REPLICA_BYTES
        {
            return Err(Error::Protocol);
        }
        Ok(Self {
            anchor: anchor.clone(),
            crypto: CryptoProvider::new().map_err(|_| Error::Authentication)?,
            epoch: snapshot.epoch,
            head: snapshot.head,
            members,
            consumed,
            welcomes,
        })
    }
    /// Validate without mutation before proposing. No success receipt is implied.
    pub fn validate(&self, proposal: &Proposal) -> Result<()> {
        self.transition(proposal).map(|_| ())
    }
    /// Apply ONLY after verified ordered acceptance. Current log transport/quorum
    /// is a separate owner; signatures and expected epochs do not substitute it.
    pub fn apply_accepted(&mut self, proposal: &Proposal) -> Result<()> {
        let transition = self.transition(proposal)?;
        let next = self.epoch.checked_add(1).ok_or(Error::Protocol)?;
        let head = proposal.id()?;
        // Build the next public values off to the side. Resource/serialization
        // failure must leave BOTH membership and replicated delivery unchanged.
        let mut members = self.members.clone();
        let mut consumed = self.consumed.clone();
        let mut welcomes = self.welcomes.clone();
        match transition {
            Transition::Add {
                fingerprint,
                identity,
                member,
                package,
            } => {
                members.insert(fingerprint, (identity, member));
                consumed.insert(package);
                if let Some(wire) = proposal.encrypted_welcome() {
                    welcomes.insert(
                        fingerprint,
                        Welcome {
                            proposal: head,
                            epoch: next,
                            author: proposal.author(),
                            recipient: fingerprint,
                            wire: wire.to_vec(),
                        },
                    );
                }
            }
            Transition::Remove(fingerprint) => {
                members.remove(&fingerprint);
                welcomes.remove(&fingerprint);
            }
        }
        if welcomes
            .values()
            .map(|welcome| welcome.wire.len())
            .sum::<usize>()
            > config::ROOM_WELCOME_REPLICA_BYTES
        {
            return Err(Error::Limit {
                resource: "replicated encrypted Welcomes",
                max: config::ROOM_WELCOME_REPLICA_BYTES,
            });
        }
        Snapshot {
            version: 1,
            room: self.anchor.id(),
            epoch: next,
            head,
            members: members
                .values()
                .map(|(identity, _)| identity.clone())
                .collect(),
            consumed: consumed.iter().cloned().collect(),
            welcomes: welcomes.values().cloned().collect(),
        }
        .encode()?;
        self.members = members;
        self.consumed = consumed;
        self.welcomes = welcomes;
        self.epoch = next;
        self.head = head;
        Ok(())
    }
    fn transition(&self, proposal: &Proposal) -> Result<Transition> {
        let (_, author) = self.members.get(&proposal.author()).ok_or(Error::Denied)?;
        proposal.verify(self.anchor.id(), self.epoch, self.head, &author.peer())?;
        match proposal.action()? {
            Action::Add => {
                if self.members.len() >= config::ROOM_MAX_MEMBERS
                    || self.consumed.len() >= config::ROOM_PACKAGE_REPLAY_LIMIT
                {
                    return Err(Error::Denied);
                }
                let candidate = validate_package(
                    &self.anchor,
                    &self.crypto,
                    proposal.package_bytes().ok_or(Error::Protocol)?,
                )?;
                let member = candidate.member.clone();
                let fingerprint = member.peer().fingerprint_bytes();
                if self.members.contains_key(&fingerprint)
                    || !author.rights().permits_issuance(member.rights())
                {
                    return Err(Error::Denied);
                }
                let issuer = self
                    .members
                    .values()
                    .find(|(_, known)| known.peer().fingerprint() == member.issuer)
                    .map(|(_, known)| known)
                    .ok_or(Error::Denied)?;
                if !issuer.rights().permits_issuance(member.rights()) {
                    return Err(Error::Denied);
                }
                let package = candidate
                    .package
                    .hash_ref(&self.crypto)
                    .map_err(|_| Error::Authentication)?
                    .as_slice()
                    .to_vec();
                if self.consumed.contains(&package) {
                    return Err(Error::Denied);
                }
                Ok(Transition::Add {
                    fingerprint,
                    identity: candidate.public_identity()?,
                    member,
                    package,
                })
            }
            Action::Remove => {
                let target = proposal.removed().ok_or(Error::Protocol)?;
                if !author.rights().can_remove()
                    || target == proposal.author()
                    || !self.members.contains_key(&target)
                {
                    return Err(Error::Denied);
                }
                Ok(Transition::Remove(target))
            }
        }
    }
}

enum Transition {
    Add {
        fingerprint: [u8; 32],
        identity: PublicIdentity,
        member: Member,
        package: Vec<u8>,
    },
    Remove([u8; 32]),
}
