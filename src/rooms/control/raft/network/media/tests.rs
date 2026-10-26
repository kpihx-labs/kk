use super::*;

#[test]
fn policy_leases_bind_epoch_direction_generation_expiry_and_explicit_withdrawal() {
    let hub = Hub::new();
    let peer = PeerId([1; 32]);
    *hub.lease.write().unwrap() = Some(Lease {
        policy: MediaPolicy {
            epoch: 7,
            inbound: [peer].into_iter().collect(),
            outbound: Default::default(),
        },
        until: Instant::now() + config::MEDIA_POLICY_LEASE,
    });
    assert!(hub.allowed(peer, true, 7, 0));
    assert!(!hub.allowed(peer, false, 7, 0));
    assert!(!hub.allowed(peer, true, 6, 0));
    assert!(!hub.allowed(peer, true, 7, 1));
    hub.lease.write().unwrap().as_mut().unwrap().until = Instant::now() - Duration::from_secs(1);
    assert!(!hub.allowed(peer, true, 7, 0));
    hub.lease.write().unwrap().as_mut().unwrap().until =
        Instant::now() + config::MEDIA_POLICY_LEASE;
    hub.invalidate_sessions();
    assert!(!hub.allowed(peer, true, 7, 0));
    assert_eq!(hub.generation.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn media_datagrams_need_consent_source_kind_and_exact_connection_and_cannot_grow_the_queue() {
    let root = tempfile::tempdir().unwrap();
    let identity = Arc::new(Identity::create(&root.path().join("owner")).unwrap());
    let sender = Identity::create(&root.path().join("sender"))
        .unwrap()
        .peer();
    let peer = PeerId::from_peer(&sender);
    let (factory, server) = Factory::bind(
        [42; 32],
        identity.clone(),
        BTreeMap::from([
            (PeerId::from_peer(&identity.peer()), identity.peer()),
            (peer, sender),
        ]),
        "127.0.0.1:0".parse().unwrap(),
    )
    .unwrap();
    let shared = &factory.shared;
    let hub = &shared.media;
    shared.media_epoch.store(7, Ordering::SeqCst);
    *hub.lease.write().unwrap() = Some(Lease {
        policy: MediaPolicy {
            epoch: 7,
            inbound: [peer].into_iter().collect(),
            outbound: Default::default(),
        },
        until: Instant::now() + config::MEDIA_POLICY_LEASE,
    });
    let offer = Offer {
        id: [8; 16],
        kind: Kind::Screen,
    };
    assert!(hub.offer(shared, peer, 11, offer.clone()).is_err());
    let (send, mut received) = mpsc::channel(config::MEDIA_PACKET_QUEUE);
    hub.state.lock().unwrap().listeners.insert(
        (peer, Kind::Screen),
        Listener {
            token: 1,
            sender: send,
            active: false,
        },
    );
    hub.offer(shared, peer, 11, offer.clone()).unwrap();
    assert!(
        hub.offer(
            shared,
            peer,
            11,
            Offer {
                id: offer.id,
                kind: Kind::Audio
            }
        )
        .is_err()
    );
    let mut bytes = offer.id.to_vec();
    bytes.extend([0x80, 97, 0, 1, 0, 0, 0, 1, 1, 2, 3, 4, 5]);
    hub.receive(shared, peer, 12, &bytes);
    assert!(received.try_recv().is_err());
    for _ in 0..config::MEDIA_PACKET_QUEUE + 2 {
        hub.receive(shared, peer, 11, &bytes);
    }
    assert_eq!(received.len(), config::MEDIA_PACKET_QUEUE);
    assert_eq!(hub.dropped.load(Ordering::Relaxed), 2);
    assert_eq!(
        received.try_recv().unwrap().frame.packet.bytes(),
        &bytes[16..]
    );
    shared.media_epoch.store(8, Ordering::SeqCst);
    let before = received.len();
    hub.receive(shared, peer, 11, &bytes);
    assert_eq!(received.len(), before);
    let drivers = factory.driver_owner();
    factory.close();
    drop(server);
    drop(factory);
    drivers.terminate();
    drivers.released().await.unwrap();
}
