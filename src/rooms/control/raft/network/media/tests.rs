use super::*;

// Synthetic public identities and the existing bound factory/hub harness only.
// These tests inject RTP into Hub::receive; they do not capture or send media.
fn screen_harness(
    count: usize,
) -> (
    tempfile::TempDir,
    Factory,
    super::super::Server,
    Vec<PeerId>,
) {
    let root = tempfile::tempdir().unwrap();
    let identity = Arc::new(Identity::create(&root.path().join("owner")).unwrap());
    let mut members = BTreeMap::from([(PeerId::from_peer(&identity.peer()), identity.peer())]);
    let mut peers = Vec::new();
    for index in 0..count {
        let peer = Identity::create(&root.path().join(format!("sender-{index}")))
            .unwrap()
            .peer();
        let id = PeerId::from_peer(&peer);
        members.insert(id, peer);
        peers.push(id);
    }
    let (factory, server) =
        Factory::bind([42; 32], identity, members, "127.0.0.1:0".parse().unwrap()).unwrap();
    factory.shared.media_epoch.store(7, Ordering::SeqCst);
    *factory.shared.media.lease.write().unwrap() = Some(Lease {
        policy: MediaPolicy {
            epoch: 7,
            inbound: peers.iter().copied().collect(),
            outbound: peers.iter().copied().collect(),
        },
        until: Instant::now() + config::MEDIA_POLICY_LEASE,
    });
    (root, factory, server, peers)
}

fn screen_offer() -> Offer {
    Offer {
        id: [8; 16],
        kind: Kind::Screen,
    }
}

fn screen_datagram(sequence: u16, marker: u8) -> Vec<u8> {
    let mut bytes = screen_offer().id.to_vec();
    bytes.extend([0x80, 97]);
    bytes.extend(sequence.to_be_bytes());
    // Deliberately identical timestamps and SSRC across all remote sources.
    bytes.extend([0, 0, 0, 1, 1, 2, 3, 4, marker]);
    bytes
}

async fn expect_screen(subscription: &mut Subscription, bytes: &[u8]) {
    let frame = tokio::time::timeout(Duration::from_secs(1), subscription.next())
        .await
        .expect("already queued screen frame must not wait")
        .unwrap();
    assert_eq!(frame.track, screen_offer().id);
    assert_eq!(frame.packet.bytes(), &bytes[16..]);
}

