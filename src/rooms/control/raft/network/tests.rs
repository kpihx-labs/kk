//! Production QUIC transport over actual loopback UDP sockets. Not the cross-VM
//! room gate: lifecycle, public/private apply cursors and room pairing remain open.
use super::super::{LogStore, StateMachine};
use super::*;
use crate::rooms::{Grant, Rights, RootBundle, group::Group, mls::CryptoMember};
use futures_util::FutureExt;
use openraft::Vote;
use std::time::Duration;

#[tokio::test(flavor = "current_thread")]
async fn delayed_committed_scope_updates_cannot_restore_old_pins_or_rollback_media_epoch() {
    use super::super::machine::TransportScope;
    let root = crate::test_support::tempdir().unwrap();
    let identity = Arc::new(Identity::create(&root.path().join("owner")).unwrap());
    let old = Identity::create(&root.path().join("old")).unwrap().peer();
    let new = Identity::create(&root.path().join("new")).unwrap().peer();
    let own = identity.peer();
    let own_id = PeerId::from_peer(&own);
    let (factory, server) = Factory::bind(
        [42; 32],
        identity,
        BTreeMap::from([
            (own_id, own.clone()),
            (PeerId::from_peer(&old), old.clone()),
        ]),
        "127.0.0.1:0".parse().unwrap(),
    )
    .unwrap();
    let position =
        |term, index| openraft::LogId::new(openraft::CommittedLeaderId::new(term, own_id), index);
    let scope = |applied, epoch, peer| TransportScope {
        applied: Some(applied),
        epoch,
        members: vec![own.clone(), peer],
        nodes: BTreeMap::new(),
    };
    factory
        .refresh_scope(scope(position(1, 2), 4, new.clone()))
        .await
        .unwrap();
    factory
        .refresh_scope(scope(position(1, 1), 3, old.clone()))
        .await
        .unwrap();
    assert!(factory.allows(PeerId::from_peer(&new)).unwrap());
    assert!(!factory.allows(PeerId::from_peer(&old)).unwrap());
    assert_eq!(factory.shared.media_epoch.load(Ordering::SeqCst), 4);
    assert!(
        factory
            .refresh_scope(scope(position(2, 2), 4, old.clone()))
            .await
            .is_err()
    );
    assert!(
        factory
            .refresh_scope(scope(position(1, 3), 2, old))
            .await
            .is_err()
    );
    factory
        .replace_peers(BTreeMap::from([(own_id, own.clone())]))
        .await
        .unwrap();
    factory
        .refresh_scope(scope(position(1, 3), 5, new.clone()))
        .await
        .unwrap();
    assert!(!factory.allows(PeerId::from_peer(&new)).unwrap());
    assert_eq!(factory.shared.media_epoch.load(Ordering::SeqCst), 5);
    let drivers = factory.driver_owner();
    factory.close();
    drop(server);
    drop(factory);
    drivers.terminate();
    drivers.released().await.unwrap();
}

async fn elected(raft: &Raft<TypeConfig>, excluded: Option<PeerId>) -> PeerId {
    let mut metrics = raft.metrics();
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let current = metrics.borrow().current_leader;
            if let Some(current) = current.filter(|id| Some(*id) != excluded) {
                return current;
            }
            metrics.changed().await.unwrap();
        }
    })
    .await
    .expect("TLS control majority did not elect leader")
}
async fn ordered(
    nodes: &BTreeMap<PeerId, Raft<TypeConfig>>,
    mut leader: PeerId,
    proposal: super::super::super::Proposal,
) -> (PeerId, ApplyResult) {
    // An observed leader ID is not a lease. Only retry explicit election
    // redirects, with the EXACT signed operation inside one original deadline.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match nodes[&leader]
                .client_write(proposal.clone().with_ordered_decision(true))
                .await
            {
                Ok(response) => return (leader, response.data),
                Err(error) => match error.forward_to_leader::<Node>() {
                    Some(forward)
                        if forward.leader_id.is_some_and(|id| nodes.contains_key(&id)) =>
                    {
                        leader = forward.leader_id.unwrap();
                        tokio::task::yield_now().await;
                    }
                    _ => panic!("unexpected ordered fixture error: {error}"),
                },
            }
        }
    })
    .await
    .expect("ordered fixture did not settle within its deadline")
}

