//! Real OpenRaft nodes and durable SQLite with a controlled in-process RPC router.
//! This is algorithm/partition evidence, NOT the production QUIC/VM room gate.
use super::*;
use crate::{
    Identity,
    rooms::{Grant, Rights, RootBundle, mls::CryptoMember},
};
use futures_util::FutureExt;
use openraft::error::{InstallSnapshotError, RPCError, RaftError, RemoteError, Unreachable};
use openraft::network::RPCOption;
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use openraft::{Config, Raft, RaftNetwork, RaftNetworkFactory};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, RwLock},
    time::Duration,
};

#[derive(Clone, Default)]
struct Router {
    nodes: Arc<RwLock<BTreeMap<PeerId, Raft<TypeConfig>>>>,
    isolated: Arc<RwLock<BTreeSet<PeerId>>>,
}
struct Factory {
    source: PeerId,
    router: Router,
}
struct Client {
    source: PeerId,
    target: PeerId,
    router: Router,
}
impl RaftNetworkFactory<TypeConfig> for Factory {
    type Network = Client;
    async fn new_client(&mut self, target: PeerId, _node: &Node) -> Self::Network {
        Client {
            source: self.source,
            target,
            router: self.router.clone(),
        }
    }
}
impl Client {
    fn target(&self) -> Result<Raft<TypeConfig>, Unreachable> {
        let blocked = self.router.isolated.read().unwrap();
        if blocked.contains(&self.source) || blocked.contains(&self.target) {
            return Err(Unreachable::new(&std::io::Error::other(
                "controlled partition",
            )));
        }
        self.router
            .nodes
            .read()
            .unwrap()
            .get(&self.target)
            .cloned()
            .ok_or_else(|| Unreachable::new(&std::io::Error::other("controlled node unavailable")))
    }
}
impl RaftNetwork<TypeConfig> for Client {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<TypeConfig>,
        _option: RPCOption,
    ) -> Result<AppendEntriesResponse<PeerId>, RPCError<PeerId, Node, RaftError<PeerId>>> {
        self.target()
            .map_err(RPCError::Unreachable)?
            .append_entries(rpc)
            .await
            .map_err(|error| RPCError::RemoteError(RemoteError::new(self.target, error)))
    }
    async fn vote(
        &mut self,
        rpc: VoteRequest<PeerId>,
        _option: RPCOption,
    ) -> Result<VoteResponse<PeerId>, RPCError<PeerId, Node, RaftError<PeerId>>> {
        self.target()
            .map_err(RPCError::Unreachable)?
            .vote(rpc)
            .await
            .map_err(|error| RPCError::RemoteError(RemoteError::new(self.target, error)))
    }
    async fn install_snapshot(
        &mut self,
        rpc: InstallSnapshotRequest<TypeConfig>,
        _option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<PeerId>,
        RPCError<PeerId, Node, RaftError<PeerId, InstallSnapshotError>>,
    > {
        self.target()
            .map_err(RPCError::Unreachable)?
            .install_snapshot(rpc)
            .await
            .map_err(|error| RPCError::RemoteError(RemoteError::new(self.target, error)))
    }
}

async fn leader(raft: &Raft<TypeConfig>, excluded: Option<PeerId>) -> PeerId {
    let mut metrics = raft.metrics();
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let current = metrics.borrow().current_leader;
            if let Some(current) = current.filter(|value| Some(*value) != excluded) {
                return current;
            }
            metrics.changed().await.unwrap();
        }
    })
    .await
    .expect("eligible majority did not elect a leader")
}

