use super::*;

#[tokio::test]
async fn detached_replication_clients_cannot_keep_an_owned_listener_bound() {
    let directory = tempfile::tempdir().unwrap();
    let identity = Arc::new(Identity::create(&directory.path().join("identity")).unwrap());
    let id = PeerId::from_peer(&identity.peer());
    let (mut factory, server) = Factory::bind(
        [17; 32],
        identity.clone(),
        BTreeMap::from([(id, identity.peer())]),
        "127.0.0.1:0".parse().unwrap(),
    )
    .unwrap();
    let address = factory.local_addr().unwrap();
    let client = factory
        .new_client(id, &Node::for_peer(&identity.peer(), Some(address)))
        .await;
    let owner = factory.driver_owner();
    factory.close();
    factory.wait_idle().await.unwrap();
    drop(server);
    drop(factory);
    owner.released().await.unwrap();
    let rebound = std::net::UdpSocket::bind(address).unwrap();
    assert_eq!(rebound.local_addr().unwrap(), address);
    assert!(matches!(
        client
            .exchange(
                Request::ReadBarrier,
                RPCOption::new(Duration::from_millis(50))
            )
            .await,
        Err(Error::Cancelled)
    ));
}

#[tokio::test]
async fn cancelled_rpc_resets_only_its_stream_and_preserves_the_authenticated_peer_link() {
    let directory = tempfile::tempdir().unwrap();
    let alice = Arc::new(Identity::create(&directory.path().join("alice")).unwrap());
    let bob = Arc::new(Identity::create(&directory.path().join("bob")).unwrap());
    let a = PeerId::from_peer(&alice.peer());
    let b = PeerId::from_peer(&bob.peer());
    let peers = BTreeMap::from([(a, alice.peer()), (b, bob.peer())]);
    let room = [17; 32];
    let (client, _unused) =
        Factory::bind(room, alice, peers.clone(), "127.0.0.1:0".parse().unwrap()).unwrap();
    let (server_factory, server) =
        Factory::bind(room, bob, peers, "127.0.0.1:0".parse().unwrap()).unwrap();
    let node = Node {
        endpoint: Some(server_factory.local_addr().unwrap()),
        ..Default::default()
    };
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    // Deliberately delayed raw TRANSPORT fixture. Its synthetic reply is not a
    // quorum/room acceptance proof and never enters a private Group.
    let serving = tokio::spawn(async move {
        let incoming = server.shared.endpoint.accept().await.unwrap();
        let connection = incoming.await.unwrap();
        assert_eq!(
            PeerId::from_peer(&transport::peer_identity(&connection).unwrap()),
            a
        );
        let (_blocked_send, mut recv) = connection.accept_bi().await.unwrap();
        let first: Envelope = read_frame(&mut recv).await.unwrap();
        assert_eq!(first.room, room);
        assert!(matches!(first.request, Request::ReadBarrier));
        entered.send(()).unwrap();
        let (mut send, mut recv) = connection.accept_bi().await.unwrap();
        let next: Envelope = read_frame(&mut recv).await.unwrap();
        assert_eq!(next.room, room);
        assert!(matches!(next.request, Request::ReadBarrier));
        send_frame(&mut send, &Response::ReadBarrier(Reply::Applied(None)))
            .await
            .unwrap();
        send.finish().unwrap();
        let _ = released.await;
    });
    let client = Arc::new(client);
    let caller = client.clone();
    let target = node.clone();
    let pending = tokio::spawn(async move {
        caller
            .read_barrier(b, &target, Duration::from_secs(5))
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), waiting)
        .await
        .unwrap()
        .unwrap();
    let slot = client.shared.slots.lock().await.get(&b).unwrap().clone();
    let cached = slot.lock().await.clone().unwrap().connection;
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    assert!(
        cached.close_reason().is_none(),
        "one cancelled stream closed its shared peer connection"
    );
    assert!(matches!(
        client
            .read_barrier(b, &node, Duration::from_secs(2))
            .await
            .unwrap(),
        Reply::Applied(None)
    ));
    // A nonresponsive operation must retire the cache for future attempts, but
    // cannot kill other holders of the same authenticated multiplexed link.
    assert!(matches!(
        client
            .read_barrier(b, &node, Duration::from_millis(50))
            .await,
        Err(Error::Timeout)
    ));
    assert!(
        slot.lock().await.is_none(),
        "unresponsive peer link stayed cached after its deadline"
    );
    assert!(
        cached.close_reason().is_none(),
        "cache retirement closed another holder's live connection"
    );
    release.send(()).unwrap();
    serving.await.unwrap();
}
