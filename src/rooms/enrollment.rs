//! Signed authority verification for the scoped transport's first stage.
//! Roots/grants remain their existing schemes; no private key enters this DTO.
use super::{
    Anchor, Grant, Rights, RootBundle,
    mls::{CryptoMember, PublicIdentity},
};
use super::{
    control::raft::{
        Node, network,
        service::{ControlNode, Mode, Settings},
    },
    runtime::{Handle, Room},
};
use crate::contacts::{Action, Registry};
use crate::pairing::{EnrollmentStage, RoomSession};
use crate::{Error, Identity, Peer, Result, config, signed};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    path::Path,
};
use std::{path::PathBuf, sync::Arc, time::Duration};

const DOMAIN: &[u8] = b"K&K scoped enrollment authority v1\0";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seed {
    pub certificate: Vec<u8>,
    pub endpoint: SocketAddr,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    body: Body,
    proof: Vec<u8>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    version: u8,
    root: RootBundle,
    creator: PublicIdentity,
    inviter: PublicIdentity,
    grants: Vec<Grant>,
    seeds: Vec<Seed>,
}
pub struct VerifiedAuthority {
    anchor: Anchor,
    creator: PublicIdentity,
    grants: Vec<Grant>,
    subject: [u8; 32],
    seeds: Vec<Seed>,
}
impl Authority {
    pub(crate) fn issued(
        identity: &Identity,
        anchor: &Anchor,
        creator: PublicIdentity,
        inviter: PublicIdentity,
        grants: Vec<Grant>,
        seeds: Vec<Seed>,
    ) -> Result<Self> {
        let body = Body {
            version: 1,
            root: anchor.bundle().clone(),
            creator,
            inviter,
            grants,
            seeds,
        };
        let proof = signed::sign(identity, DOMAIN, &body.digest()?)?;
        Ok(Self { body, proof })
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self).map_err(|_| Error::Protocol)?;
        if bytes.len() > config::ROOM_BOOTSTRAP_AUTHORITY_BYTES {
            return Err(Error::Limit {
                resource: "room bootstrap authority",
                max: config::ROOM_BOOTSTRAP_AUTHORITY_BYTES,
            });
        }
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > config::ROOM_BOOTSTRAP_AUTHORITY_BYTES {
            return Err(Error::Limit {
                resource: "room bootstrap authority",
                max: config::ROOM_BOOTSTRAP_AUTHORITY_BYTES,
            });
        }
        serde_json::from_slice(bytes).map_err(|_| Error::Protocol)
    }
    /// expected_room comes from the already approved scoped invitation, NOT
    /// this payload. expected_inviter is the proved TLS peer. Exact accepted
    /// rights are explicit, so a transport peer cannot silently broaden them.
    pub fn verify(
        &self,
        expected_room: [u8; 32],
        expected_inviter: &Peer,
        subject: &Peer,
        accepted_rights: Rights,
    ) -> Result<VerifiedAuthority> {
        let body = &self.body;
        if body.version != 1 || body.grants.is_empty() {
            return Err(Error::Protocol);
        }
        signed::verify(expected_inviter, DOMAIN, &body.digest()?, &self.proof)?;
        let anchor = body.root.verify_id(expected_room)?;
        let creator = body.creator.verify(&anchor)?;
        if creator.grant_id() != anchor.id()
            || creator.peer().fingerprint_bytes() != anchor.creator.fingerprint_bytes()
        {
            return Err(Error::Authentication);
        }
        let inviter = body.inviter.verify(&anchor)?;
        if inviter.peer().fingerprint_bytes() != expected_inviter.fingerprint_bytes()
            || !inviter.rights().can_invite()
        {
            return Err(Error::Denied);
        }
        let granted = anchor.verify_chain(&body.grants)?;
        if granted.peer().fingerprint_bytes() != subject.fingerprint_bytes()
            || granted.rights() != accepted_rights
        {
            return Err(Error::Denied);
        }
        let parent = anchor.verify_chain(&body.grants[..body.grants.len() - 1])?;
        if parent.grant_id() != inviter.grant_id()
            || parent.peer().fingerprint_bytes() != expected_inviter.fingerprint_bytes()
        {
            return Err(Error::Authentication);
        }
        if body.seeds.is_empty() || body.seeds.len() > config::ROOM_MAX_MEMBERS {
            return Err(Error::Protocol);
        }
        let mut unique = BTreeSet::new();
        for seed in &body.seeds {
            let peer = Peer::from_der(seed.certificate.clone())?;
            if !unique.insert(peer.fingerprint_bytes())
                || seed.endpoint.port() == 0
                || seed.endpoint.ip().is_unspecified()
                || seed.endpoint.ip().is_multicast()
            {
                return Err(Error::Protocol);
            }
        }
        Ok(VerifiedAuthority {
            anchor,
            creator: body.creator.clone(),
            grants: body.grants.clone(),
            subject: subject.fingerprint_bytes(),
            seeds: body.seeds.clone(),
        })
    }
}
impl Body {
    fn digest(&self) -> Result<[u8; 32]> {
        let bytes = serde_json::to_vec(self).map_err(|_| Error::Protocol)?;
        if bytes.len() > config::ROOM_BOOTSTRAP_AUTHORITY_BYTES {
            return Err(Error::Limit {
                resource: "room bootstrap authority",
                max: config::ROOM_BOOTSTRAP_AUTHORITY_BYTES,
            });
        }
        Ok(Sha256::digest(bytes).into())
    }
}
impl VerifiedAuthority {
    pub fn anchor(&self) -> &Anchor {
        &self.anchor
    }
    pub fn creator(&self) -> &PublicIdentity {
        &self.creator
    }
    pub fn seeds(&self) -> &[Seed] {
        &self.seeds
    }
    pub fn control_peers(
        &self,
        identity: &Identity,
    ) -> Result<BTreeMap<super::control::raft::PeerId, Peer>> {
        if identity.peer().fingerprint_bytes() != self.subject {
            return Err(Error::Authentication);
        }
        let mut peers = BTreeMap::new();
        for seed in &self.seeds {
            let peer = Peer::from_der(seed.certificate.clone())?;
            peers.insert(super::control::raft::PeerId::from_peer(&peer), peer);
        }
        peers.insert(
            super::control::raft::PeerId::from_peer(&identity.peer()),
            identity.peer(),
        );
        if peers.len() > config::ROOM_MAX_MEMBERS {
            return Err(Error::Limit {
                resource: "room bootstrap peers",
                max: config::ROOM_MAX_MEMBERS,
            });
        }
        Ok(peers)
    }
    pub fn create_candidate(&self, identity: &Identity, state: &Path) -> Result<CryptoMember> {
        if identity.peer().fingerprint_bytes() != self.subject {
            return Err(Error::Authentication);
        }
        CryptoMember::create_private(identity, &self.anchor, self.grants.clone(), state)
    }
}