#[tokio::test]
async fn mutual_tls_context_sender_binding_reuse_and_revocation_are_enforced() {
    let directory = crate::test_support::tempdir().unwrap();
    let alice = Arc::new(Identity::create(&directory.path().join("alice")).unwrap());
    let bob = Arc::new(Identity::create(&directory.path().join("bob")).unwrap());
    let a = PeerId::from_peer(&alice.peer());
    let b = PeerId::from_peer(&bob.peer());
    let anchor = RootBundle::create(&alice, "TLS control fixture")
        .unwrap()
        .verify(&alice.peer())
        .unwrap();
    let public = CryptoMember::new(&alice, &anchor, vec![])
        .unwrap()
        .public_identity()
        .unwrap();
    let peers = BTreeMap::from([(a, alice.peer()), (b, bob.peer())]);
    let (mut factory_a, _server_a) = Factory::bind(
        anchor.id(),
        alice.clone(),
        peers.clone(),
        "127.0.0.1:0".parse().unwrap(),
    )
    .unwrap();
    let (factory_b, server_b) = Factory::bind(
        anchor.id(),
        bob.clone(),
        peers.clone(),
        "127.0.0.1:0".parse().unwrap(),
    )
    .unwrap();
    let store = LogStore::create(&directory.path().join("control-b")).unwrap();
    let machine = StateMachine::create(store.clone(), &anchor, public)
        .await
        .unwrap();
    let raft = Raft::new(
        b,
        Arc::new(configuration().unwrap()),
        factory_b.clone(),
        store,
        machine.clone(),
    )
    .await
    .unwrap();
    let task = tokio::spawn(server_b.run(raft.clone(), machine));
    let outcome = std::panic::AssertUnwindSafe(async {
        let node = Node {
            endpoint: Some(factory_b.local_addr().unwrap()),
            ..Default::default()
        };
        let mut client = factory_a.new_client(b, &node).await;
        let option = RPCOption::new(Duration::from_millis(800));
        // Valid TLS Alice may not claim to be candidate Bob in a high-term RPC.
        assert!(
            client
                .vote(
                    VoteRequest {
                        vote: Vote::new(90, b),
                        last_log_id: None
                    },
                    option.clone()
                )
                .await
                .is_err()
        );
        assert_eq!(raft.metrics().borrow().current_term, 0);
        let (mut wrong, _server) = Factory::bind(
            [9; 32],
            alice.clone(),
            peers,
            "127.0.0.1:0".parse().unwrap(),
        )
        .unwrap();
        let mut wrong_room = wrong.new_client(b, &node).await;
        assert!(
            wrong_room
                .vote(
                    VoteRequest {
                        vote: Vote::new(91, a),
                        last_log_id: None
                    },
                    option.clone()
                )
                .await
                .is_err()
        );
        assert_eq!(raft.metrics().borrow().current_term, 0);
        // A was explicitly selected as a bootstrap controller in this fixture.
        // The initialized-membership test separately denies pinned nonvoters.
        client
            .vote(
                VoteRequest {
                    vote: Vote::new(1, a),
                    last_log_id: None,
                },
                option.clone(),
            )
            .await
            .unwrap();
        assert!(matches!(
            factory_a
                .read_barrier(b, &node, Duration::from_millis(800))
                .await
                .unwrap(),
            Reply::Forward { leader: None, .. }
        ));
        let slot = factory_a.shared.slots.lock().await.get(&b).unwrap().clone();
        let stable = slot.lock().await.as_ref().unwrap().connection.stable_id();
        assert!(matches!(
            factory_a
                .read_barrier(b, &node, Duration::from_millis(800))
                .await
                .unwrap(),
            Reply::Forward { leader: None, .. }
        ));
        assert_eq!(
            slot.lock().await.as_ref().unwrap().connection.stable_id(),
            stable
        );
        let before = raft.metrics().borrow().current_term;
        factory_b
            .replace_peers(BTreeMap::from([(b, bob.peer())]))
            .await
            .unwrap();
        assert!(
            client
                .vote(
                    VoteRequest {
                        vote: Vote::new(99, a),
                        last_log_id: None
                    },
                    option
                )
                .await
                .is_err()
        );
        assert_eq!(raft.metrics().borrow().current_term, before);
    })
    .catch_unwind()
    .await;
    raft.shutdown().await.unwrap();
    task.abort();
    let _ = task.await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

// Private synchronous MLS work and independent voter drivers need separate
// executor threads; the blocking-caller quorum-read regression below proves it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_quic_quorum_orders_crypto_changes_and_survives_creator_control_loss() {
    let directory = crate::test_support::tempdir().unwrap();
    let identities: Vec<_> = (0..3)
        .map(|index| {
            Arc::new(Identity::create(&directory.path().join(format!("identity-{index}"))).unwrap())
        })
        .collect();
    let ids: Vec<_> = identities
        .iter()
        .map(|identity| PeerId::from_peer(&identity.peer()))
        .collect();
    let peers: BTreeMap<_, _> = ids
        .iter()
        .copied()
        .zip(identities.iter().map(|identity| identity.peer()))
        .collect();
    let anchor = RootBundle::create(&identities[0], "Actual TLS quorum fixture")
        .unwrap()
        .verify(&identities[0].peer())
        .unwrap();
    let actor = CryptoMember::new(&identities[0], &anchor, vec![]).unwrap();
    let creator = actor.public_identity().unwrap();
    let mut private_a = Group::create(actor).unwrap();
    let grant = Grant::issue(
        &anchor,
        &[],
        &identities[0],
        &identities[1].peer(),
        Rights::MODERATOR,
    )
    .unwrap();
    let mut candidate_b = CryptoMember::new(&identities[1], &anchor, vec![grant]).unwrap();
    let package = candidate_b.key_package().unwrap();
    let mut factories = Vec::new();
    let mut servers = Vec::new();
    let mut nodes_meta = BTreeMap::new();
    for (index, id) in ids.iter().copied().enumerate() {
        let (factory, server) = Factory::bind(
            anchor.id(),
            identities[index].clone(),
            peers.clone(),
            "127.0.0.1:0".parse().unwrap(),
        )
        .unwrap();
        nodes_meta.insert(
            id,
            Node {
                endpoint: Some(factory.local_addr().unwrap()),
                ..Default::default()
            },
        );
        factories.push(factory);
        servers.push(server);
    }
    let mut config = configuration().unwrap();
    config.heartbeat_interval = 50;
    config.election_timeout_min = 250;
    config.election_timeout_max = 500;
    let config = Arc::new(config.validate().unwrap());
    let mut nodes = BTreeMap::new();
    let mut machines = BTreeMap::new();
    let mut tasks = Vec::new();
    for (index, server) in servers.into_iter().enumerate() {
        let store = LogStore::create(&directory.path().join(format!("control-{index}"))).unwrap();
        let machine = StateMachine::create(store.clone(), &anchor, creator.clone())
            .await
            .unwrap();
        let raft = Raft::new(
            ids[index],
            config.clone(),
            factories[index].clone(),
            store,
            machine.clone(),
        )
        .await
        .unwrap();
        tasks.push(Some(tokio::spawn(
            server.run(raft.clone(), machine.clone()),
        )));
        nodes.insert(ids[index], raft);
        machines.insert(ids[index], machine);
    }
    let outcome = std::panic::AssertUnwindSafe(async {
        nodes[&ids[0]].initialize(nodes_meta).await.unwrap();
        let leader = elected(&nodes[&ids[0]], None).await;
        // Synchronous private MLS work must not suspend all independent voters.
        // Park this caller until a real quorum read finishes on their executor.
        let control = nodes[&leader].clone();
        let (completed, received) = std::sync::mpsc::sync_channel(1);
        let quorum = tokio::spawn(async move {
            let _ = completed.send(control.ensure_linearizable().await);
        });
        received
            .recv_timeout(Duration::from_secs(5))
            .expect("quorum drivers stopped with the synchronous private caller")
            .expect("independent quorum read failed");
        quorum.await.unwrap();
        let view = machines[&leader].public_view().await.unwrap();
        let add = private_a
            .prepare_add(private_a.validate_package(&package).unwrap())
            .unwrap();
        let proposal = super::super::super::Proposal::add(
            &identities[0],
            anchor.id(),
            0,
            view.head(),
            &add,
            &package,
        )
        .unwrap();
        let (leader, receipt) = ordered(&nodes, leader, proposal).await;
        assert!(receipt.accepted);
        let public = machines[&leader].public_view().await.unwrap();
        let welcome = private_a
            .confirm_ordered_view(1, &add, public.membership_digest().unwrap())
            .unwrap()
            .unwrap();
        let mut private_b = Group::join(candidate_b, &welcome, &identities[0].peer(), 1).unwrap();
        nodes[&ids[0]].shutdown().await.unwrap();
        let task = tasks[0].take().unwrap();
        task.abort();
        let _ = task.await;
        let leader = elected(&nodes[&ids[1]], Some(ids[0])).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            while machines[&leader].public_view().await.unwrap().epoch() != 1 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let view = machines[&leader].public_view().await.unwrap();
        let removal = private_b
            .prepare_remove(&identities[0].peer().fingerprint())
            .unwrap();
        let proposal = super::super::super::Proposal::remove(
            &identities[1],
            anchor.id(),
            1,
            view.head(),
            &removal,
            &identities[0].peer(),
        )
        .unwrap();
        let (leader, receipt) = ordered(&nodes, leader, proposal).await;
        assert!(receipt.accepted);
        let view = machines[&leader].public_view().await.unwrap();
        private_b
            .confirm_ordered_view(2, &removal, view.membership_digest().unwrap())
            .unwrap();
        let encrypted = private_b
            .encrypt_text("after creator control loss", false)
            .unwrap();
        assert!(private_a.decrypt_text(&encrypted).is_err());
    })
    .catch_unwind()
    .await;
    for node in nodes.values() {
        let _ = node.shutdown().await;
    }
    for task in tasks.into_iter().flatten() {
        task.abort();
        let _ = task.await;
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
