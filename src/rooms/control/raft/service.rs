//! Foreground-owned PUBLIC control service. It supplies actual ordered acceptance,
//! not a claim that private MLS state or the joining recipient is already ready.
use super::{
    ApplyResult, LogStore, Node, PeerId, StateMachine, TypeConfig,
    network::{self, Factory},
};
use crate::{
    Error, Identity, Peer, Result, config,
    rooms::{Anchor, control::Proposal, mls::PublicIdentity},
};
use openraft::Raft;
use std::{collections::BTreeMap, net::SocketAddr, path::Path, sync::Arc, time::Duration};
use tokio::task::JoinHandle;

pub struct Settings {
    pub anchor: Anchor,
    pub listen: SocketAddr,
    pub peers: BTreeMap<PeerId, Peer>,
    pub timing: openraft::Config,
}
pub enum Mode {
    Create(PublicIdentity),
    Resume,
}
enum Operation {
    Submit(Box<Proposal>),
    ReadBarrier,
    Configuration,
    WelcomeForSelf,
    NodeIntent(Box<super::node_intent::Request>),
    Voters(Box<super::voters::Request>),
}
enum Output {
    Submitted(ApplyResult),
    ReadBarrier(Option<openraft::LogId<PeerId>>),
    Configuration(openraft::StoredMembership<PeerId, Node>),
    Welcome(Option<Box<network::Delivery>>),
    NodeIntent(super::node_intent::Applied),
    Voters(super::voters::Applied),
}

