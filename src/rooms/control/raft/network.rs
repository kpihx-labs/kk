//! Actual QUIC/mutual-TLS Raft RPC. Peers are explicitly authorized in this room
//! control scope; a certificate here does not become a global contact.
use super::{ApplyResult, Node, PeerId, TypeConfig};
use crate::{Error, Identity, Peer, Result, config, transport};
use openraft::error::PayloadTooLarge;
use openraft::error::{InstallSnapshotError, RPCError, RaftError, RemoteError, Unreachable};
use openraft::network::RPCOption;
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use openraft::{Raft, RaftNetwork, RaftNetworkFactory};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::time::Duration;
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{
        Arc, RwLock, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tokio::{
    sync::{Mutex, Semaphore},
    task::JoinSet,
};

const ALPN: &[u8] = b"kk-room-control/1";
mod file;
mod media;
mod profile;
mod replica;
pub use media::{
    Frame as MediaFrame, Publication as MediaPublication, Status as MediaStatus,
    Subscription as MediaSubscription,
};
pub use profile::ProfileMetadata;

pub fn configuration() -> Result<openraft::Config> {
    openraft::Config {
        heartbeat_interval: 250,
        election_timeout_min: 1000,
        election_timeout_max: 2000,
        max_payload_entries: 32,
        snapshot_max_chunk_size: config::ROOM_CONTROL_SNAPSHOT_CHUNK as u64,
        ..Default::default()
    }
    .validate()
    .map_err(|_| Error::Configuration("invalid control timing"))
}
type Slot = Arc<Mutex<Option<Cached>>>;
#[derive(Clone)]
struct Cached {
    address: SocketAddr,
    connection: quinn::Connection,
}
struct Shared {
    room: [u8; 32],
    identity: Arc<Identity>,
    endpoint: quinn::Endpoint,
    drivers: transport::DriverOwner,
    peers: RwLock<BTreeMap<PeerId, Peer>>,
    slots: Mutex<BTreeMap<PeerId, Slot>>,
    room_port: RwLock<Option<crate::rooms::runtime::Handle>>,
    profile_source: RwLock<profile::SourceState>,
    profile_cache: std::sync::Mutex<profile::Cache>,
    #[cfg(all(test, unix))]
    profile_publication_pause: std::sync::Mutex<Option<profile::PublicationPause>>,
    room_data: Arc<Semaphore>,
    room_files: RwLock<file::Consent>,
    file_shares: RwLock<replica::Registry>,
    file_data: Arc<Semaphore>,
    writes: Mutex<()>,
    scope_updates: Mutex<Option<openraft::LogId<PeerId>>>,
    media_epoch: AtomicU64,
    media: Arc<media::Hub>,
    explicit_scope: AtomicBool,
    stopping: AtomicBool,
    bootstrap_controllers: std::collections::BTreeSet<PeerId>,
}
pub struct Factory {
    shared: Arc<Shared>,
    // The original bound handle owns the stop boundary. Raft, scope and request
    // clones borrow it and must not revoke consent when a temporary clone drops.
    owner: bool,
}
impl Clone for Factory {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
            owner: false,
        }
    }
}
impl Drop for Factory {
    fn drop(&mut self) {
        if self.owner {
            self.close();
        }
    }
}
pub(super) struct ScopeWitness(Weak<Shared>);
impl ScopeWitness {
    pub fn owners(&self) -> usize {
        self.0.strong_count()
    }
}
pub struct Server {
    shared: Arc<Shared>,
}
pub struct Client {
    // OpenRaft may retain a detached/backoff client after core shutdown. A
    // client is a borrower, not an owner of this node's listener and socket.
    shared: Weak<Shared>,
    target: PeerId,
    node: Node,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    room: [u8; 32],
    request: Request,
}
#[derive(Serialize, Deserialize)]
enum Request {
    Submit(super::super::Proposal),
    ReadBarrier,
    Configuration,
    RoomText(Vec<u8>),
    RoomFile,
    ProfileGet,
    ConversationCapabilities,
    FileAvailability([u8; 32]),
    FileFetch([u8; 32]),
    MediaOffer(media::Offer),
    MediaEnd([u8; 16]),
    WelcomeForSelf,
    NodeIntent(super::node_intent::Request),
    Voters(super::voters::Request),
    Append(AppendEntriesRequest<TypeConfig>),
    Vote(VoteRequest<PeerId>),
    Install(InstallSnapshotRequest<TypeConfig>),
}
#[derive(Serialize, Deserialize)]
enum Response {
    ConversationCapabilities(crate::rooms::conversation::Capabilities),
    Submit(Submission),
    ReadBarrier(Barrier),
    Configuration(Reply<openraft::StoredMembership<PeerId, Node>>),
    RoomText(DataReply),
    RoomFile(DataReply),
    FileAvailability(Option<crate::files::FileInfo>),
    Media(DataReply),
    Welcome(Reply<Option<Box<Delivery>>>),
    NodeIntent(Reply<super::node_intent::Applied>),
    Voters(Reply<super::voters::Applied>),
    Append(std::result::Result<AppendEntriesResponse<PeerId>, RaftError<PeerId>>),
    Vote(std::result::Result<VoteResponse<PeerId>, RaftError<PeerId>>),
    Install(
        std::result::Result<
            InstallSnapshotResponse<PeerId>,
            RaftError<PeerId, InstallSnapshotError>,
        >,
    ),
}
#[derive(Serialize, Deserialize)]
enum DataReply {
    Accepted([u8; 32]),
    Denied,
    Busy,
    Timeout,
    Rejected,
}