#[tokio::test]
async fn minority_cannot_ack_and_majority_recovers_a_partitioned_leader() {
    let directory = crate::test_support::tempdir().unwrap();
    let identities: Vec<_> = (0..3)
        .map(|index| Identity::create(&directory.path().join(format!("identity-{index}"))).unwrap())
        .collect();
    let ids: Vec<_> = identities
        .iter()
        .map(|identity| PeerId::from_peer(&identity.peer()))
        .collect();
    let anchor = RootBundle::create(&identities[0], "Durable consensus fixture")
        .unwrap()
        .verify(&identities[0].peer())
        .unwrap();
    let creator = CryptoMember::new(&identities[0], &anchor, vec![])
        .unwrap()
        .public_identity()
        .unwrap();
    let grant = Grant::issue(
        &anchor,
        &[],
        &identities[0],
        &identities[1].peer(),
        Rights::MODERATOR,
    )
    .unwrap();
    let mut bob = CryptoMember::new(&identities[1], &anchor, vec![grant]).unwrap();
    let package = bob.key_package().unwrap();
    let router = Router::default();
    let configuration = Arc::new(
        Config {
            heartbeat_interval: 50,
            election_timeout_min: 200,
            election_timeout_max: 400,
            max_payload_entries: 32,
            ..Default::default()
        }
        .validate()
        .unwrap(),
    );
    let mut nodes = BTreeMap::new();
    let mut machines = BTreeMap::new();
    for (index, id) in ids.iter().copied().enumerate() {
        let store = LogStore::create(&directory.path().join(format!("control-{index}"))).unwrap();
        let machine = StateMachine::create(store.clone(), &anchor, creator.clone())
            .await
            .unwrap();
        let raft = Raft::new(
            id,
            configuration.clone(),
            Factory {
                source: id,
                router: router.clone(),
            },
            store,
            machine.clone(),
        )
        .await
        .unwrap();
        nodes.insert(id, raft.clone());
        machines.insert(id, machine);
        router.nodes.write().unwrap().insert(id, raft);
    }
    let outcome = std::panic::AssertUnwindSafe(async {
        nodes[&ids[0]]
            .initialize(
                ids.iter()
                    .copied()
                    .map(|id| (id, Node::default()))
                    .collect::<BTreeMap<_, _>>(),
            )
            .await
            .unwrap();
        let initial = leader(&nodes[&ids[0]], None).await;
        let initial_view = machines[&initial].public_view().await.unwrap();
        let add = Proposal::add(
            &identities[0],
            anchor.id(),
            0,
            initial_view.head(),
            b"opaque consensus fixture add",
            &package,
        )
        .unwrap();
        let applied = nodes[&initial]
            .client_write(add.with_ordered_decision(true))
            .await
            .unwrap();
        assert!(applied.data.accepted);
        assert_eq!(applied.data.epoch, 1);
        router.isolated.write().unwrap().insert(initial);
        let current_view = machines[&initial].public_view().await.unwrap();
        let minority = Proposal::remove(
            &identities[0],
            anchor.id(),
            1,
            current_view.head(),
            b"minority opaque remove",
            &identities[1].peer(),
        )
        .unwrap();
        let no_ack = tokio::time::timeout(
            Duration::from_millis(900),
            nodes[&initial].client_write(minority.with_ordered_decision(true)),
        )
        .await;
        assert!(
            !matches!(no_ack, Ok(Ok(_))),
            "minority reported quorum success"
        );
        assert_eq!(machines[&initial].public_view().await.unwrap().epoch(), 1);
        let observer = ids.iter().copied().find(|id| *id != initial).unwrap();
        let elected = leader(&nodes[&observer], Some(initial)).await;
        // Election and state-machine application are distinct completion points.
        tokio::time::timeout(Duration::from_secs(5), async {
            while machines[&elected].public_view().await.unwrap().epoch() != 1 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("new leader did not apply the previously committed prefix");
        let majority_view = machines[&elected].public_view().await.unwrap();
        let remove_creator = Proposal::remove(
            &identities[1],
            anchor.id(),
            1,
            majority_view.head(),
            b"majority opaque remove",
            &identities[0].peer(),
        )
        .unwrap();
        let result = nodes[&elected]
            .client_write(remove_creator.with_ordered_decision(true))
            .await
            .unwrap();
        assert!(result.data.accepted);
        assert_eq!(result.data.epoch, 2);
        router.isolated.write().unwrap().remove(&initial);
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let view = machines[&initial].public_view().await.unwrap();
                if view.epoch() == 2 && view.head() == result.data.head {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("healed node did not converge to the accepted majority record");
    })
    .catch_unwind()
    .await;
    router.isolated.write().unwrap().clear();
    for node in nodes.values() {
        node.shutdown().await.unwrap();
    }
    router.nodes.write().unwrap().clear();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