/// New candidate paths are explicit. An error after private join requires
/// reopening that existing state, NOT re-running this creation path over it.
pub struct CandidateOptions {
    pub private_state: PathBuf,
    pub control_state: PathBuf,
    pub profile: PathBuf,
    pub listen: SocketAddr,
    pub rights: Rights,
    pub deadline: Duration,
}
pub struct Joined {
    pub room: Room,
    pub control: Arc<ControlNode>,
    pub admission: [u8; 32],
}
/// Verified public bootstrap and its original purpose-bound live session. The
/// caller may durably save this public context BEFORE any private key creation.
pub struct Prepared {
    session: RoomSession,
    identity: Arc<Identity>,
    options: CandidateOptions,
    authority: VerifiedAuthority,
    inviter: Peer,
    expires: tokio::time::Instant,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplicatedAdmission {
    version: u8,
    room: [u8; 32],
    subject: [u8; 32],
    admission: [u8; 32],
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageOffer {
    package: Vec<u8>,
    endpoint: SocketAddr,
}

/// The proved inviter must currently host the control leader for this primitive.
/// This result means committed/replicated admission, NOT claimed remote private
/// activation or a vote. No arbitrary callback owns these trust rules.
pub async fn admit(
    mut session: RoomSession,
    owner: &Handle,
    rights: Rights,
    deadline: Duration,
) -> Result<ReplicatedAdmission> {
    crate::transport::validate_duration(deadline, Duration::from_secs(300))?;
    let peer = session.peer().clone();
    tokio::time::timeout(deadline, async {
        session
            .respond(EnrollmentStage::Authority, deadline, |_| async {
                owner
                    .issue_enrollment_authority(peer.clone(), rights, deadline)
                    .await?
                    .encode()
            })
            .await?;
        let mut completed = None;
        session
            .respond(EnrollmentStage::KeyPackage, deadline, |bytes| {
                let completed = &mut completed;
                let peer = &peer;
                async move {
                    let offer: PackageOffer =
                        serde_json::from_slice(&bytes).map_err(|_| Error::Protocol)?;
                    if offer.endpoint.port() == 0
                        || offer.endpoint.ip().is_unspecified()
                        || offer.endpoint.ip().is_multicast()
                    {
                        return Err(Error::Protocol);
                    }
                    let proposal = owner
                        .prepare_scoped_admission(peer.clone(), offer.package, deadline)
                        .await?;
                    let receipt = owner.submit_prepared(deadline).await?;
                    if !receipt.accepted {
                        return Err(Error::Denied);
                    }
                    owner
                        .provision_scoped_learner(
                            peer.clone(),
                            Node::for_peer(&peer.peer, Some(offer.endpoint)),
                            deadline,
                        )
                        .await?;
                    let reply = ReplicatedAdmission {
                        version: 1,
                        room: peer.room,
                        subject: peer.peer.fingerprint_bytes(),
                        admission: proposal.id()?,
                    };
                    let bytes = serde_json::to_vec(&reply).map_err(|_| Error::Protocol)?;
                    *completed = Some(reply);
                    Ok(bytes)
                }
            })
            .await?;
        session.close().await?;
        completed.ok_or(Error::Protocol)
    })
    .await
    .map_err(|_| Error::Timeout)?
}

/// Create keys ONLY on this recipient after authority/policy validation. The
/// returned !Send Room remains caller-owned; no private daemon is auto-spawned.
pub async fn prepare(
    mut session: RoomSession,
    identity: Arc<Identity>,
    options: CandidateOptions,
) -> Result<Prepared> {
    crate::transport::validate_duration(options.deadline, Duration::from_secs(300))?;
    if session.peer().local_identity != identity.peer().fingerprint() {
        return Err(Error::Authentication);
    }
    let inviter = session.peer().peer.clone();
    let expected_room = session.peer().room;
    let deadline = options.deadline;
    let expires = tokio::time::Instant::now() + deadline;
    let authority = tokio::time::timeout(deadline, async {
        let bytes = session
            .request(
                EnrollmentStage::Authority,
                b"public room authority",
                deadline,
            )
            .await?;
        let raw = Authority::decode(&bytes)?;
        let authority = raw.verify(expected_room, &inviter, &identity.peer(), options.rights)?;
        let peers = authority.control_peers(&identity)?;
        let profile = Registry::open(options.profile.clone())?;
        profile.authorize_peer(&inviter, &identity, Action::Rooms, true)?;
        for peer in peers.values() {
            profile.authorize_peer(peer, &identity, Action::Rooms, true)?;
        }
        Ok::<_, Error>(authority)
    })
    .await
    .map_err(|_| Error::Timeout)??;
    Ok(Prepared {
        session,
        identity,
        options,
        authority,
        inviter,
        expires,
    })
}

pub async fn join(
    session: RoomSession,
    identity: Arc<Identity>,
    options: CandidateOptions,
) -> Result<Joined> {
    prepare(session, identity, options).await?.join().await
}

impl Prepared {
    pub fn authority(&self) -> &VerifiedAuthority {
        &self.authority
    }
    pub fn inviter(&self) -> &Peer {
        &self.inviter
    }
    pub async fn join(self) -> Result<Joined> {
        self.join_until(std::future::pending()).await
    }
    /// Cancellation is handled INSIDE the owner, so even a canceled join awaits
    /// its public store/transport cleanup. Already committed effects remain real.
    pub async fn join_until(
        self,
        cancel: impl std::future::Future<Output = Result<()>>,
    ) -> Result<Joined> {
        let Self {
            mut session,
            identity,
            options,
            authority,
            inviter,
            expires,
        } = self;
        let deadline = expires.saturating_duration_since(tokio::time::Instant::now());
        if deadline.is_zero() {
            return Err(Error::Timeout);
        }
        let expected_room = authority.anchor().id();
        let peers = authority.control_peers(&identity)?;
        let profile = Registry::open(options.profile)?;
        profile.authorize_peer(&inviter, &identity, Action::Rooms, true)?;
        for peer in peers.values() {
            profile.authorize_peer(peer, &identity, Action::Rooms, true)?;
        }
        let mut started = None;
        // Keep the composed SDK/bootstrap future out of every caller's stack
        // layout without changing its single-owner cancellation boundary.
        let operation = Box::pin(tokio::time::timeout(deadline, async {
            let mut candidate = authority.create_candidate(&identity, &options.private_state)?;
            let package = candidate.key_package()?;
            let control = Arc::new(
                ControlNode::start(
                    identity.clone(),
                    &options.control_state,
                    Settings {
                        anchor: authority.anchor().clone(),
                        listen: options.listen,
                        peers,
                        timing: network::configuration()?,
                    },
                    Mode::Create(authority.creator().clone()),
                )
                .await?,
            );
            started = Some(control.clone());
            let bytes = serde_json::to_vec(&PackageOffer {
                package,
                endpoint: control.local_addr()?,
            })
            .map_err(|_| Error::Protocol)?;
            let response = session
                .request(EnrollmentStage::KeyPackage, &bytes, deadline)
                .await?;
            let receipt: ReplicatedAdmission =
                serde_json::from_slice(&response).map_err(|_| Error::Protocol)?;
            if receipt.version != 1
                || receipt.room != expected_room
                || receipt.subject != identity.peer().fingerprint_bytes()
            {
                return Err(Error::Authentication);
            }
            let (_, record) = control
                .receive_welcome(deadline)
                .await?
                .ok_or(Error::Node("admission not available"))?;
            if record.proposal_id() != Some(receipt.admission) {
                return Err(Error::Authentication);
            }
            let room = Room::join(
                identity,
                control.clone(),
                candidate,
                &inviter,
                profile,
                deadline,
            )
            .await?;
            session.close().await?;
            Ok(Joined {
                room,
                control,
                admission: receipt.admission,
            })
        }));
        let result = tokio::select! {
            biased;
            canceled = cancel => match canceled { Ok(()) => Err(Error::Cancelled), Err(error) => Err(error) },
            result = operation => result.unwrap_or(Err(Error::Timeout)),
        };
        if result.is_err()
            && let Some(control) = started.take()
        {
            match Arc::try_unwrap(control) {
                Ok(node) => {
                    if let Err(error) = node.shutdown().await {
                        eprintln!("Enrollment control cleanup failed: {error}");
                    }
                }
                Err(_) => eprintln!("Enrollment control cleanup failed: active owner remains"),
            }
        }
        result
    }
}