/// Public-order response only, never a claim that private room state or the new
/// recipient is ready. Exact retries recover historical receipts, marked replayed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Reply<T> {
    Applied(T),
    Forward {
        leader: Option<PeerId>,
        node: Option<Node>,
    },
    Unavailable,
    StateUnavailable,
    Denied,
    Conflict,
}
/// Remote position is a hint, not private-join authority. The recipient must
/// match this envelope against its OWN protected committed control record.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delivery {
    pub artifact: super::super::Welcome,
    pub position: super::Position,
}
pub type Submission = Reply<ApplyResult>;
pub type Barrier = Reply<Option<openraft::LogId<PeerId>>>;
impl<T> Reply<T> {
    pub(super) fn map<U>(self, transform: impl FnOnce(T) -> U) -> Reply<U> {
        match self {
            Self::Applied(value) => Reply::Applied(transform(value)),
            Self::Forward { leader, node } => Reply::Forward { leader, node },
            Self::Unavailable => Reply::Unavailable,
            Self::StateUnavailable => Reply::StateUnavailable,
            Self::Denied => Reply::Denied,
            Self::Conflict => Reply::Conflict,
        }
    }
}
pub(super) async fn welcome_local(
    raft: &Raft<TypeConfig>,
    machine: &super::StateMachine,
    recipient: PeerId,
) -> Reply<Option<Box<Delivery>>> {
    match applied_barrier(raft).await {
        Reply::Applied(_) => match machine.committed_welcome(recipient).await {
            Ok(None) => Reply::Applied(None),
            Ok(Some((artifact, record))) => match record.position() {
                Ok(position) => Reply::Applied(Some(Box::new(Delivery { artifact, position }))),
                Err(_) => Reply::StateUnavailable,
            },
            Err(_) => Reply::StateUnavailable,
        },
        other => other.map(|_| None),
    }
}

pub(super) async fn barrier_local(raft: &Raft<TypeConfig>) -> Barrier {
    match raft.ensure_linearizable().await {
        Ok(log) => Reply::Applied(log),
        Err(error) => match error.forward_to_leader::<Node>() {
            Some(forward) => Reply::Forward {
                leader: forward.leader_id,
                node: forward.leader_node.clone(),
            },
            None => Reply::Unavailable,
        },
    }
}

pub(super) async fn submit_local(
    raft: &Raft<TypeConfig>,
    proposal: super::super::Proposal,
) -> Submission {
    match raft.client_write(proposal).await {
        Ok(response) => Submission::Applied(response.data),
        Err(error) => match error.forward_to_leader::<Node>() {
            Some(forward) => Submission::Forward {
                leader: forward.leader_id,
                node: forward.leader_node.clone(),
            },
            None => Submission::Unavailable,
        },
    }
}
async fn submit_guarded(
    shared: &Shared,
    raft: &Raft<TypeConfig>,
    proposal: super::super::Proposal,
) -> Submission {
    let _write = shared.writes.lock().await;
    submit_local(raft, proposal).await
}
async fn node_intent_local(
    shared: &Arc<Shared>,
    raft: &Raft<TypeConfig>,
    machine: &super::StateMachine,
    request: super::node_intent::Request,
) -> Reply<super::node_intent::Applied> {
    // Both signed application writes and configuration intents pass this gate.
    // Authorization cannot race an ordered role/removal write at this leader.
    // Raft append/vote/snapshot RPCs NEVER acquire it (quorum must keep moving).
    let _write = shared.writes.lock().await;
    match applied_barrier(raft).await {
        Reply::Applied(_) => {}
        Reply::Forward { leader, node } => return Reply::Forward { leader, node },
        _ => return Reply::Unavailable,
    }
    let view = match machine.public_view().await {
        Ok(view) => view,
        Err(_) => return Reply::StateUnavailable,
    };
    let before = raft.metrics().borrow().membership_config.clone();
    let actual = before.membership();
    let target = match request.verify(shared.room, &view, actual.get_node(&request.body.target)) {
        Ok(peer) => peer,
        Err(_) => return Reply::Denied,
    };
    let id = request.body.target;
    let address = matches!(request.body.kind, super::node_intent::Kind::Address);
    if !address && actual.voter_ids().any(|voter| voter == id) {
        return Reply::Conflict;
    }
    let satisfied = actual
        .get_node(&id)
        .is_some_and(|node| node.endpoint == Some(request.body.endpoint));
    if request.body.before != *before.log_id() && !satisfied {
        return Reply::Conflict;
    }
    if actual.get_node(&id).is_none() && actual.nodes().count() >= config::ROOM_MAX_MEMBERS {
        return Reply::Conflict;
    }
    let factory = Factory {
        shared: shared.clone(),
        owner: false,
    };
    let scope = match machine.transport_scope().await {
        Ok(scope) => scope,
        Err(_) => return Reply::StateUnavailable,
    };
    if factory.refresh_scope(scope).await.is_err() {
        return Reply::StateUnavailable;
    }
    let node = Node::for_peer(&target, Some(request.body.endpoint));
    let update = if actual.get_node(&id).is_some() {
        // AddNodes deliberately preserves existing metadata. SetNodes changes
        // only the requested device record and retains the exact voter sets.
        raft.change_membership(
            openraft::ChangeMembers::SetNodes(BTreeMap::from([(id, node.clone())])),
            true,
        )
        .await
    } else {
        raft.add_learner(id, node.clone(), true).await
    };
    match update {
        Ok(response) => {
            if !response.membership.as_ref().is_some_and(|membership| {
                membership.get_node(&id) == Some(&node)
                    && membership.get_joint_config() == actual.get_joint_config()
            }) {
                return Reply::StateUnavailable;
            }
            Reply::Applied(super::node_intent::Applied {
                target: id,
                node,
                membership: response.log_id,
            })
        }
        Err(error) => match error.forward_to_leader::<Node>() {
            Some(forward) => Reply::Forward {
                leader: forward.leader_id,
                node: forward.leader_node.clone(),
            },
            None => Reply::Unavailable,
        },
    }
}

pub(super) async fn configuration_local(
    raft: &Raft<TypeConfig>,
    machine: &super::StateMachine,
    caller: PeerId,
) -> Reply<openraft::StoredMembership<PeerId, Node>> {
    match applied_barrier(raft).await {
        Reply::Applied(_) => {}
        Reply::Forward { leader, node } => return Reply::Forward { leader, node },
        _ => return Reply::Unavailable,
    }
    match machine.configuration().await {
        Ok(configuration) if configuration.membership().get_node(&caller).is_some() => {
            Reply::Applied(configuration)
        }
        Ok(_) => Reply::Denied,
        Err(_) => Reply::StateUnavailable,
    }
}