async fn close_screen_harness(factory: Factory, server: super::super::Server) {
    factory.shared.media.shutdown().await.unwrap();
    let drivers = factory.driver_owner();
    factory.close();
    drop(server);
    drop(factory);
    drivers.terminate();
    drivers.released().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn simultaneous_screens_isolate_colliding_tracks_and_ssrc_through_stop_and_revocation() {
    let (_root, factory, server, peers) = screen_harness(2);
    let shared = &factory.shared;
    let hub = &shared.media;
    let mut first = factory
        .subscribe_media(peers[0], Kind::Screen)
        .await
        .unwrap();
    let mut second = factory
        .subscribe_media(peers[1], Kind::Screen)
        .await
        .unwrap();
    hub.offer(shared, peers[0], 11, screen_offer()).unwrap();
    hub.offer(shared, peers[1], 12, screen_offer()).unwrap();
    assert_eq!(factory.media_status().unwrap().active_tracks, 2);

    for sequence in 1..=3 {
        let a = screen_datagram(sequence, 0xa1);
        let b = screen_datagram(sequence, 0xb2);
        hub.receive(shared, peers[1], 12, &b);
        hub.receive(shared, peers[0], 11, &a);
        assert_eq!(first.receiver.len(), usize::from(sequence));
        assert_eq!(second.receiver.len(), usize::from(sequence));
    }
    // Even with colliding IDs, the other source's connection is not accepted.
    hub.receive(shared, peers[0], 12, &screen_datagram(4, 0xee));
    hub.receive(shared, peers[1], 11, &screen_datagram(4, 0xff));
    assert_eq!(first.receiver.len(), 3);
    assert_eq!(second.receiver.len(), 3);
    for sequence in 1..=3 {
        expect_screen(&mut first, &screen_datagram(sequence, 0xa1)).await;
        expect_screen(&mut second, &screen_datagram(sequence, 0xb2)).await;
    }
    assert!(first.receiver.try_recv().is_err());
    assert!(second.receiver.try_recv().is_err());

    hub.end(peers[0], screen_offer().id);
    {
        let mut state = hub.state.lock().unwrap();
        assert!(
            state.tracks[&(peers[0], screen_offer().id)]
                .ending
                .is_some()
        );
        assert!(
            state.tracks[&(peers[1], screen_offer().id)]
                .ending
                .is_none()
        );
        // Advance only this track's grace deadline, without wall-clock sleeps.
        state
            .tracks
            .get_mut(&(peers[0], screen_offer().id))
            .unwrap()
            .ending = Some(Instant::now() - Duration::from_secs(1));
    }
    hub.receive(shared, peers[0], 11, &screen_datagram(4, 0xa1));
    assert!(first.receiver.try_recv().is_err());
    assert!(matches!(first.next().await, Err(Error::Cancelled)));
    hub.receive(shared, peers[1], 12, &screen_datagram(4, 0xb2));
    expect_screen(&mut second, &screen_datagram(4, 0xb2)).await;
    drop(first);
    assert_eq!(factory.media_status().unwrap().subscriptions, 1);

    // Restart just the stopped source, then revoke it while a frame is queued.
    let mut first = factory
        .subscribe_media(peers[0], Kind::Screen)
        .await
        .unwrap();
    hub.offer(shared, peers[0], 13, screen_offer()).unwrap();
    hub.receive(shared, peers[0], 13, &screen_datagram(5, 0xa1));
    assert_eq!(first.receiver.len(), 1);
    hub.lease
        .write()
        .unwrap()
        .as_mut()
        .unwrap()
        .policy
        .inbound
        .remove(&peers[0]);
    assert!(matches!(first.next().await, Err(Error::Denied)));
    hub.receive(shared, peers[0], 13, &screen_datagram(6, 0xa1));
    assert_eq!(first.receiver.len(), 1);
    assert!(matches!(
        hub.offer(shared, peers[0], 13, screen_offer()),
        Err(Error::Denied)
    ));
    hub.receive(shared, peers[1], 12, &screen_datagram(5, 0xb2));
    expect_screen(&mut second, &screen_datagram(5, 0xb2)).await;
    drop(first);
    drop(second);
    close_screen_harness(factory, server).await;
}

#[tokio::test(flavor = "current_thread")]
async fn screen_duplicate_and_capacity_rejections_preserve_established_sessions() {
    let (_root, factory, server, peers) = screen_harness(config::MEDIA_MAX_STREAMS + 1);
    let shared = &factory.shared;
    let hub = &shared.media;
    let mut subscriptions = Vec::new();
    for (index, peer) in peers.iter().take(config::MEDIA_MAX_STREAMS).enumerate() {
        subscriptions.push(factory.subscribe_media(*peer, Kind::Screen).await.unwrap());
        hub.offer(shared, *peer, 11 + index, screen_offer())
            .unwrap();
    }
    let before = factory.media_status().unwrap();
    assert_eq!(before.subscriptions, config::MEDIA_MAX_STREAMS);
    assert_eq!(before.active_tracks, config::MEDIA_MAX_STREAMS);
    assert!(matches!(
        factory.subscribe_media(peers[0], Kind::Screen).await,
        Err(Error::Node(_))
    ));
    let duplicate = Offer {
        id: [9; 16],
        kind: Kind::Screen,
    };
    assert!(matches!(
        hub.offer(shared, peers[0], 99, duplicate),
        Err(Error::Node(_))
    ));
    assert!(matches!(
        factory
            .subscribe_media(peers[config::MEDIA_MAX_STREAMS], Kind::Screen)
            .await,
        Err(Error::Node(_))
    ));

    // Isolate the incoming-track budget independently of the subscription budget.
    // This fifth listener is a fixture insertion, not an accepted subscription.
    let extra_peer = peers[config::MEDIA_MAX_STREAMS];
    let (sender, mut rejected_queue) = mpsc::channel(config::MEDIA_PACKET_QUEUE);
    hub.state.lock().unwrap().listeners.insert(
        (extra_peer, Kind::Screen),
        Listener {
            token: u64::MAX,
            sender,
            active: false,
        },
    );
    assert!(matches!(
        hub.offer(shared, extra_peer, 100, screen_offer()),
        Err(Error::Node(_))
    ));
    hub.receive(shared, extra_peer, 100, &screen_datagram(1, 0xee));
    assert!(rejected_queue.try_recv().is_err());
    assert!(
        !hub.state
            .lock()
            .unwrap()
            .tracks
            .contains_key(&(extra_peer, screen_offer().id))
    );
    hub.state
        .lock()
        .unwrap()
        .listeners
        .remove(&(extra_peer, Kind::Screen));

    // Reserve the remaining combined users while all incoming screens are live.
    // These are publication-side Usage reservations, not actual RTP publications.
    let reservations: Vec<_> = (0..config::MEDIA_MAX_STREAMS)
        .map(|_| hub.use_media(Arc::downgrade(shared)).unwrap())
        .collect();
    assert_eq!(
        factory.media_status().unwrap().sessions,
        config::MEDIA_MAX_STREAMS * 2
    );
    assert!(matches!(
        hub.use_media(Arc::downgrade(shared)),
        Err(Error::Node(_))
    ));
    let after = factory.media_status().unwrap();
    assert_eq!(after.subscriptions, before.subscriptions);
    assert_eq!(after.active_tracks, before.active_tracks);
    assert_eq!(after.dropped_packets, before.dropped_packets);
    for (index, subscription) in subscriptions.iter_mut().enumerate() {
        let bytes = screen_datagram(1, index as u8);
        hub.receive(shared, peers[index], 11 + index, &bytes);
        expect_screen(subscription, &bytes).await;
        assert!(subscription.receiver.try_recv().is_err());
    }
    drop(reservations);
    assert_eq!(factory.media_status().unwrap().sessions, before.sessions);
    drop(subscriptions);
    close_screen_harness(factory, server).await;
}

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
