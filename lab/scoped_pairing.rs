//! Real scoped bootstrap adapter; approvals are NOT MLS/controller membership.
//! Ephemeral invitation transfer is intentionally out-of-band and never logged.
use super::{Context, anchor, output_json, publish_json};
use kk::{
    Error, Identity, Peer, Result,
    pairing::{self, Invite, PairServer, Proposal, RoomJoinOptions, RoomPeer, Scope},
};
use serde::Deserialize;
use serde_json::json;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listen {
    context: Context,
    listen: SocketAddr,
    expected_certificate: PathBuf,
    invite: PathBuf,
    approval: PathBuf,
    result: PathBuf,
    status: PathBuf,
    data_port: u16,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Join {
    context: Context,
    expected_certificate: PathBuf,
    invite: PathBuf,
    approval: PathBuf,
    result: PathBuf,
    data_port: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    identity: PathBuf,
    invite: PathBuf,
    role: String,
    private_state: PathBuf,
    control_state: PathBuf,
    profile: PathBuf,
    listen: SocketAddr,
    status: PathBuf,
    config_out: PathBuf,
    result: PathBuf,
}

/// Thin recipient process: authority/grants/seeds/KeyPackage use the proved
/// network session, never host-copied public bootstrap artifacts.
pub async fn enroll_candidate(config: Candidate) -> Result<()> {
    let identity = std::sync::Arc::new(Identity::load(&config.identity)?);
    let invite = Invite::load(&config.invite).await?;
    let Scope::Room(room) = invite.scope else {
        return Err(Error::Pairing);
    };
    let expected = invite.fingerprint.clone();
    let options = RoomJoinOptions {
        room,
        address: invite.address,
        expected_id: Some(invite.id),
        expected_fingerprint: Some(expected.clone()),
        code: invite.code().clone(),
        data_port: config.listen.port(),
        deadline: Duration::from_secs(90),
    };
    let session = pairing::join_room_session(options, &identity, |offer| async move {
        Ok(offer.scope == Scope::Room(room) && offer.fingerprint == expected)
    }, |_| Ok(())).await?;
    let joined = kk::rooms::enrollment::join(
        session,
        identity,
        kk::rooms::enrollment::CandidateOptions {
            private_state: config.private_state,
            control_state: config.control_state.clone(),
            profile: config.profile,
            listen: config.listen,
            rights: super::role_rights(&config.role)?,
            deadline: Duration::from_secs(90),
        },
    )
    .await?;
    let prepared = async {
        let parent = config.config_out.parent().ok_or(Error::Protocol)?;
        let directory = parent.join("approved");
        kk::profile::private_directory(&directory, true)?;
        let root = joined.room.root_bundle();
        let anchor = root.verify_id(room)?;
        let original_creator = anchor.verify_chain(&[])?.peer();
        let creator_id = kk::rooms::control::raft::PeerId::from_peer(&original_creator);
        let creator_certificate = directory.join(format!("{creator_id}.der"));
        super::output(&creator_certificate, original_creator.certificate_der())?;
        let genesis = directory.join("genesis.json");
        output_json(&genesis, &super::Genesis { root, creator: joined.control.public_origin().await? })?;
        let scoped = joined.control.approved_control_peers()?;
        let membership = joined.control.control_membership();
        let mut peers = Vec::new();
        for (id, node) in membership.membership().nodes() {
            let peer = scoped.get(id).ok_or(Error::Authentication)?;
            let certificate = directory.join(format!("{id}.der"));
            if *id != creator_id { super::output(&certificate, peer.certificate_der())?; }
            peers.push(super::PeerFile { certificate, endpoint: node.endpoint.ok_or(Error::Protocol)? });
        }
        let node = super::NodeConfig { context: Context { identity: config.identity, genesis, creator_certificate }, peers,
            listen: joined.control.local_addr()?, state: config.control_state, status: config.status, initialize: false, resume: true };
        output_json(&config.config_out, &node)?;
        Ok::<_, Error>(json!({"ok": true, "room": room, "admission": joined.admission, "epoch": joined.room.epoch(),
            "voters": membership.membership().voter_ids().collect::<Vec<_>>(), "member": joined.control.id()}))
    }.await;
    drop(joined.room);
    let shutdown = match std::sync::Arc::try_unwrap(joined.control) {
        Ok(node) => node.shutdown().await,
        Err(_) => Err(Error::Node("enrollment fixture leaked a control owner")),
    };
    if let Err(error) = &shutdown {
        eprintln!("Enrollment fixture shutdown failed: {error}");
    }
    let value = prepared?;
    shutdown?;
    output_json(&config.result, &value)
}

fn approval(path: &Path, peer: RoomPeer) -> Result<()> {
    output_json(
        path,
        &json!({"kind": "scoped-bootstrap-only", "room": peer.room, "peer": peer.peer.fingerprint(),
        "local_identity": peer.local_identity, "endpoint": peer.endpoint}),
    )
}
fn completed(path: &Path, proposal: Proposal) -> Result<()> {
    let Scope::Room(room) = proposal.scope else {
        return Err(Error::Pairing);
    };
    output_json(
        path,
        &json!({"kind": "scoped-bootstrap-only", "room": room, "peer": proposal.fingerprint, "endpoint": proposal.endpoint}),
    )
}
pub async fn listen(config: Listen) -> Result<()> {
    let (anchor, _) = anchor(&config.context)?;
    let room = anchor.id();
    let identity = Identity::load(&config.context.identity)?;
    let expected = Peer::load(&config.expected_certificate)?.fingerprint();
    let server = PairServer::bind_room(config.listen, Duration::from_secs(60), room).await?;
    Invite::from_server(&server, &identity)?.export(&config.invite)?;
    publish_json(
        &config.status,
        &json!({"pid": std::process::id(), "room": room, "id": server.id(), "ready": true}),
        true,
    )?;
    let proposal =
        server
            .run_room(
                &identity,
                config.data_port,
                |offer| async move {
                    Ok(offer.scope == Scope::Room(room) && offer.fingerprint == expected)
                },
                |peer| approval(&config.approval, peer),
            )
            .await?;
    completed(&config.result, proposal)
}
pub async fn join(config: Join) -> Result<()> {
    let (anchor, _) = anchor(&config.context)?;
    let room = anchor.id();
    let identity = Identity::load(&config.context.identity)?;
    let expected = Peer::load(&config.expected_certificate)?.fingerprint();
    let invite = Invite::load(&config.invite).await?;
    if invite.scope != Scope::Room(room) || invite.fingerprint != expected {
        return Err(Error::Pairing);
    }
    let options = RoomJoinOptions {
        room,
        address: invite.address,
        expected_id: Some(invite.id),
        expected_fingerprint: Some(expected.clone()),
        code: invite.code().clone(),
        data_port: config.data_port,
        deadline: Duration::from_secs(40),
    };
    let proposal = pairing::join_room(options, &identity, |offer| async move {
        Ok(offer.scope == Scope::Room(room) && offer.fingerprint == expected)
    }, |peer| approval(&config.approval, peer)).await?;
    completed(&config.result, proposal)
}