async fn applied_barrier(raft: &Raft<TypeConfig>) -> Barrier {
    let required = match barrier_local(raft).await {
        Reply::Applied(required) => required,
        reply => return reply,
    };
    match raft
        .wait(Some(config::DEFAULT_TIMEOUT))
        .applied_index_at_least(
            required.map(|log| log.index),
            "node metadata authorization view",
        )
        .await
    {
        Ok(_) => Reply::Applied(required),
        Err(_) => Reply::Unavailable,
    }
}
async fn voters_local(
    shared: &Arc<Shared>,
    raft: &Raft<TypeConfig>,
    machine: &super::StateMachine,
    request: super::voters::Request,
) -> Reply<super::voters::Applied> {
    let _write = shared.writes.lock().await;
    match applied_barrier(raft).await {
        Reply::Applied(_) => {}
        Reply::Forward { leader, node } => return Reply::Forward { leader, node },
        _ => return Reply::Unavailable,
    }
    let view = match machine.public_view().await {
        Ok(view) => view,
        Err(_) => return Reply::StateUnavailable,
    };
    if request.verify(shared.room, &view).is_err() {
        return Reply::Denied;
    }
    let current = raft.metrics().borrow().membership_config.clone();
    let actual = current.membership();
    let configs = actual.get_joint_config();
    let same = configs.last() == Some(&request.body.voters);
    // Resume ONLY the same uniform/joint target; never overwrite a different
    // in-progress transition or silently rebase an old signed configuration.
    if (request.body.before != *current.log_id() && !same) || (configs.len() > 1 && !same) {
        return Reply::Conflict;
    }
    for id in &request.body.voters {
        let Some(node) = actual.get_node(id) else {
            return Reply::Conflict;
        };
        if !node.certificate.is_empty() && node.peer(*id).is_err() {
            return Reply::Denied;
        }
    }
    match raft
        .change_membership(request.body.voters.clone(), true)
        .await
    {
        Ok(response) => Reply::Applied(super::voters::Applied {
            voters: request.body.voters,
            membership: response.log_id,
        }),
        Err(error) => match error.forward_to_leader::<Node>() {
            Some(forward) => Reply::Forward {
                leader: forward.leader_id,
                node: forward.leader_node.clone(),
            },
            None => Reply::Unavailable,
        },
    }
}
impl Factory {
    pub(super) async fn read_configuration(
        &self,
        target: PeerId,
        node: &Node,
        deadline: Duration,
    ) -> Result<Reply<openraft::StoredMembership<PeerId, Node>>> {
        transport::validate_duration(deadline, config::DEFAULT_TIMEOUT)?;
        let client = Client {
            shared: Arc::downgrade(&self.shared),
            target,
            node: node.clone(),
        };
        match client
            .exchange(Request::Configuration, RPCOption::new(deadline))
            .await?
        {
            Response::Configuration(configuration) => Ok(configuration),
            _ => Err(Error::Protocol),
        }
    }
    pub(super) fn voter_request(
        &self,
        voters: std::collections::BTreeSet<PeerId>,
        before: Option<openraft::LogId<PeerId>>,
    ) -> Result<super::voters::Request> {
        super::voters::Request::issue(&self.shared.identity, self.shared.room, voters, before)
    }
    pub(super) async fn voters_direct(
        &self,
        raft: &Raft<TypeConfig>,
        machine: &super::StateMachine,
        request: super::voters::Request,
    ) -> Reply<super::voters::Applied> {
        voters_local(&self.shared, raft, machine, request).await
    }
    pub(super) async fn voters(
        &self,
        target: PeerId,
        node: &Node,
        request: super::voters::Request,
        deadline: Duration,
    ) -> Result<Reply<super::voters::Applied>> {
        transport::validate_duration(deadline, config::DEFAULT_TIMEOUT)?;
        let client = Client {
            shared: Arc::downgrade(&self.shared),
            target,
            node: node.clone(),
        };
        match client
            .exchange(Request::Voters(request), RPCOption::new(deadline))
            .await?
        {
            Response::Voters(reply) => Ok(reply),
            _ => Err(Error::Protocol),
        }
    }
    pub(super) fn node_request(
        &self,
        target: PeerId,
        endpoint: SocketAddr,
        before: Option<openraft::LogId<PeerId>>,
    ) -> Result<super::node_intent::Request> {
        super::node_intent::Request::issue(
            &self.shared.identity,
            self.shared.room,
            target,
            endpoint,
            before,
        )
    }
    pub(super) fn address_request(
        &self,
        endpoint: SocketAddr,
        before: Option<openraft::LogId<PeerId>>,
    ) -> Result<super::node_intent::Request> {
        super::node_intent::Request::address(
            &self.shared.identity,
            self.shared.room,
            endpoint,
            before,
        )
    }
    pub(super) async fn submit_direct(
        &self,
        raft: &Raft<TypeConfig>,
        proposal: super::super::Proposal,
    ) -> Submission {
        submit_guarded(&self.shared, raft, proposal).await
    }
    pub(super) async fn node_direct(
        &self,
        raft: &Raft<TypeConfig>,
        machine: &super::StateMachine,
        request: super::node_intent::Request,
    ) -> Reply<super::node_intent::Applied> {
        node_intent_local(&self.shared, raft, machine, request).await
    }
    pub(super) async fn node_update(
        &self,
        target: PeerId,
        node: &Node,
        request: super::node_intent::Request,
        deadline: Duration,
    ) -> Result<Reply<super::node_intent::Applied>> {
        transport::validate_duration(deadline, config::DEFAULT_TIMEOUT)?;
        let client = Client {
            shared: Arc::downgrade(&self.shared),
            target,
            node: node.clone(),
        };
        match client
            .exchange(Request::NodeIntent(request), RPCOption::new(deadline))
            .await?
        {
            Response::NodeIntent(reply) => Ok(reply),
            _ => Err(Error::Protocol),
        }
    }
    pub fn allows(&self, id: PeerId) -> Result<bool> {
        authorized(&self.shared, id)
    }
    pub(super) fn approved_peers(&self) -> Result<BTreeMap<PeerId, Peer>> {
        self.shared
            .peers
            .read()
            .map(|peers| peers.clone())
            .map_err(|_| Error::Protocol)
    }
    pub(super) async fn refresh_scope(&self, scope: super::machine::TransportScope) -> Result<()> {
        let mut applied = self.shared.scope_updates.lock().await;
        if let Some(current) = applied.as_ref() {
            let Some(incoming) = scope.applied.as_ref() else {
                return Ok(());
            };
            if incoming.index < current.index {
                return Ok(());
            }
            if incoming.index == current.index && incoming != current {
                return Err(Error::Authentication);
            }
        }
        if scope.epoch < self.shared.media_epoch.load(Ordering::SeqCst) {
            return Err(Error::Authentication);
        }
        self.shared.media_epoch.store(scope.epoch, Ordering::SeqCst);
        let profile_members: Vec<_> = scope.members.iter().map(PeerId::from_peer).collect();
        self.shared
            .profile_cache
            .lock()
            .map_err(|_| Error::Protocol)?
            .observe(scope.epoch, &profile_members)?;
        // An unconfigured recipient's synthetic genesis is not a newer trust
        // roster. Retain its independently approved controller seeds until a
        // committed configuration exists, including delegated enrollment.
        if scope.applied.is_none() && scope.nodes.is_empty() {
            return Ok(());
        }
        // An explicit local roster is a veto/override, never silently widened
        // by background eligibility changes. Normal nodes follow committed scope.
        if self.shared.explicit_scope.load(Ordering::SeqCst) {
            *applied = scope.applied;
            return Ok(());
        }
        let known = self.approved_peers()?;
        let own = self.shared.identity.peer();
        let mut peers = BTreeMap::from([(PeerId::from_peer(&own), own)]);
        for (id, node) in scope.nodes {
            let peer = if node.certificate.is_empty() {
                known.get(&id).cloned().ok_or(Error::Authentication)?
            } else {
                node.peer(id)?
            };
            peers.insert(id, peer);
        }
        for peer in scope.members {
            peers.insert(PeerId::from_peer(&peer), peer);
        }
        self.install_scope(peers).await?;
        *applied = scope.applied;
        Ok(())
    }
    pub fn bind(
        room: [u8; 32],
        identity: Arc<Identity>,
        peers: BTreeMap<PeerId, Peer>,
        address: SocketAddr,
    ) -> Result<(Self, Server)> {
        validate_peers(&peers)?;
        let roots: Vec<_> = peers.values().cloned().collect();
        let config = transport::server_config(&identity, &roots, &[ALPN], transport::Policy::Room)?;
        let (endpoint, drivers) = transport::owned_server(config, address)?;
        let own = PeerId::from_peer(&identity.peer());
        let shared = Arc::new(Shared {
            bootstrap_controllers: peers
                .keys()
                .copied()
                .filter(|id| *id != PeerId::from_peer(&identity.peer()))
                .collect(),
            room,
            identity,
            endpoint,
            drivers,
            peers: RwLock::new(peers),
            slots: Mutex::new(BTreeMap::new()),
            room_port: RwLock::new(None),
            profile_source: RwLock::new(profile::SourceState::default()),
            profile_cache: std::sync::Mutex::new(profile::Cache::new(room, own)),
            #[cfg(all(test, unix))]
            profile_publication_pause: std::sync::Mutex::new(None),
            room_data: Arc::new(Semaphore::new(config::ROOM_DATA_RPCS)),
            room_files: RwLock::new(file::Consent::default()),
            file_shares: RwLock::new(replica::Registry::default()),
            file_data: Arc::new(Semaphore::new(config::NODE_MAX_FILES)),
            writes: Mutex::new(()),
            scope_updates: Mutex::new(None),
            media_epoch: AtomicU64::new(0),
            media: media::Hub::new(),
            explicit_scope: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
        });
        Ok((
            Self {
                shared: shared.clone(),
                owner: true,
            },
            Server { shared },
        ))
    }
    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.shared.endpoint.local_addr()?)
    }
    pub(super) fn fail_closed(&self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        self.shared
            .profile_cache
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .suspend();
        self.revoke_files();
        self.shared.endpoint.close(1u32.into(), b"");
    }
    pub(super) fn close(&self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        self.shared
            .profile_cache
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .suspend();
        self.revoke_files();
        self.shared.media.close();
        self.shared.endpoint.close(0u32.into(), b"");
    }
    pub(super) fn driver_owner(&self) -> transport::DriverOwner {
        self.shared.drivers.clone()
    }
    pub(super) fn owner_witness(&self) -> ScopeWitness {
        ScopeWitness(Arc::downgrade(&self.shared))
    }
    pub(super) async fn wait_idle(&self) -> Result<()> {
        tokio::time::timeout(
            config::QUIC_SHUTDOWN_GRACE,
            self.shared.endpoint.wait_idle(),
        )
        .await
        .map_err(|_| Error::Timeout)
    }
    pub async fn register_room(&self, handle: crate::rooms::runtime::Handle) -> Result<()> {
        let status = handle.ready(config::DEFAULT_TIMEOUT).await?;
        if status.room != self.shared.room
            || status.owner != PeerId::from_peer(&self.shared.identity.peer())
        {
            return Err(Error::Authentication);
        }
        let mut port = self.shared.room_port.write().map_err(|_| Error::Protocol)?;
        if port.is_some() {
            return Err(Error::Configuration(
                "private room endpoint already registered",
            ));
        }
        self.shared
            .profile_cache
            .lock()
            .map_err(|_| Error::Protocol)?
            .register(status.epoch, &status.members)?;
        *port = Some(handle);
        Ok(())
    }
    pub fn unregister_room(&self) -> Result<()> {
        self.shared
            .profile_cache
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .suspend();
        self.shared.media.invalidate_sessions();
        let result = match self.shared.room_port.write() {
            Ok(mut port) => {
                port.take();
                Ok(())
            }
            Err(poison) => {
                poison.into_inner().take();
                Err(Error::Protocol)
            }
        };
        self.revoke_files();
        result
    }
    pub async fn room_text(
        &self,
        target: PeerId,
        node: &Node,
        wire: &[u8],
        deadline: Duration,
    ) -> Result<()> {
        transport::validate_duration(deadline, config::DEFAULT_TIMEOUT)?;
        if wire.len() > config::ROOM_MAX_WIRE_BYTES {
            return Err(Error::Limit {
                resource: "room ciphertext",
                max: config::ROOM_MAX_WIRE_BYTES,
            });
        }
        let expected: [u8; 32] = Sha256::digest(wire).into();
        let client = Client {
            shared: Arc::downgrade(&self.shared),
            target,
            node: node.clone(),
        };
        match client
            .exchange(Request::RoomText(wire.to_vec()), RPCOption::new(deadline))
            .await?
        {
            Response::RoomText(DataReply::Accepted(actual)) if actual == expected => Ok(()),
            Response::RoomText(DataReply::Denied) => Err(Error::Denied),
            Response::RoomText(DataReply::Busy) => {
                Err(Error::Node("remote private room queue unavailable"))
            }
            Response::RoomText(DataReply::Timeout) => Err(Error::Timeout),
            _ => Err(Error::Protocol),
        }
    }
    pub async fn submit(
        &self,
        target: PeerId,
        node: &Node,
        proposal: super::super::Proposal,
        deadline: Duration,
    ) -> Result<Submission> {
        transport::validate_duration(deadline, config::DEFAULT_TIMEOUT)?;
        let client = Client {
            shared: Arc::downgrade(&self.shared),
            target,
            node: node.clone(),
        };
        match client
            .exchange(Request::Submit(proposal), RPCOption::new(deadline))
            .await?
        {
            Response::Submit(result) => Ok(result),
            _ => Err(Error::Protocol),
        }
    }
    pub async fn read_barrier(
        &self,
        target: PeerId,
        node: &Node,
        deadline: Duration,
    ) -> Result<Barrier> {
        transport::validate_duration(deadline, config::DEFAULT_TIMEOUT)?;
        let client = Client {
            shared: Arc::downgrade(&self.shared),
            target,
            node: node.clone(),
        };
        match client
            .exchange(Request::ReadBarrier, RPCOption::new(deadline))
            .await?
        {
            Response::ReadBarrier(result) => Ok(result),
            _ => Err(Error::Protocol),
        }
    }
    pub async fn welcome_for_self(
        &self,
        target: PeerId,
        node: &Node,
        deadline: Duration,
    ) -> Result<Reply<Option<Box<Delivery>>>> {
        transport::validate_duration(deadline, config::DEFAULT_TIMEOUT)?;
        let client = Client {
            shared: Arc::downgrade(&self.shared),
            target,
            node: node.clone(),
        };
        match client
            .exchange(Request::WelcomeForSelf, RPCOption::new(deadline))
            .await?
        {
            Response::Welcome(result) => {
                if let Reply::Applied(Some(delivery)) = &result
                    && (delivery.artifact.recipient
                        != PeerId::from_peer(&self.shared.identity.peer()).0
                        || delivery.artifact.wire.is_empty()
                        || delivery.artifact.wire.len() > config::ROOM_MAX_WIRE_BYTES)
                {
                    return Err(Error::Authentication);
                }
                Ok(result)
            }
            _ => Err(Error::Protocol),
        }
    }
    /// Caller supplies an already authorized room-control roster, not discovery.
    pub async fn replace_peers(&self, peers: BTreeMap<PeerId, Peer>) -> Result<()> {
        let _scope = self.shared.scope_updates.lock().await;
        validate_peers(&peers)?;
        self.shared
            .profile_cache
            .lock()
            .map_err(|_| Error::Protocol)?
            .invalidate();
        let result = self.install_scope(peers).await;
        if result.is_ok() {
            self.shared.explicit_scope.store(true, Ordering::SeqCst);
        }
        result
    }
    async fn install_scope(&self, peers: BTreeMap<PeerId, Peer>) -> Result<()> {
        validate_peers(&peers)?;
        let roots: Vec<_> = peers.values().cloned().collect();
        let config = transport::server_config(
            &self.shared.identity,
            &roots,
            &[ALPN],
            transport::Policy::Room,
        )?;
        *self.shared.peers.write().map_err(|_| Error::Protocol)? = peers;
        self.shared.endpoint.set_server_config(Some(config));
        let mut slots = self.shared.slots.lock().await;
        for (id, slot) in slots.iter() {
            if !authorized(&self.shared, *id)?
                && let Ok(mut cached) = slot.try_lock()
                && let Some(cached) = cached.take()
            {
                cached.connection.close(1u32.into(), b"");
            }
        }
        slots.retain(|id, _| authorized(&self.shared, *id).unwrap_or(false));
        Ok(())
    }
}
fn validate_peers(peers: &BTreeMap<PeerId, Peer>) -> Result<()> {
    if peers.is_empty()
        || peers.len() > config::ROOM_TRANSPORT_PEERS
        || peers
            .iter()
            .any(|(id, peer)| *id != PeerId::from_peer(peer))
    {
        return Err(Error::Configuration(
            "control roster must bind bounded exact certificate fingerprints",
        ));
    }
    Ok(())
}
fn authorized(shared: &Shared, id: PeerId) -> Result<bool> {
    Ok(shared
        .peers
        .read()
        .map_err(|_| Error::Protocol)?
        .contains_key(&id))
}
fn may_lead(shared: &Shared, raft: &Raft<TypeConfig>, id: PeerId) -> bool {
    let membership = raft.metrics().borrow().membership_config.clone();
    if membership.membership().voter_ids().any(|voter| voter == id) {
        return true;
    }
    // Empty learners accept initial state only from independently approved
    // controller seeds, not merely from another introduced data member.
    membership.membership().voter_ids().next().is_none()
        && shared.bootstrap_controllers.contains(&id)
}
fn may_vote(shared: &Shared, raft: &Raft<TypeConfig>, id: PeerId) -> bool {
    let membership = raft.metrics().borrow().membership_config.clone();
    let configured = membership.membership();
    if configured.voter_ids().next().is_none() {
        // Bootstrap elections precede the first replicated configuration. Only
        // an independently approved controller seed can request this vote; an
        // unconfigured learner's response is not counted unless actually chosen.
        return shared.bootstrap_controllers.contains(&id);
    }
    configured.voter_ids().any(|voter| voter == id)
        && configured
            .voter_ids()
            .any(|voter| voter == PeerId::from_peer(&shared.identity.peer()))
}
impl RaftNetworkFactory<TypeConfig> for Factory {
    type Network = Client;
    async fn new_client(&mut self, target: PeerId, node: &Node) -> Client {
        Client {
            shared: Arc::downgrade(&self.shared),
            target,
            node: node.clone(),
        }
    }
}
struct Attempt {
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    complete: bool,
}
#[derive(Clone, Copy)]
enum ExchangePolicy {
    Required,
    OptionalProfile,
}
impl ExchangePolicy {
    fn response_limit(self) -> usize {
        match self {
            Self::Required => config::ROOM_CONTROL_RPC_BYTES,
            Self::OptionalProfile => config::ROOM_PROFILE_WIRE_BYTES,
        }
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        if !self.complete {
            // Each bounded RPC owns only its streams. Retiring a shared peer
            // connection here also cancels unrelated votes/data on other lanes.
            // Peer-wide authentication/revocation still closes connections at
            // its actual owner, not at a single interrupted operation.
            let _ = self.send.reset(1u32.into());
            let _ = self.recv.stop(1u32.into());
        }
    }
}
impl Client {
    async fn connection(&self, shared: &Shared) -> Result<quinn::Connection> {
        let peer = shared
            .peers
            .read()
            .map_err(|_| Error::Protocol)?
            .get(&self.target)
            .cloned()
            .ok_or(Error::Denied)?;
        let address = self
            .node
            .endpoint
            .ok_or(Error::Configuration("control endpoint hint missing"))?;
        let slot = shared
            .slots
            .lock()
            .await
            .entry(self.target)
            .or_insert_with(|| Arc::new(Mutex::new(None)))
            .clone();
        let mut slot = slot.lock().await;
        if let Some(cached) = slot.as_ref().filter(|cached| {
            cached.address == address && cached.connection.close_reason().is_none()
        }) {
            return Ok(cached.connection.clone());
        }
        let config =
            transport::client_config(&shared.identity, &peer, ALPN, transport::Policy::Room)?;
        let connection = shared
            .endpoint
            .connect_with(config, address, config::TLS_NAME)
            .map_err(|_| Error::Network)?
            .await
            .map_err(crate::error::connection_error)?;
        transport::verify_peer(&connection, &peer)?;
        if transport::protocol(&connection)? != ALPN {
            return Err(Error::Protocol);
        }
        *slot = Some(Cached {
            address,
            connection: connection.clone(),
        });
        Ok(connection)
    }
    async fn exchange(&self, request: Request, option: RPCOption) -> Result<Response> {
        Ok(self.exchange_on(request, option).await?.0)
    }
    async fn exchange_on(
        &self,
        request: Request,
        option: RPCOption,
    ) -> Result<(Response, quinn::Connection)> {
        self.exchange_policy(request, option, ExchangePolicy::Required)
            .await
    }
    async fn exchange_policy<T: DeserializeOwned>(
        &self,
        request: Request,
        option: RPCOption,
        policy: ExchangePolicy,
    ) -> Result<(T, quinn::Connection)> {
        let shared = self.shared.upgrade().ok_or(Error::Cancelled)?;
        if shared.stopping.load(Ordering::SeqCst) {
            return Err(Error::Cancelled);
        }
        let deadline = option.soft_ttl().min(config::DEFAULT_TIMEOUT);
        let mut used = None;
        let result = tokio::time::timeout(deadline, async {
            let connection = self.connection(&shared).await?;
            if shared.stopping.load(Ordering::SeqCst) {
                return Err(Error::Cancelled);
            }
            if !authorized(&shared, self.target)? {
                connection.close(1u32.into(), b"");
                return Err(Error::Denied);
            }
            used = Some(connection.stable_id());
            let (send, recv) = connection.open_bi().await.map_err(|_| Error::Network)?;
            let mut attempt = Attempt {
                send,
                recv,
                complete: false,
            };
            let priority = match &request {
                Request::Vote(_) => 1,
                Request::Append(_) => 0,
                Request::Submit(_) => 0,
                Request::ReadBarrier => 1,
                Request::Configuration => 1,
                Request::RoomText(_) => -1,
                Request::ProfileGet | Request::ConversationCapabilities => -1,
                Request::RoomFile | Request::FileFetch(_) => -2,
                Request::FileAvailability(_) => -1,
                Request::MediaOffer(_) | Request::MediaEnd(_) => 0,
                Request::WelcomeForSelf => 0,
                Request::NodeIntent(_) => 0,
                Request::Voters(_) => 0,
                Request::Install(_) => -1,
            };
            attempt
                .send
                .set_priority(priority)
                .map_err(|_| Error::Network)?;
            send_frame(
                &mut attempt.send,
                &Envelope {
                    room: shared.room,
                    request,
                },
            )
            .await?;
            attempt.send.finish().map_err(|_| Error::Network)?;
            let response = match policy {
                ExchangePolicy::Required => read_frame(&mut attempt.recv).await?,
                ExchangePolicy::OptionalProfile => {
                    read_frame_limit(&mut attempt.recv, policy.response_limit()).await?
                }
            };
            attempt.complete = true;
            Ok((response, connection))
        })
        .await
        .unwrap_or(Err(Error::Timeout));
        if matches!(policy, ExchangePolicy::Required)
            && matches!(&result, Err(Error::Timeout | Error::Network))
        {
            self.retire_unresponsive(&shared, used);
        }
        result
    }
    fn retire_unresponsive(&self, shared: &Shared, used: Option<usize>) {
        let Some(used) = used else {
            return;
        };
        // Cache eviction is NOT peer-connection closure: independent users keep
        // their streams, while future RPCs can reauthenticate after a reboot.
        // Never wait beyond the operation deadline or evict a newer connection.
        let Ok(slots) = shared.slots.try_lock() else {
            return;
        };
        let Some(slot) = slots.get(&self.target) else {
            return;
        };
        let Ok(mut cached) = slot.try_lock() else {
            return;
        };
        if cached
            .as_ref()
            .is_some_and(|cached| cached.connection.stable_id() == used)
        {
            cached.take();
        }
    }
    fn unreachable<T>(&self) -> RPCError<PeerId, Node, RaftError<PeerId, T>>
    where
        T: std::error::Error,
    {
        RPCError::Unreachable(Unreachable::new(&std::io::Error::other(
            "authenticated control RPC unavailable",
        )))
    }
}
impl RaftNetwork<TypeConfig> for Client {
    async fn append_entries(
        &mut self,
        request: AppendEntriesRequest<TypeConfig>,
        option: RPCOption,
    ) -> std::result::Result<AppendEntriesResponse<PeerId>, RPCError<PeerId, Node, RaftError<PeerId>>>
    {
        let count = request.entries.len();
        match self.exchange(Request::Append(request), option).await {
            Ok(Response::Append(value)) => {
                value.map_err(|error| RPCError::RemoteError(RemoteError::new(self.target, error)))
            }
            Err(Error::Limit { .. }) => Err(RPCError::PayloadTooLarge(
                PayloadTooLarge::new_entries_hint((count / 2).max(1) as u64),
            )),
            _ => Err(self.unreachable()),
        }
    }
    async fn vote(
        &mut self,
        request: VoteRequest<PeerId>,
        option: RPCOption,
    ) -> std::result::Result<VoteResponse<PeerId>, RPCError<PeerId, Node, RaftError<PeerId>>> {
        match self.exchange(Request::Vote(request), option).await {
            Ok(Response::Vote(value)) => {
                value.map_err(|error| RPCError::RemoteError(RemoteError::new(self.target, error)))
            }
            _ => Err(self.unreachable()),
        }
    }
    async fn install_snapshot(
        &mut self,
        request: InstallSnapshotRequest<TypeConfig>,
        option: RPCOption,
    ) -> std::result::Result<
        InstallSnapshotResponse<PeerId>,
        RPCError<PeerId, Node, RaftError<PeerId, InstallSnapshotError>>,
    > {
        match self.exchange(Request::Install(request), option).await {
            Ok(Response::Install(value)) => {
                value.map_err(|error| RPCError::RemoteError(RemoteError::new(self.target, error)))
            }
            _ => Err(self.unreachable()),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        self.shared
            .room_files
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .revoke();
        self.shared.endpoint.close(0u32.into(), b"");
    }
}
impl Server {
    pub async fn run(self, raft: Raft<TypeConfig>, machine: super::StateMachine) -> Result<()> {
        if machine.room_id() != self.shared.room {
            return Err(Error::Authentication);
        }
        let connections = Arc::new(Semaphore::new(config::ROOM_CONTROL_CONNECTIONS));
        let requests = Arc::new(Semaphore::new(config::ROOM_CONTROL_RPCS));
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                incoming = self.shared.endpoint.accept() => {
                    let Some(incoming) = incoming else { break; };
                    let Ok(permit) = connections.clone().try_acquire_owned() else { incoming.refuse(); continue; };
                    let shared = self.shared.clone(); let raft = raft.clone(); let requests = requests.clone(); let machine = machine.clone();
                    tasks.spawn(async move { let _permit = permit; let _ = serve_connection(shared, raft, machine, incoming, requests).await; });
                },
                _ = tasks.join_next(), if !tasks.is_empty() => {}
            }
        }
        while tasks.join_next().await.is_some() {}
        Ok(())
    }
}
async fn serve_connection(
    shared: Arc<Shared>,
    raft: Raft<TypeConfig>,
    machine: super::StateMachine,
    incoming: quinn::Incoming,
    requests: Arc<Semaphore>,
) -> Result<()> {
    let connection = tokio::time::timeout(config::DEFAULT_TIMEOUT, incoming)
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(crate::error::connection_error)?;
    let id = PeerId::from_peer(&transport::peer_identity(&connection)?);
    let connection_id = connection.stable_id();
    if !authorized(&shared, id)? || transport::protocol(&connection)? != ALPN {
        connection.close(1u32.into(), b"");
        return Err(Error::Denied);
    }
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            stream = connection.accept_bi() => {
                let Ok((mut send, mut recv)) = stream else { break; };
                #[cfg(test)]
                file::trace(&shared, send.id(), "admitted");
                if !authorized(&shared, id)? { connection.close(1u32.into(), b""); break; }
                let Ok(permit) = requests.clone().try_acquire_owned() else { let _ = send.reset(1u32.into()); continue; };
                let shared = shared.clone(); let raft = raft.clone(); let machine = machine.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let header = async {
                        let envelope: Envelope = read_prefix(&mut recv).await?;
                        // Control messages require FIN. File streams continue
                        // with the existing binary offer after their ready reply.
                        if !matches!(envelope.request, Request::RoomFile | Request::FileFetch(_)) && recv.read(&mut [0]).await?.is_some() {
                            return Err(Error::Protocol);
                        }
                        Ok::<_, Error>(envelope)
                    };
                    let envelope = match tokio::time::timeout(config::DEFAULT_TIMEOUT, header).await {
                        Ok(Ok(envelope)) => envelope,
                        failure => {
                            #[cfg(test)]
                            file::trace(&shared, send.id(), match failure { Ok(Err(error)) => error.code(), _ => "header TIMEOUT" });
                            #[cfg(not(test))]
                            let _ = failure;
                            return;
                        }
                    };
                    #[cfg(test)]
                    file::trace(&shared, send.id(), "envelope parsed");
                    let deadline = match envelope.request {
                        Request::RoomFile | Request::FileFetch(_) => config::DEFAULT_FILE_TIMEOUT,
                        Request::ProfileGet | Request::ConversationCapabilities => config::ROOM_PROFILE_TIMEOUT,
                        _ => config::DEFAULT_TIMEOUT,
                    };
                    let _ = tokio::time::timeout(deadline, async {
                        if envelope.room != shared.room || !authorized(&shared, id)? { return Err(Error::Denied); }
                        let response = match envelope.request {
                            Request::ConversationCapabilities => {
                                drop(_permit);
                                let (handle, peer) = profile::port(&shared, id)?;
                                let epoch = handle.authorize_profile_metadata(peer, deadline).await?;
                                let _data = shared.room_data.clone().try_acquire_owned().map_err(|_| Error::Denied)?;
                                send_frame(&mut send, &Response::ConversationCapabilities(crate::rooms::conversation::Capabilities {
                                    epoch, version:crate::rooms::conversation::VERSION,
                                })).await?;
                                send.finish().map_err(|_| Error::Network)?;
                                return Ok(());
                            },
                            Request::ProfileGet => {
                                // Metadata authorization synchronizes the private room
                                // through quorum RPCs; never occupy that control lane.
                                drop(_permit);
                                let (metadata, _profile_work) = profile::disclose(&shared, id).await?;
                                metadata.send(&shared, &mut send).await?;
                                send.finish().map_err(|_| Error::Network)?;
                                return Ok(());
                            },
                            Request::Configuration => Response::Configuration(configuration_local(&raft, &machine, id).await),
                            Request::MediaOffer(offer) => {
                                let reply = match shared.media.offer(&shared, id, connection_id, offer) {
                                    Ok(()) => DataReply::Accepted(shared.room),
                                    Err(_) => DataReply::Denied,
                                };
                                Response::Media(reply)
                            },
                            Request::MediaEnd(track) => {
                                shared.media.end(id, track);
                                Response::Media(DataReply::Accepted(shared.room))
                            },
                            Request::RoomFile => {
                                drop(_permit);
                                #[cfg(test)]
                                let stream_id = send.id();
                                let result = file::receive(&shared, id, send, recv).await;
                                #[cfg(test)]
                                file::trace(&shared, stream_id, result.as_ref().map_or_else(|error| error.code(), |_| "receive ended OK"));
                                return result;
                            },
                            Request::FileAvailability(hash) => {
                                drop(_permit);
                                let _data = shared.room_data.clone().try_acquire_owned().map_err(|_| Error::Denied)?;
                                let info = replica::advertisement(&shared, id, hash).await.ok();
                                send_frame(&mut send, &Response::FileAvailability(info)).await?;
                                send.finish().map_err(|_| Error::Network)?;
                                return Ok(());
                            },
                            Request::FileFetch(hash) => {
                                drop(_permit);
                                replica::serve(&shared, id, hash, send, recv).await?;
                                return Ok(());
                            },
                            Request::Voters(request) => {
                                if request.body.author != id { return Err(Error::Authentication); }
                                Response::Voters(voters_local(&shared, &raft, &machine, request).await)
                            },
                            Request::NodeIntent(request) => {
                                if request.body.author != id { return Err(Error::Authentication); }
                                Response::NodeIntent(node_intent_local(&shared, &raft, &machine, request).await)
                            },
                            // No recipient field is accepted from the request:
                            // the verified TLS certificate selects the artifact.
                            Request::WelcomeForSelf => Response::Welcome(welcome_local(&raft, &machine, id).await),
                            Request::RoomText(wire) => {
                                // Room processing may itself require quorum RPCs.
                                // Do not hold a control admission permit while
                                // waiting on the actor, or data can deadlock control.
                                drop(_permit);
                                let data_permit = shared.room_data.clone().try_acquire_owned();
                                if data_permit.is_err() || wire.len() > config::ROOM_MAX_WIRE_BYTES {
                                    send_frame(&mut send, &Response::RoomText(DataReply::Busy)).await?;
                                    send.finish().map_err(|_| Error::Network)?;
                                    return Ok(());
                                }
                                let _data_permit = data_permit.map_err(|_| Error::Protocol)?;
                                let port = shared.room_port.read().map_err(|_| Error::Protocol)?.clone();
                                let peer = shared.peers.read().map_err(|_| Error::Protocol)?.get(&id).cloned().ok_or(Error::Denied)?;
                                let digest: [u8; 32] = Sha256::digest(&wire).into();
                                let result = match port {
                                    Some(port) => match port.accept_text(peer, wire, config::DEFAULT_TIMEOUT).await {
                                        Ok(()) => DataReply::Accepted(digest),
                                        Err(Error::Denied | Error::Revoked | Error::Authentication) => DataReply::Denied,
                                        Err(Error::Limit { .. } | Error::Cancelled) => DataReply::Busy,
                                        Err(Error::Timeout) => DataReply::Timeout,
                                        Err(_) => DataReply::Rejected,
                                    },
                                    None => DataReply::Busy,
                                };
                                Response::RoomText(result)
                            },
                            Request::ReadBarrier => Response::ReadBarrier(barrier_local(&raft).await),
                            Request::Submit(proposal) => {
                                if proposal.author() != id.0 { return Err(Error::Authentication); }
                                Response::Submit(submit_guarded(&shared, &raft, proposal).await)
                            },
                            Request::Append(request) => {
                                if request.vote.leader_id.node_id != id { return Err(Error::Authentication); }
                                if !may_lead(&shared, &raft, id) { return Err(Error::Denied); }
                                Response::Append(raft.append_entries(request).await)
                            },
                            Request::Vote(request) => {
                                if request.vote.leader_id.node_id != id { return Err(Error::Authentication); }
                                if !may_vote(&shared, &raft, id) { return Err(Error::Denied); }
                                Response::Vote(raft.vote(request).await)
                            },
                            Request::Install(request) => {
                                if request.vote.leader_id.node_id != id || request.data.len() > config::ROOM_CONTROL_SNAPSHOT_CHUNK { return Err(Error::Authentication); }
                                if !may_lead(&shared, &raft, id) { return Err(Error::Denied); }
                                Response::Install(raft.install_snapshot(request).await)
                            }
                        };
                        send_frame(&mut send, &response).await?;
                        send.finish().map_err(|_| Error::Network)?;
                        Ok::<_, Error>(())
                    }).await;
                });
            },
            packet = connection.read_datagram() => {
                let Ok(packet) = packet else { break; };
                if authorized(&shared, id)? { shared.media.receive(&shared, id, connection_id, &packet); }
            },
            _ = tasks.join_next(), if !tasks.is_empty() => {}
        }
    }
    shared.media.disconnected(connection_id);
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    Ok(())
}
async fn send_frame(stream: &mut quinn::SendStream, value: &impl Serialize) -> Result<()> {
    let bytes = encode_frame(value)?;
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .await?;
    stream.write_all(&bytes).await?;
    Ok(())
}
struct BoundedOutput {
    bytes: Vec<u8>,
    exceeded: bool,
}
impl std::io::Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|size| size > config::ROOM_CONTROL_RPC_BYTES)
        {
            self.exceeded = true;
            return Err(std::io::Error::other("control RPC exceeds resource bound"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encode_frame(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut output = BoundedOutput {
        bytes: Vec::new(),
        exceeded: false,
    };
    if serde_json::to_writer(&mut output, value).is_err() {
        if output.exceeded {
            return Err(Error::Limit {
                resource: "control RPC",
                max: config::ROOM_CONTROL_RPC_BYTES,
            });
        }
        return Err(Error::Protocol);
    }
    Ok(output.bytes)
}
async fn read_frame<T: DeserializeOwned>(stream: &mut quinn::RecvStream) -> Result<T> {
    read_frame_limit(stream, config::ROOM_CONTROL_RPC_BYTES).await
}
async fn read_frame_limit<T: DeserializeOwned>(
    stream: &mut quinn::RecvStream,
    max: usize,
) -> Result<T> {
    let value = read_prefix_limit(stream, max).await?;
    if stream.read(&mut [0]).await?.is_some() {
        return Err(Error::Protocol);
    }
    Ok(value)
}
async fn read_prefix<T: DeserializeOwned>(stream: &mut quinn::RecvStream) -> Result<T> {
    read_prefix_limit(stream, config::ROOM_CONTROL_RPC_BYTES).await
}
async fn read_prefix_limit<T: DeserializeOwned>(
    stream: &mut quinn::RecvStream,
    max: usize,
) -> Result<T> {
    let mut length = [0; 4];
    stream.read_exact(&mut length).await?;
    let length = u32::from_be_bytes(length) as usize;
    if length > max {
        return Err(Error::Limit {
            resource: "control RPC",
            max,
        });
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|_| Error::Protocol)
}

#[cfg(all(test, unix))]
mod cancellation_tests;
#[cfg(all(test, unix))]
mod role_tests;
#[cfg(all(test, unix))]
mod tests;
