//! Consent to remove only the signing member, not an MLS commit or quorum proof.
use super::{View, signed};
use crate::rooms::control::raft::PeerId;
use crate::{Error, Identity, Peer, Result};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

const DOMAIN: &[u8] = b"K&K room voluntary departure request v1\0";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    version: u8,
    room: [u8; 32],
    member: PeerId,
    epoch: u64,
    head: [u8; 32],
    nonce: [u8; 16],
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    body: Body,
    signature: Vec<u8>,
}

impl Request {
    pub(crate) fn issue(identity: &Identity, room: [u8; 32], view: &View) -> Result<Self> {
        let mut nonce = [0; 16];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| Error::Authentication)?;
        let body = Body {
            version: 1,
            room,
            member: PeerId::from_peer(&identity.peer()),
            epoch: view.epoch(),
            head: view.head(),
            nonce,
        };
        let signature = signed::sign(identity, DOMAIN, &body)?;
        Ok(Self { body, signature })
    }

    pub fn member(&self) -> PeerId {
        self.body.member
    }

    pub(crate) fn verify(&self, room: [u8; 32], view: &View) -> Result<Peer> {
        if self.body.version != 1
            || self.body.room != room
            || self.body.epoch != view.epoch()
            || self.body.head != view.head()
            || self.signature.is_empty()
            || self.signature.len() > 80
        {
            return Err(Error::Denied);
        }
        let member = view
            .peers()
            .into_iter()
            .find(|peer| PeerId::from_peer(peer) == self.body.member)
            .ok_or(Error::Denied)?;
        signed::verify(&member, DOMAIN, &self.body, &self.signature)?;
        Ok(member)
    }
}
