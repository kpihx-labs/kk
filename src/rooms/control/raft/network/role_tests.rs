use super::super::{LogStore, StateMachine};
use super::*;
use crate::rooms::{RootBundle, mls::CryptoMember};

#[tokio::test]
async fn a_pinned_nonvoter_cannot_inject_votes_leadership_or_snapshots() {
    let directory = crate::test_support::tempdir().unwrap();
    let a = Arc::new(Identity::create(&directory.path().join("a")).unwrap());
    let b = Arc::new(Identity::create(&directory.path().join("b")).unwrap());
    let a_id = PeerId::from_peer(&a.peer());
    let b_id = PeerId::from_peer(&b.peer());
    let root = RootBundle::create(&a, "Control role boundary")
        .unwrap()
        .verify(&a.peer())
        .unwrap();
    let origin = CryptoMember::new(&a, &root, vec![])
        .unwrap()
        .public_identity()
        .unwrap();
    let peers = BTreeMap::from([(a_id, a.peer()), (b_id, b.peer())]);
    let (factory_a, server) = Factory::bind(
        root.id(),
        a.clone(),
        peers.clone(),
        "127.0.0.1:0".parse().unwrap(),
    )
    .unwrap();
    let (factory_b, _unused) =
        Factory::bind(root.id(), b, peers, "127.0.0.1:0".parse().unwrap()).unwrap();
    let node = Node::for_peer(&a.peer(), Some(factory_a.local_addr().unwrap()));
    let store = LogStore::create(&directory.path().join("control")).unwrap();
    let machine = StateMachine::create(store.clone(), &root, origin)
        .await
        .unwrap();
    let raft = Raft::new(
        a_id,
        Arc::new(configuration().unwrap()),
        factory_a.clone(),
        store,
        machine.clone(),
    )
    .await
    .unwrap();
    let serving = tokio::spawn(server.run(raft.clone(), machine));
    raft.initialize(BTreeMap::from([(a_id, node.clone())]))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while raft.current_leader().await != Some(a_id) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let term = raft.metrics().borrow().current_term;
    let vote = openraft::Vote::new(term + 100, b_id);
    let client = Client {
        shared: Arc::downgrade(&factory_b.shared),
        target: a_id,
        node,
    };
    let option = RPCOption::new(Duration::from_secs(1));
    assert!(
        client
            .exchange(
                Request::Vote(VoteRequest {
                    vote,
                    last_log_id: None
                }),
                option.clone()
            )
            .await
            .is_err()
    );
    assert_eq!(raft.metrics().borrow().current_term, term);
    let vote = openraft::Vote::new_committed(term + 100, b_id);
    assert!(
        client
            .exchange(
                Request::Append(AppendEntriesRequest {
                    vote,
                    prev_log_id: None,
                    entries: vec![],
                    leader_commit: None
                }),
                option.clone()
            )
            .await
            .is_err()
    );
    assert_eq!(raft.metrics().borrow().current_term, term);
    assert!(
        client
            .exchange(
                Request::Install(InstallSnapshotRequest {
                    vote,
                    meta: openraft::SnapshotMeta::default(),
                    offset: 0,
                    data: vec![],
                    done: true
                }),
                option
            )
            .await
            .is_err()
    );
    assert_eq!(raft.metrics().borrow().current_term, term);
    raft.shutdown().await.unwrap();
    factory_a.close();
    serving.await.unwrap().unwrap();
}
