use super::*;
use crate::rooms::{Grant, Rights, RootBundle, mls::CryptoMember};
use futures_util::FutureExt;

fn timing() -> openraft::Config {
    let mut config = network::configuration().unwrap();
    config.heartbeat_interval = 50;
    config.election_timeout_min = 200;
    config.election_timeout_max = 400;
    config
}
async fn wait_leader(node: &ControlNode) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while node.current_leader().await.is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn foreground_service_resumes_exact_public_receipts_without_reinitializing() {
    let directory = tempfile::tempdir().unwrap();
    let alice = Arc::new(Identity::create(&directory.path().join("alice")).unwrap());
    let bob = Identity::create(&directory.path().join("bob")).unwrap();
    let anchor = RootBundle::create(&alice, "Service resume fixture")
        .unwrap()
        .verify(&alice.peer())
        .unwrap();
    let creator = CryptoMember::new(&alice, &anchor, vec![])
        .unwrap()
        .public_identity()
        .unwrap();
    let grant = Grant::issue(&anchor, &[], &alice, &bob.peer(), Rights::PARTICIPANT).unwrap();
    let mut bob_actor = CryptoMember::new(&bob, &anchor, vec![grant]).unwrap();
    let package = bob_actor.key_package().unwrap();
    let id = PeerId::from_peer(&alice.peer());
    let peers = BTreeMap::from([(id, alice.peer())]);
    let path = directory.path().join("control");
    let settings = || Settings {
        anchor: anchor.clone(),
        listen: "127.0.0.1:0".parse().unwrap(),
        peers: peers.clone(),
        timing: timing(),
    };
    let node = ControlNode::start(alice.clone(), &path, settings(), Mode::Create(creator))
        .await
        .unwrap();
    // A request already waiting BEFORE initialization must survive the no-leader
    // interval. This is deterministic; a longer VM timeout alone would not fix it.
    let (fresh, ()) = tokio::join!(node.fresh_catch_up(None, Duration::from_secs(5)), async {
        tokio::time::sleep(Duration::from_millis(120)).await;
        node.initialize(BTreeMap::from([(
            id,
            Node {
                endpoint: Some(node.local_addr().unwrap()),
                ..Default::default()
            },
        )]))
        .await
        .unwrap();
        wait_leader(&node).await;
    });
    let (view, records) = fresh.unwrap();
    assert_eq!(view.epoch(), 0);
    assert!(!records.is_empty());
    let proposal = Proposal::add(
        &alice,
        anchor.id(),
        0,
        anchor.id(),
        b"opaque service fixture",
        &package,
    )
    .unwrap();
    let receipt = node
        .submit_public(proposal.clone(), Duration::from_secs(5))
        .await
        .unwrap();
    assert!(receipt.accepted && !receipt.replayed);
    node.shutdown().await.unwrap();
    let resumed = ControlNode::start(alice, &path, settings(), Mode::Resume)
        .await
        .unwrap();
    wait_leader(&resumed).await;
    assert!(
        resumed
            .initialize(BTreeMap::from([(id, Node::default())]))
            .await
            .is_err()
    );
    let replay = resumed
        .submit_public(proposal, Duration::from_secs(5))
        .await
        .unwrap();
    assert!(replay.accepted && replay.replayed);
    assert_eq!(replay.first_log, receipt.first_log);
    assert_eq!(resumed.public_view().await.unwrap().epoch(), 1);
    resumed.shutdown().await.unwrap();
}

#[tokio::test]
async fn original_author_routes_to_remote_leader_over_actual_mutual_tls() {
    let directory = tempfile::tempdir().unwrap();
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
    let anchor = RootBundle::create(&identities[0], "Service remote fixture")
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
    let mut nodes = Vec::new();
    let mut voters = BTreeMap::new();
    for (index, identity) in identities.iter().enumerate() {
        let settings = Settings {
            anchor: anchor.clone(),
            listen: "127.0.0.1:0".parse().unwrap(),
            peers: peers.clone(),
            timing: timing(),
        };
        let node = ControlNode::start(
            identity.clone(),
            &directory.path().join(format!("control-{index}")),
            settings,
            Mode::Create(creator.clone()),
        )
        .await
        .unwrap();
        voters.insert(
            node.id(),
            Node {
                endpoint: Some(node.local_addr().unwrap()),
                ..Default::default()
            },
        );
        nodes.push(node);
    }
    let result = std::panic::AssertUnwindSafe(async {
        nodes[0].initialize(voters).await.unwrap();
        wait_leader(&nodes[0]).await;
        let proposal = Proposal::add(
            &identities[0],
            anchor.id(),
            0,
            anchor.id(),
            b"opaque remote service fixture",
            &package,
        )
        .unwrap();
        assert!(
            nodes[0]
                .submit_public(proposal, Duration::from_secs(5))
                .await
                .unwrap()
                .accepted
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            while nodes[1].public_view().await.unwrap().epoch() != 1
                || nodes[1].current_leader().await.is_none()
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let view = nodes[1].public_view().await.unwrap();
        let removal = Proposal::remove(
            &identities[1],
            anchor.id(),
            1,
            view.head(),
            b"opaque remote removal",
            &identities[0].peer(),
        )
        .unwrap();
        let receipt = nodes[1]
            .submit_public(removal.clone(), Duration::from_secs(5))
            .await
            .unwrap();
        assert!(receipt.accepted);
        let replay = nodes[1]
            .submit_public(removal, Duration::from_secs(5))
            .await
            .unwrap();
        assert!(replay.accepted && replay.replayed);
        assert_eq!(receipt.first_log, replay.first_log);
        // A follower must go through the pinned remote leader and apply its
        // quorum read position before exposing its matching local record/view.
        let (fresh, records) = nodes[1]
            .fresh_catch_up(None, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(fresh.epoch(), 2);
        assert_eq!(records.last().unwrap().epoch(), fresh.epoch());
        let cursor = records.last().unwrap().position().unwrap();
        let (same, pending) = nodes[1]
            .fresh_catch_up(Some(cursor), Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(same.epoch(), 2);
        assert!(pending.iter().all(|record| record.epoch() == 2));
        // Old keys and a cached leader hint do not make an isolated node fresh.
        let leader_id = nodes[0].current_leader().await.unwrap();
        let isolated = nodes
            .iter()
            .position(|node| node.id() == leader_id)
            .unwrap();
        nodes[isolated]
            .network
            .replace_peers(BTreeMap::from([(
                ids[isolated],
                identities[isolated].peer(),
            )]))
            .await
            .unwrap();
        assert!(
            nodes[isolated]
                .fresh_catch_up(Some(cursor), Duration::from_millis(600))
                .await
                .is_err()
        );
        nodes[isolated]
            .network
            .replace_peers(peers.clone())
            .await
            .unwrap();
    })
    .catch_unwind()
    .await;
    for node in nodes {
        node.shutdown().await.unwrap();
    }
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
