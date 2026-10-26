//! Device-key-bound node metadata. Enrollment adds replication only; a configured
//! device may update its own address. Neither action grants data rights or votes.
use super::{Node, PeerId};
use crate::{Error, Identity, Peer, Result, rooms::control::View, signed};
use openraft::LogId;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

const DOMAIN: &[u8] = b"K&K room node metadata intent v2\0";
#[derive(Clone, Copy, Serialize, Deserialize)]
pub(super) enum Kind {
    Learner,
    Address,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Body {
    pub version: u8,
    pub kind: Kind,
    pub room: [u8; 32],
    pub author: PeerId,
    pub target: PeerId,
    pub endpoint: SocketAddr,
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
    pub target: PeerId,
    pub node: Node,
    pub membership: LogId<PeerId>,
}
impl Request {
    pub fn issue(
        identity: &Identity,
        room: [u8; 32],
        target: PeerId,
        endpoint: SocketAddr,
        before: Option<LogId<PeerId>>,
    ) -> Result<Self> {
        Self::signed(identity, room, target, endpoint, before, Kind::Learner)
    }
    pub fn address(
        identity: &Identity,
        room: [u8; 32],
        endpoint: SocketAddr,
        before: Option<LogId<PeerId>>,
    ) -> Result<Self> {
        Self::signed(
            identity,
            room,
            PeerId::from_peer(&identity.peer()),
            endpoint,
            before,
            Kind::Address,
        )
    }
    fn signed(
        identity: &Identity,
        room: [u8; 32],
        target: PeerId,
        endpoint: SocketAddr,
        before: Option<LogId<PeerId>>,
        kind: Kind,
    ) -> Result<Self> {
        let mut nonce = [0; 16];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| Error::Authentication)?;
        let body = Body {
            version: 2,
            kind,
            room,
            author: PeerId::from_peer(&identity.peer()),
            target,
            endpoint,
            before,
            nonce,
        };
        let signature = signed::sign(identity, DOMAIN, &body)?;
        Ok(Self { body, signature })
    }
    pub fn verify(&self, room: [u8; 32], view: &View, configured: Option<&Node>) -> Result<Peer> {
        let body = &self.body;
        if body.version != 2
            || body.room != room
            || self.signature.is_empty()
            || self.signature.len() > 80
            || body.endpoint.port() == 0
            || body.endpoint.ip().is_unspecified()
            || body.endpoint.ip().is_multicast()
        {
            return Err(Error::Protocol);
        }
        if let Kind::Address = body.kind {
            if body.author != body.target || body.before.is_none() {
                return Err(Error::Protocol);
            }
            let peer = configured.ok_or(Error::Denied)?.peer(body.author)?;
            signed::verify(&peer, DOMAIN, &self.body, &self.signature)?;
            return Ok(peer);
        }
        if body.author == body.target {
            return Err(Error::Protocol);
        }
        let peers = view.peers();
        let author = peers
            .iter()
            .find(|peer| peer.fingerprint_bytes() == body.author.0)
            .ok_or(Error::Denied)?;
        if !view
            .member_rights(&body.author.0)
            .is_some_and(|rights| rights.can_invite())
        {
            return Err(Error::Denied);
        }
        signed::verify(author, DOMAIN, &self.body, &self.signature)?;
        peers
            .into_iter()
            .find(|peer| peer.fingerprint_bytes() == body.target.0)
            .ok_or(Error::Denied)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::rooms::{Grant, Rights, RootBundle, control::Proposal, mls::CryptoMember};
    #[test]
    fn self_address_is_bound_to_configured_device_purpose_room_and_prior_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let owner = Identity::create(&directory.path().join("owner")).unwrap();
        let device = Identity::create(&directory.path().join("device")).unwrap();
        let root = RootBundle::create(&owner, "Node address authorization")
            .unwrap()
            .verify(&owner.peer())
            .unwrap();
        let creator = CryptoMember::new(&owner, &root, vec![]).unwrap();
        let view = View::genesis(&root, creator.public_identity().unwrap()).unwrap();
        let configured = Node::for_peer(&device.peer(), Some("127.0.0.1:45460".parse().unwrap()));
        let before = Some(LogId::new(
            openraft::CommittedLeaderId::new(1, PeerId::from_peer(&owner.peer())),
            3,
        ));
        let request = Request::address(
            &device,
            root.id(),
            "127.0.0.1:45461".parse().unwrap(),
            before,
        )
        .unwrap();
        // Configured controller/replication authority is independent of data
        // membership. This does not add the device to the room or grant a vote.
        assert_eq!(
            request
                .verify(root.id(), &view, Some(&configured))
                .unwrap()
                .fingerprint(),
            device.peer().fingerprint()
        );
        assert!(request.verify(root.id(), &view, None).is_err());
        assert!(request.verify([42; 32], &view, Some(&configured)).is_err());
        let mut wrong = request.clone();
        wrong.body.endpoint.set_port(45462);
        assert!(wrong.verify(root.id(), &view, Some(&configured)).is_err());
        let mut wrong = request.clone();
        wrong.body.kind = Kind::Learner;
        assert!(wrong.verify(root.id(), &view, Some(&configured)).is_err());
        let mut wrong = request;
        wrong.body.target = PeerId::from_peer(&owner.peer());
        assert!(wrong.verify(root.id(), &view, Some(&configured)).is_err());
    }
    #[test]
    fn device_signature_context_and_current_invite_rights_bind_learner_intent() {
        let directory = tempfile::tempdir().unwrap();
        let a = Identity::create(&directory.path().join("a")).unwrap();
        let b = Identity::create(&directory.path().join("b")).unwrap();
        let c = Identity::create(&directory.path().join("c")).unwrap();
        let root = RootBundle::create(&a, "Learner authorization fixture")
            .unwrap()
            .verify(&a.peer())
            .unwrap();
        let creator = CryptoMember::new(&a, &root, vec![]).unwrap();
        let mut view = View::genesis(&root, creator.public_identity().unwrap()).unwrap();
        for (identity, rights) in [(&b, Rights::MODERATOR), (&c, Rights::PARTICIPANT)] {
            let grant = Grant::issue(&root, &[], &a, &identity.peer(), rights).unwrap();
            let mut member = CryptoMember::new(identity, &root, vec![grant]).unwrap();
            let package = member.key_package().unwrap();
            // Public authorization fixture only, not an MLS/quorum claim.
            let proposal = Proposal::add(
                &a,
                root.id(),
                view.epoch(),
                view.head(),
                b"opaque fixture",
                &package,
            )
            .unwrap();
            view.apply_accepted(&proposal).unwrap();
        }
        let target = PeerId::from_peer(&c.peer());
        let request = Request::issue(
            &b,
            root.id(),
            target,
            "127.0.0.1:45461".parse().unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(
            request
                .verify(root.id(), &view, None)
                .unwrap()
                .fingerprint(),
            c.peer().fingerprint()
        );
        assert!(request.verify([42; 32], &view, None).is_err());
        let mut altered = request.clone();
        altered.body.endpoint.set_port(45462);
        assert!(altered.verify(root.id(), &view, None).is_err());
        let mut altered = request.clone();
        altered.body.target = PeerId::from_peer(&a.peer());
        assert!(altered.verify(root.id(), &view, None).is_err());
        let mut altered = request.clone();
        altered.body.author = PeerId::from_peer(&a.peer());
        assert!(altered.verify(root.id(), &view, None).is_err());
        let unprivileged = Request::issue(
            &c,
            root.id(),
            PeerId::from_peer(&b.peer()),
            "127.0.0.1:45461".parse().unwrap(),
            None,
        )
        .unwrap();
        assert!(matches!(
            unprivileged.verify(root.id(), &view, None),
            Err(Error::Denied)
        ));
        let removal = Proposal::remove(
            &a,
            root.id(),
            view.epoch(),
            view.head(),
            b"opaque removal",
            &b.peer(),
        )
        .unwrap();
        view.apply_accepted(&removal).unwrap();
        assert!(matches!(
            request.verify(root.id(), &view, None),
            Err(Error::Denied)
        ));
    }
}
