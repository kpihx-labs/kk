//! Explicit moderator-authorized controller selection, not automatic promotion.
use super::PeerId;
use crate::{Error, Identity, Result, config, rooms::control::View, signed};
use openraft::LogId;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const DOMAIN: &[u8] = b"K&K explicit room voter intent v1\0";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Body {
    pub version: u8,
    pub room: [u8; 32],
    pub author: PeerId,
    pub voters: BTreeSet<PeerId>,
    pub before: Option<LogId<PeerId>>,
    nonce: [u8; 16],
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub body: Body,
    signature: Vec<u8>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Applied {
    pub voters: BTreeSet<PeerId>,
    pub membership: LogId<PeerId>,
}
impl Request {
    pub fn issue(
        identity: &Identity,
        room: [u8; 32],
        voters: BTreeSet<PeerId>,
        before: Option<LogId<PeerId>>,
    ) -> Result<Self> {
        if voters.is_empty() || voters.len() > config::ROOM_MAX_MEMBERS {
            return Err(Error::Configuration(
                "explicit voter set must be nonempty and bounded",
            ));
        }
        let mut nonce = [0; 16];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| Error::Authentication)?;
        let body = Body {
            version: 1,
            room,
            author: PeerId::from_peer(&identity.peer()),
            voters,
            before,
            nonce,
        };
        let signature = signed::sign(identity, DOMAIN, &body)?;
        Ok(Self { body, signature })
    }
    pub fn verify(&self, room: [u8; 32], view: &View) -> Result<()> {
        let body = &self.body;
        if body.version != 1
            || body.room != room
            || body.voters.is_empty()
            || body.voters.len() > config::ROOM_MAX_MEMBERS
            || self.signature.is_empty()
            || self.signature.len() > 80
        {
            return Err(Error::Protocol);
        }
        if !view
            .member_rights(&body.author.0)
            .is_some_and(|rights| rights.can_control())
        {
            return Err(Error::Denied);
        }
        let peers = view.peers();
        let author = peers
            .iter()
            .find(|peer| PeerId::from_peer(peer) == body.author)
            .ok_or(Error::Denied)?;
        signed::verify(author, DOMAIN, body, &self.signature)?;
        for target in &body.voters {
            if view.member_rights(&target.0).is_none() {
                return Err(Error::Denied);
            }
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::rooms::{Grant, Rights, RootBundle, control::Proposal, mls::CryptoMember};
    #[test]
    fn voter_authority_requires_explicit_bounded_admin_intent_and_exact_signature() {
        let directory = crate::test_support::tempdir().unwrap();
        let a = Identity::create(&directory.path().join("a")).unwrap();
        let b = Identity::create(&directory.path().join("b")).unwrap();
        let anchor = RootBundle::create(&a, "Controller approval fixture")
            .unwrap()
            .verify(&a.peer())
            .unwrap();
        let creator = CryptoMember::new(&a, &anchor, vec![]).unwrap();
        let mut view = View::genesis(&anchor, creator.public_identity().unwrap()).unwrap();
        let a_id = PeerId::from_peer(&a.peer());
        let b_id = PeerId::from_peer(&b.peer());
        assert!(Request::issue(&a, anchor.id(), BTreeSet::new(), None).is_err());
        let unknown = Request::issue(&a, anchor.id(), BTreeSet::from([b_id]), None).unwrap();
        assert!(matches!(
            unknown.verify(anchor.id(), &view),
            Err(Error::Denied)
        ));
        let grant = Grant::issue(&anchor, &[], &a, &b.peer(), Rights::PARTICIPANT).unwrap();
        let mut member = CryptoMember::new(&b, &anchor, vec![grant]).unwrap();
        let package = member.key_package().unwrap();
        let add = Proposal::add(
            &a,
            anchor.id(),
            0,
            view.head(),
            b"public fixture only",
            &package,
        )
        .unwrap();
        view.apply_accepted(&add).unwrap();
        let request = Request::issue(&a, anchor.id(), BTreeSet::from([a_id, b_id]), None).unwrap();
        request.verify(anchor.id(), &view).unwrap();
        assert!(request.verify([42; 32], &view).is_err());
        let mut altered = request.clone();
        altered.body.voters.remove(&b_id);
        assert!(altered.verify(anchor.id(), &view).is_err());
        let mut altered = request.clone();
        altered.body.author = b_id;
        assert!(altered.verify(anchor.id(), &view).is_err());
        let participant =
            Request::issue(&b, anchor.id(), BTreeSet::from([a_id, b_id]), None).unwrap();
        assert!(matches!(
            participant.verify(anchor.id(), &view),
            Err(Error::Denied)
        ));
    }
}
