//! Offline room-scoped authority proofs. Network admission and replicated state
//! are separate layers; verifying a grant never imports a global contact.

use crate::{Error, Identity, Peer, Result, signed};
pub mod control;
pub mod conversation;
mod database;
pub mod enrollment;
pub mod group;
mod lease;
#[cfg(unix)]
pub mod local;
pub mod mls;
pub mod runtime;
pub mod store;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const ROOT_DOMAIN: &[u8] = b"K&K room root v1\0";
const GRANT_DOMAIN: &[u8] = b"K&K room grant v1\0";
pub const MAX_CHAIN: usize = 8;
pub const MAX_BUNDLE: usize = 131_072;

// Common public/private view commitment: logical identity and authority, not
// private epoch authenticators, traffic secrets or changing HPKE path keys.
pub(super) fn membership_digest(
    room: [u8; 32],
    epoch: u64,
    identities: impl IntoIterator<Item = ([u8; 32], [u8; 32])>,
) -> Result<[u8; 32]> {
    let mut identities: Vec<_> = identities
        .into_iter()
        .take(crate::config::ROOM_MAX_MEMBERS + 1)
        .collect();
    if identities.is_empty() || identities.len() > crate::config::ROOM_MAX_MEMBERS {
        return Err(Error::Protocol);
    }
    identities.sort_unstable();
    if identities.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(Error::Protocol);
    }
    let mut digest = Sha256::new();
    digest.update(b"K&K logical membership v1\0");
    digest.update(room);
    digest.update(epoch.to_be_bytes());
    digest.update((identities.len() as u32).to_be_bytes());
    for (fingerprint, grant) in identities {
        digest.update(fingerprint);
        digest.update(grant);
    }
    Ok(digest.finalize().into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rights(u8);
impl Rights {
    fn permits_issuance(self, next: Self) -> bool {
        self.can_invite()
            && next.valid()
            && next.0 & !self.0 == 0
            && (next.0 & 28 == 0 || self.can_delegate())
    }
    pub const READER: Self = Self(1);
    pub const PARTICIPANT: Self = Self(3);
    pub const INVITER: Self = Self(7);
    pub const MODERATOR: Self = Self(31);
    pub fn can_publish(self) -> bool {
        self.0 & 2 != 0
    }
    pub fn can_invite(self) -> bool {
        self.0 & 4 != 0
    }
    pub fn can_delegate(self) -> bool {
        self.0 & 8 != 0
    }
    pub fn can_remove(self) -> bool {
        self.0 & 16 != 0
    }
    /// Explicit controller-set approval requires the existing administrative
    /// ceiling, never mere membership, read/publish access or device possession.
    pub fn can_control(self) -> bool {
        self.can_invite() && self.can_remove() && self.can_delegate()
    }
    fn valid(self) -> bool {
        self.0 & 1 != 0 && self.0 & !31 == 0
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootBody {
    version: u8,
    nonce: [u8; 32],
    title: String,
    creator: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootBundle {
    body: RootBody,
    signature: Vec<u8>,
}

#[derive(Clone)]
pub struct Anchor {
    bundle: RootBundle,
    creator: Peer,
    id: [u8; 32],
}

impl RootBundle {
    pub fn create(identity: &Identity, title: &str) -> Result<Self> {
        if title.is_empty() || title.len() > 80 {
            return Err(Error::Configuration(
                "room title must contain 1..80 UTF-8 bytes",
            ));
        }
        let mut nonce = [0; 32];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| Error::Authentication)?;
        let body = RootBody {
            version: 1,
            nonce,
            title: title.to_owned(),
            creator: identity.peer().certificate.to_vec(),
        };
        let signature = signed::sign(identity, ROOT_DOMAIN, &body)?;
        Ok(Self { body, signature })
    }
    /// expected_creator must come from explicit approval, not discovery or the bundle itself.
    pub fn verify(&self, expected_creator: &Peer) -> Result<Anchor> {
        if self.body.version != 1 || self.body.title.is_empty() || self.body.title.len() > 80 {
            return Err(Error::Protocol);
        }
        let creator = Peer::from_der(self.body.creator.clone())?;
        if creator.fingerprint() != expected_creator.fingerprint() {
            return Err(Error::Authentication);
        }
        signed::verify(&creator, ROOT_DOMAIN, &self.body, &self.signature)?;
        let id = Sha256::digest(signed::preimage(ROOT_DOMAIN, &self.body)?).into();
        Ok(Anchor {
            bundle: self.clone(),
            creator,
            id,
        })
    }
    /// Alternative explicit trust input: an already approved exact room digest
    /// (e.g. the purpose-bound invitation). Never take this expected ID from the
    /// same untrusted authority payload and call that independent approval.
    pub fn verify_id(&self, expected_room: [u8; 32]) -> Result<Anchor> {
        let id: [u8; 32] = Sha256::digest(signed::preimage(ROOT_DOMAIN, &self.body)?).into();
        if id != expected_room {
            return Err(Error::Authentication);
        }
        self.verify(&Peer::from_der(self.body.creator.clone())?)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        encode(self)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        decode(bytes)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantBody {
    room: [u8; 32],
    parent: [u8; 32],
    nonce: [u8; 16],
    issuer: String,
    subject: Vec<u8>,
    rights: Rights,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    body: GrantBody,
    signature: Vec<u8>,
}

#[derive(Clone)]
pub struct Member {
    peer: Peer,
    rights: Rights,
    grant: [u8; 32],
    issuer: String,
}
impl Member {
    pub fn peer(&self) -> Peer {
        self.peer.clone()
    }
    pub fn rights(&self) -> Rights {
        self.rights
    }
    pub fn grant_id(&self) -> [u8; 32] {
        self.grant
    }
}

impl Anchor {
    pub fn id(&self) -> [u8; 32] {
        self.id
    }
    pub fn title(&self) -> &str {
        &self.bundle.body.title
    }
    pub fn bundle(&self) -> RootBundle {
        self.bundle.clone()
    }
    /// Cryptographic chain verification only. Revocation and admission state must
    /// additionally be checked by the room's state owner before any effect.
    pub fn verify_chain(&self, chain: &[Grant]) -> Result<Member> {
        if chain.len() > MAX_CHAIN {
            return Err(Error::Protocol);
        }
        let mut member = Member {
            peer: self.creator.clone(),
            rights: Rights::MODERATOR,
            grant: self.id,
            issuer: self.creator.fingerprint(),
        };
        let mut seen = BTreeSet::from([member.peer.fingerprint()]);
        for grant in chain {
            let body = &grant.body;
            if body.room != self.id
                || body.parent != member.grant
                || body.issuer != member.peer.fingerprint()
                || !member.rights.permits_issuance(body.rights)
            {
                return Err(Error::Denied);
            }
            signed::verify(&member.peer, GRANT_DOMAIN, body, &grant.signature)?;
            let peer = Peer::from_der(body.subject.clone())?;
            if !seen.insert(peer.fingerprint()) {
                return Err(Error::Denied);
            }
            member = Member {
                peer,
                rights: body.rights,
                grant: grant.id()?,
                issuer: body.issuer.clone(),
            };
        }
        Ok(member)
    }
}
impl Grant {
    pub fn issue(
        anchor: &Anchor,
        parents: &[Grant],
        issuer: &Identity,
        subject: &Peer,
        rights: Rights,
    ) -> Result<Self> {
        if parents.len() >= MAX_CHAIN {
            return Err(Error::Protocol);
        }
        let parent = anchor.verify_chain(parents)?;
        if issuer.peer().fingerprint() != parent.peer.fingerprint() {
            return Err(Error::Authentication);
        }
        let mut nonce = [0; 16];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| Error::Authentication)?;
        let body = GrantBody {
            room: anchor.id,
            parent: parent.grant,
            nonce,
            issuer: issuer.peer().fingerprint(),
            subject: subject.certificate.to_vec(),
            rights,
        };
        let signature = signed::sign(issuer, GRANT_DOMAIN, &body)?;
        let grant = Self { body, signature };
        let mut chain = parents.to_vec();
        chain.push(grant.clone());
        anchor.verify_chain(&chain)?;
        Ok(grant)
    }
    pub fn id(&self) -> Result<[u8; 32]> {
        Ok(Sha256::digest(signed::preimage(GRANT_DOMAIN, &self.body)?).into())
    }
    pub fn encode_chain(chain: &[Self]) -> Result<Vec<u8>> {
        if chain.len() > MAX_CHAIN {
            return Err(Error::Protocol);
        }
        encode(chain)
    }
    pub fn decode_chain(bytes: &[u8]) -> Result<Vec<Self>> {
        let chain: Vec<Self> = decode(bytes)?;
        if chain.len() > MAX_CHAIN {
            return Err(Error::Protocol);
        }
        Ok(chain)
    }
}
fn encode(value: &(impl Serialize + ?Sized)) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Protocol)?;
    if bytes.len() > MAX_BUNDLE {
        return Err(Error::Limit {
            resource: "room bundle",
            max: MAX_BUNDLE,
        });
    }
    Ok(bytes)
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    if bytes.len() > MAX_BUNDLE {
        return Err(Error::Limit {
            resource: "room bundle",
            max: MAX_BUNDLE,
        });
    }
    serde_json::from_slice(bytes).map_err(|_| Error::Protocol)
}