pub struct ControlNode {
    id: PeerId,
    raft: Raft<TypeConfig>,
    machine: StateMachine,
    network: Factory,
    server: Option<JoinHandle<Result<()>>>,
    scope: Option<JoinHandle<Result<()>>>,
    scope_stop: Option<tokio::sync::oneshot::Sender<()>>,
    store_released: tokio::sync::watch::Receiver<bool>,
}
impl ControlNode {
    pub async fn start(
        identity: Arc<Identity>,
        directory: &Path,
        settings: Settings,
        mode: Mode,
    ) -> Result<Self> {
        let id = PeerId::from_peer(&identity.peer());
        let resuming = matches!(&mode, Mode::Resume);
        let (network, server) = Factory::bind(
            settings.anchor.id(),
            identity,
            settings.peers,
            settings.listen,
        )?;
        let (store, machine) = match mode {
            Mode::Create(creator) => {
                let store = LogStore::create(directory)?;
                let machine = StateMachine::create(store.clone(), &settings.anchor, creator)
                    .await
                    .map_err(|_| Error::Storage("cannot initialize public control state"))?;
                (store, machine)
            }
            Mode::Resume => {
                let store = LogStore::open(directory)?;
                let machine = StateMachine::open(store.clone(), &settings.anchor)
                    .await
                    .map_err(|_| Error::Storage("cannot restore public control state"))?;
                (store, machine)
            }
        };
        if resuming {
            let scope = machine
                .transport_scope()
                .await
                .map_err(|_| Error::Storage("cannot restore committed transport scope"))?;
            network.refresh_scope(scope).await?;
        }
        let mut changes = machine.subscribe_scope();
        let store_released = store.released();
        let timing = settings
            .timing
            .validate()
            .map_err(|_| Error::Configuration("invalid control configuration"))?;
        let raft = Raft::new(
            id,
            Arc::new(timing),
            network.clone(),
            store,
            machine.clone(),
        )
        .await
        .map_err(|_| Error::Storage("cannot start durable control engine"))?;
        let task = tokio::spawn(server.run(raft.clone(), machine.clone()));
        let observer = machine.clone();
        let transport = network.clone();
        let (scope_stop, mut stop) = tokio::sync::oneshot::channel();
        let scope = tokio::spawn(async move {
            let result = async {
                loop {
                    let update = tokio::select! { _ = &mut stop => false, update = changes.changed() => update.is_ok() };
                    if !update { break; }
                    let scope = observer
                        .transport_scope()
                        .await
                        .map_err(|_| Error::Storage("cannot read committed transport scope"))?;
                    transport.refresh_scope(scope).await?;
                }
                Ok(())
            }
            .await;
            if let Err(error) = &result {
                transport.fail_closed();
                eprintln!("Control transport scope failed: {error}");
            }
            result
        });
        Ok(Self {
            id,
            raft,
            machine,
            network,
            server: Some(task),
            scope: Some(scope),
            scope_stop: Some(scope_stop),
            store_released,
        })
    }
    pub fn id(&self) -> PeerId {
        self.id
    }
    pub fn room_id(&self) -> [u8; 32] {
        self.machine.room_id()
    }
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.network.local_addr()
    }
    /// Registers a bounded Send handle, never the !Send private provider itself.
    pub async fn register_room(&self, handle: crate::rooms::runtime::Handle) -> Result<()> {
        self.network.register_room(handle).await
    }
    pub fn unregister_room(&self) -> Result<()> {
        self.network.unregister_room()
    }
    /// Explicit SDK name source, independent of local profile/settings wiring.
    pub fn configure_profile_name(&self, name: String, share: bool) -> Result<()> {
        self.network.configure_profile_name(name, share)
    }
    /// Existing profile source, verified against this service's public identity.
    /// None disables disclosure; no private key is loaded or data created.
    pub fn configure_profile_directory(&self, directory: Option<std::path::PathBuf>) -> Result<()> {
        self.network.configure_profile_directory(directory)
    }
    /// Bounded optional room metadata. Never cached; identity and epoch are
    /// verified before acceptance. Names are presentation text, not trust.
    pub async fn peer_profile(
        &self,
        target: PeerId,
        timeout: Duration,
    ) -> Result<network::ProfileMetadata> {
        self.network
            .peer_profile(target, &self.configured_node(target)?, timeout)
            .await
    }
    /// Memory-only, synchronous snapshot. Pass the complete current private-room
    /// membership and epoch, not an additive interest list. Missing/expired names
    /// are omitted; bounded refresh is scheduled through the transport owner.
    /// This never waits for network, quorum or a private-room actor.
    pub fn cached_peer_profiles(
        &self,
        epoch: u64,
        members: &[PeerId],
    ) -> Result<BTreeMap<PeerId, String>> {
        if members.len() > config::ROOM_MAX_MEMBERS {
            return Err(Error::Limit {
                resource: "profile cache members",
                max: config::ROOM_MAX_MEMBERS,
            });
        }
        let metrics = self.raft.metrics();
        let membership = metrics.borrow().membership_config.clone();
        let routes = members
            .iter()
            .filter(|peer| **peer != self.id)
            .filter_map(|peer| {
                membership
                    .membership()
                    .get_node(peer)
                    .map(|node| (*peer, node.clone()))
            })
            .collect();
        self.network
            .cached_peer_profiles(epoch, members, *membership.log_id(), routes)
    }
    pub fn enable_files(
        &self,
        directory: std::path::PathBuf,
        max_bytes: u64,
        budget: u64,
    ) -> Result<()> {
        self.network.enable_files(directory, max_bytes, budget)
    }

    pub fn configure_files(
        &self,
        directory: Option<std::path::PathBuf>,
        max_bytes: u64,
        budget: u64,
    ) -> Result<bool> {
        self.network.configure_files(directory, max_bytes, budget)
    }

    pub fn files_enabled(&self) -> Result<bool> {
        self.network.files_enabled()
    }
    pub async fn subscribe_media(
        &self,
        peer: PeerId,
        kind: crate::media::Kind,
    ) -> Result<network::MediaSubscription> {
        self.network.subscribe_media(peer, kind).await
    }
    pub fn media_status(&self) -> Result<network::MediaStatus> {
        self.network.media_status()
    }
    pub async fn publish_media(
        &self,
        kind: crate::media::Kind,
        peers: Vec<PeerId>,
    ) -> Result<network::MediaPublication> {
        self.network
            .publish_media(kind, peers, self.raft.metrics())
            .await
    }
    pub async fn send_room_file(
        &self,
        target: PeerId,
        source: &Path,
        sensitive: bool,
        deadline: Duration,
    ) -> Result<crate::files::Receipt> {
        let node = self.configured_node(target)?;
        self.network
            .room_file(target, &node, source, sensitive, deadline)
            .await
    }

    fn configured_node(&self, target: PeerId) -> Result<Node> {
        self.raft
            .metrics()
            .borrow()
            .membership_config
            .membership()
            .get_node(&target)
            .cloned()
            .ok_or(Error::Denied)
    }

    pub async fn share_file(
        &self,
        source: &Path,
        audience: std::collections::BTreeSet<PeerId>,
    ) -> Result<crate::files::FileInfo> {
        self.network.share_file(source, audience).await
    }

    pub fn unshare_file(&self, hash: [u8; 32]) -> Result<bool> {
        self.network.unshare_file(hash)
    }

    pub async fn file_availability(
        &self,
        target: PeerId,
        hash: [u8; 32],
        deadline: Duration,
    ) -> Result<Option<crate::files::FileInfo>> {
        self.network
            .file_availability(target, &self.configured_node(target)?, hash, deadline)
            .await
    }

    pub async fn fetch_file(
        &self,
        target: PeerId,
        hash: [u8; 32],
        deadline: Duration,
    ) -> Result<crate::files::Receipt> {
        self.network
            .fetch_file(target, &self.configured_node(target)?, hash, deadline)
            .await
    }
    /// Direct pinned-peer data. ACK is private processing plus an application
    /// event queue, not human reading. A lost response leaves delivery uncertain.
    pub async fn send_room_text(
        &self,
        target: PeerId,
        wire: &[u8],
        deadline: Duration,
    ) -> Result<()> {
        let node = self
            .raft
            .metrics()
            .borrow()
            .membership_config
            .membership()
            .get_node(&target)
            .cloned()
            .ok_or(Error::Denied)?;
        self.network.room_text(target, &node, wire, deadline).await
    }
    pub async fn current_leader(&self) -> Option<PeerId> {
        self.raft.current_leader().await
    }
    /// Cached public configuration, not a read-quorum or authorization claim.
    pub fn control_membership(&self) -> Arc<openraft::StoredMembership<PeerId, Node>> {
        self.raft.metrics().borrow().membership_config.clone()
    }
    /// Public ROOM-SCOPED transport pins only; never a global contact import.
    pub fn approved_control_peers(&self) -> Result<BTreeMap<PeerId, crate::Peer>> {
        self.network.approved_peers()
    }
    pub async fn public_view(&self) -> Result<crate::rooms::control::View> {
        self.machine
            .public_view()
            .await
            .map_err(|_| Error::Storage("cannot read public control state"))
    }
    pub async fn public_origin(&self) -> Result<PublicIdentity> {
        self.machine
            .public_origin()
            .await
            .map_err(|_| Error::Storage("public genesis proof unavailable"))
    }
    pub(crate) fn enrollment_seeds(&self) -> Result<Vec<crate::rooms::enrollment::Seed>> {
        let peers = self.network.approved_peers()?;
        let membership = self.control_membership();
        let voters = membership
            .membership()
            .voter_ids()
            .collect::<std::collections::BTreeSet<_>>();
        let mut seeds = Vec::new();
        for (id, node) in membership.membership().nodes() {
            if !voters.contains(id) {
                continue;
            }
            let peer = peers.get(id).ok_or(Error::Authentication)?;
            let endpoint = node
                .endpoint
                .ok_or(Error::Configuration("control seed has no address hint"))?;
            seeds.push(crate::rooms::enrollment::Seed {
                certificate: peer.certificate.as_ref().to_vec(),
                endpoint,
            });
        }
        if seeds.is_empty() || seeds.len() > config::ROOM_MAX_MEMBERS {
            return Err(Error::Configuration("invalid control bootstrap count"));
        }
        Ok(seeds)
    }
    /// Protected local committed records, not client receipts or a remote snapshot.
    /// A missing bounded-history bridge requires explicit verified rejoin.
    pub async fn committed_after(
        &self,
        cursor: Option<super::Position>,
    ) -> Result<Vec<super::CommittedRecord>> {
        self.machine.committed_after(cursor).await.map_err(|_| {
            Error::Storage("private catch-up unavailable; verified rejoin may be required")
        })
    }
    pub async fn initialize(&self, mut voters: BTreeMap<PeerId, Node>) -> Result<()> {
        if voters.is_empty() || voters.len() > config::ROOM_MAX_MEMBERS {
            return Err(Error::Configuration("invalid control voter count"));
        }
        let approved = self.network.approved_peers()?;
        for (id, node) in &mut voters {
            let peer = approved.get(id).ok_or(Error::Denied)?;
            if !node.certificate.is_empty() {
                node.peer(*id)?;
            }
            node.certificate = peer.certificate_der().to_vec();
        }
        self.raft
            .initialize(voters)
            .await
            .map_err(|_| Error::Storage("cannot initialize existing or invalid control membership"))
    }
    /// Explicit signed operator primitive. Target must ALREADY have
    /// committed room authority; this adds replication only, never a vote. User
    /// adapters must also enforce local profile veto/consent at their owner.
    /// Timeout is uncertain: inspect the durable membership before retrying.
    pub async fn provision_learner(
        &self,
        target: PeerId,
        node: Node,
        deadline: Duration,
    ) -> Result<()> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        let endpoint = node.endpoint.ok_or(Error::Configuration(
            "learner requires an explicit address hint",
        ))?;
        if endpoint.port() == 0
            || endpoint.ip().is_unspecified()
            || endpoint.ip().is_multicast()
            || target == self.id
        {
            return Err(Error::Configuration("invalid learner target"));
        }
        if !node.certificate.is_empty() {
            node.peer(target)?;
        }
        tokio::time::timeout(deadline, async {
            self.fresh_local(deadline).await?;
            let view = self.public_view().await?;
            if !view
                .member_rights(&self.id.0)
                .is_some_and(|rights| rights.can_invite())
            {
                return Err(Error::Denied);
            }
            if view.member_rights(&target.0).is_none() {
                return Err(Error::Denied);
            }
            let before = *self.control_membership().log_id();
            let request = self.network.node_request(target, endpoint, before)?;
            match self
                .route(Operation::NodeIntent(Box::new(request)), deadline)
                .await?
            {
                Output::NodeIntent(applied)
                    if applied.target == target && applied.node.endpoint == Some(endpoint) =>
                {
                    applied.node.peer(target)?;
                    Ok(())
                }
                _ => Err(Error::Protocol),
            }
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    /// Explicit controller approval only. Newly admitted nodes remain learners
    /// unless this signed administrative operation selects them. Removed voters
    /// remain metadata learners; this does not remove their data membership.
    pub async fn set_voters(
        &self,
        voters: std::collections::BTreeSet<PeerId>,
        deadline: Duration,
    ) -> Result<openraft::LogId<PeerId>> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        if voters.is_empty() || voters.len() > config::ROOM_MAX_MEMBERS {
            return Err(Error::Configuration(
                "explicit voter set must be nonempty and bounded",
            ));
        }
        tokio::time::timeout(deadline, async {
            self.fresh_local(deadline).await?;
            let view = self.public_view().await?;
            if !view
                .member_rights(&self.id.0)
                .is_some_and(|rights| rights.can_control())
            {
                return Err(Error::Denied);
            }
            for id in &voters {
                if view.member_rights(&id.0).is_none() {
                    return Err(Error::Denied);
                }
            }
            let request = self
                .network
                .voter_request(voters.clone(), *self.control_membership().log_id())?;
            match self
                .route(Operation::Voters(Box::new(request)), deadline)
                .await?
            {
                Output::Voters(applied) if applied.voters == voters => {
                    self.raft
                        .wait(Some(deadline))
                        .applied_index_at_least(
                            Some(applied.membership.index),
                            "local voter configuration application",
                        )
                        .await
                        .map_err(|_| Error::Timeout)?;
                    Ok(applied.membership)
                }
                _ => Err(Error::Protocol),
            }
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    /// Retain the exact signed proposal/nonce across redirects. Timeout is an
    /// uncertain outcome, never final rejection or permission to discard crypto.
    pub async fn submit_public(
        &self,
        proposal: Proposal,
        deadline: Duration,
    ) -> Result<ApplyResult> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        if proposal.author() != self.id.0 {
            return Err(Error::Authentication);
        }
        match self
            .route(Operation::Submit(Box::new(proposal)), deadline)
            .await?
        {
            Output::Submitted(result) => Ok(result),
            _ => Err(Error::Protocol),
        }
    }
    async fn route(&self, operation: Operation, deadline: Duration) -> Result<Output> {
        self.route_seeded(operation, deadline, &[]).await
    }
    async fn route_seeded(
        &self,
        operation: Operation,
        deadline: Duration,
        seeds: &[crate::rooms::enrollment::Seed],
    ) -> Result<Output> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        if seeds.len() > config::ROOM_MAX_MEMBERS {
            return Err(Error::Protocol);
        }
        let mut hints = BTreeMap::new();
        for seed in seeds {
            let peer = Peer::from_der(seed.certificate.clone())?;
            let id = PeerId::from_peer(&peer);
            if id != self.id && self.network.allows(id)? {
                hints.insert(id, Node::for_peer(&peer, Some(seed.endpoint)));
            }
        }
        let mut candidates = hints.iter().cycle();
        tokio::time::timeout(deadline, async {
            let mut target = self
                .raft
                .current_leader()
                .await
                .or_else(|| hints.keys().next().copied())
                .unwrap_or(self.id);
            let mut hinted = hints.get(&target).cloned();
            let mut visited = std::collections::BTreeSet::new();
            loop {
                if !self.network.allows(target)? {
                    let scope = self
                        .machine
                        .transport_scope()
                        .await
                        .map_err(|_| Error::Storage("cannot refresh control route scope"))?;
                    self.network.refresh_scope(scope).await?;
                    if !self.network.allows(target)? {
                        return Err(Error::Denied);
                    }
                }
                let response = if visited.insert(target) {
                    self.route_once(&operation, target, hinted.take()).await
                } else {
                    Ok(network::Reply::Unavailable)
                };
                match response {
                    Ok(network::Reply::Denied) => return Err(Error::Denied),
                    Ok(network::Reply::Conflict) => {
                        return Err(Error::Node(
                            "control configuration changed; inspect before retry",
                        ));
                    }
                    Ok(network::Reply::StateUnavailable) => {
                        return Err(Error::Storage(
                            "committed artifact unavailable; verified recovery or rejoin required",
                        ));
                    }
                    Ok(network::Reply::Applied(result)) => return Ok(result),
                    Ok(network::Reply::Forward {
                        leader: Some(leader),
                        node,
                    }) if !visited.contains(&leader) => {
                        target = leader;
                        hinted = node;
                        continue;
                    }
                    Err(Error::Network | Error::Timeout) | Ok(_) => {}
                    Err(error) => return Err(error),
                }
                // No leader/hint during an election is transient, not a false
                // permanent failure. Retry only within the original deadline;
                // signed proposal bytes/nonce remain identical on every attempt.
                tokio::time::sleep(Duration::from_millis(50)).await;
                let next = candidates.next();
                target = self
                    .raft
                    .current_leader()
                    .await
                    .or_else(|| next.map(|(id, _)| *id))
                    .unwrap_or(self.id);
                hinted = hints.get(&target).cloned();
                visited.clear();
            }
        })
        .await
        .map_err(|_| Error::Timeout)?
    }

    /// Only this already-configured device's address changes. Data authority and
    /// the voter set are untouched, including when this device is a controller.
    pub async fn update_address(
        &self,
        seeds: &[crate::rooms::enrollment::Seed],
        deadline: Duration,
    ) -> Result<openraft::LogId<PeerId>> {
        let endpoint = self.local_addr()?;
        tokio::time::timeout(deadline, async {
            let Output::Configuration(configuration) = self
                .route_seeded(Operation::Configuration, deadline, seeds)
                .await?
            else {
                return Err(Error::Protocol);
            };
            let own = configuration
                .membership()
                .get_node(&self.id)
                .ok_or(Error::Denied)?;
            own.peer(self.id)?;
            if own.endpoint == Some(endpoint) {
                return configuration.log_id().ok_or(Error::Protocol);
            }
            let request = self
                .network
                .address_request(endpoint, *configuration.log_id())?;
            match self
                .route_seeded(Operation::NodeIntent(Box::new(request)), deadline, seeds)
                .await?
            {
                Output::NodeIntent(applied)
                    if applied.target == self.id && applied.node.endpoint == Some(endpoint) =>
                {
                    applied.node.peer(self.id)?;
                    Ok(applied.membership)
                }
                _ => Err(Error::Protocol),
            }
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    async fn route_once(
        &self,
        operation: &Operation,
        target: PeerId,
        hinted: Option<Node>,
    ) -> Result<network::Reply<Output>> {
        if target == self.id {
            return Ok(match operation {
                Operation::Configuration => {
                    network::configuration_local(&self.raft, &self.machine, self.id)
                        .await
                        .map(Output::Configuration)
                }
                Operation::Submit(proposal) => self
                    .network
                    .submit_direct(&self.raft, proposal.as_ref().clone())
                    .await
                    .map(Output::Submitted),
                Operation::ReadBarrier => network::barrier_local(&self.raft)
                    .await
                    .map(Output::ReadBarrier),
                Operation::WelcomeForSelf => {
                    network::welcome_local(&self.raft, &self.machine, self.id)
                        .await
                        .map(Output::Welcome)
                }
                Operation::NodeIntent(request) => self
                    .network
                    .node_direct(&self.raft, &self.machine, request.as_ref().clone())
                    .await
                    .map(Output::NodeIntent),
                Operation::Voters(request) => self
                    .network
                    .voters_direct(&self.raft, &self.machine, request.as_ref().clone())
                    .await
                    .map(Output::Voters),
            });
        }
        let node = hinted.or_else(|| {
            self.raft
                .metrics()
                .borrow()
                .membership_config
                .membership()
                .get_node(&target)
                .cloned()
        });
        let Some(node) = node else {
            return Ok(network::Reply::Unavailable);
        };
        Ok(match operation {
            Operation::Configuration => self
                .network
                .read_configuration(target, &node, config::DEFAULT_TIMEOUT)
                .await?
                .map(Output::Configuration),
            Operation::Submit(proposal) => self
                .network
                .submit(
                    target,
                    &node,
                    proposal.as_ref().clone(),
                    config::DEFAULT_TIMEOUT,
                )
                .await?
                .map(Output::Submitted),
            Operation::ReadBarrier => self
                .network
                .read_barrier(target, &node, config::DEFAULT_TIMEOUT)
                .await?
                .map(Output::ReadBarrier),
            Operation::WelcomeForSelf => self
                .network
                .welcome_for_self(target, &node, config::DEFAULT_TIMEOUT)
                .await?
                .map(Output::Welcome),
            Operation::NodeIntent(request) => self
                .network
                .node_update(
                    target,
                    &node,
                    request.as_ref().clone(),
                    config::DEFAULT_TIMEOUT,
                )
                .await?
                .map(Output::NodeIntent),
            Operation::Voters(request) => self
                .network
                .voters(
                    target,
                    &node,
                    request.as_ref().clone(),
                    config::DEFAULT_TIMEOUT,
                )
                .await?
                .map(Output::Voters),
        })
    }
    /// Quorum read barrier followed by LOCAL application, bounded by one deadline.
    /// A minority or stale former leader cannot return a fresh-view claim.
    pub async fn fresh_catch_up(
        &self,
        cursor: Option<super::Position>,
        deadline: Duration,
    ) -> Result<(crate::rooms::control::View, Vec<super::CommittedRecord>)> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        tokio::time::timeout(deadline, async {
            self.fresh_local(deadline).await?;
            self.machine
                .committed_state_after(cursor)
                .await
                .map_err(|_| Error::Storage("private catch-up gap; verified rejoin required"))
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    async fn fresh_local(&self, deadline: Duration) -> Result<()> {
        let Output::ReadBarrier(required) = self.route(Operation::ReadBarrier, deadline).await?
        else {
            return Err(Error::Protocol);
        };
        self.raft
            .wait(Some(deadline))
            .applied_index_at_least(required.map(|log| log.index), "private room fresh view")
            .await
            .map(|_| ())
            .map_err(|_| Error::Timeout)
    }
    /// TLS selects ONLY this recipient. A remote artifact/position is not trusted
    /// as join authority: match it against fresh protected LOCAL committed state.
    pub async fn receive_welcome(
        &self,
        deadline: Duration,
    ) -> Result<Option<(crate::rooms::control::Welcome, super::CommittedRecord)>> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        tokio::time::timeout(deadline, async {
            let Output::Welcome(remote) = self.route(Operation::WelcomeForSelf, deadline).await?
            else {
                return Err(Error::Protocol);
            };
            self.fresh_local(deadline).await?;
            let local = self.machine.committed_welcome(self.id).await.map_err(|_| {
                Error::Storage("Welcome bridge missing; verified recovery or rejoin required")
            })?;
            match (remote, local) {
                (None, None) => Ok(None),
                (Some(remote), Some((artifact, record)))
                    if remote.position == record.position()?
                        && remote.artifact.proposal == artifact.proposal
                        && remote.artifact.epoch == artifact.epoch
                        && remote.artifact.author == artifact.author
                        && remote.artifact.recipient == self.id.0
                        && remote.artifact.recipient == artifact.recipient
                        && remote.artifact.wire == artifact.wire =>
                {
                    Ok(Some((artifact, record)))
                }
                (Some(_), None) => Err(Error::Revoked),
                _ => Err(Error::Authentication),
            }
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    pub async fn shutdown(mut self) -> Result<()> {
        self.network.suspend_profile_cache();
        let media = self.network.shutdown_media().await;
        // Finish bounded in-flight store work before releasing its owner lease;
        // aborting an async waiter does not cancel spawn_blocking SQLite work.
        if let Some(stop) = self.scope_stop.take() {
            let _ = stop.send(());
        }
        let scope = if let Some(task) = self.scope.take() {
            match task.await {
                Ok(result) => result,
                Err(error) if error.is_cancelled() => Ok(()),
                Err(_) => Err(Error::Storage("control scope task failed")),
            }
        } else {
            Ok(())
        };
        let result = self
            .raft
            .shutdown()
            .await
            .map_err(|_| Error::Storage("control engine shutdown failed"));
        self.network.close();
        let server = if let Some(task) = self.server.take() {
            match task.await {
                Ok(result) => result,
                Err(_) => Err(Error::Storage("control server task failed")),
            }
        } else {
            Ok(())
        };
        let idle = self.network.wait_idle().await;
        let result = result.and(scope).and(server).and(media);
        let drivers = self.network.driver_owner();
        let witness = self.network.owner_witness();
        let id = self.id;
        let mut store_released = self.store_released.clone();
        if idle.is_err() {
            eprintln!(
                "Control {id}: bounded close grace elapsed; owned drivers: {}",
                drivers.diagnostics()
            );
        }
        drop(self);
        // Application/core/server work has ended. Transport drain timers are
        // not allowed to keep a stopped node or its port alive indefinitely.
        drivers.terminate();
        let released = drivers.released().await;
        let storage = tokio::time::timeout(
            config::DEFAULT_TIMEOUT,
            store_released.wait_for(|released| *released),
        )
        .await
        .map_err(|_| Error::Storage("public control storage owner did not finish shutdown"))?
        .map(|_| ())
        .map_err(|_| Error::Storage("public control storage release signal failed"));
        if let Err(error) = &released {
            eprintln!(
                "Control {id} socket release failed: {error}; scope owners={}; remaining={}",
                witness.owners(),
                drivers.diagnostics()
            );
        }
        result.and(released).and(storage)
    }
}
impl Drop for ControlNode {
    fn drop(&mut self) {
        if let Some(task) = &self.scope {
            task.abort();
        }
        if let Some(task) = self.server.take() {
            task.abort();
        }
    }
}

#[cfg(all(test, unix))]
mod profile_tests;
#[cfg(all(test, unix))]
mod tests;
