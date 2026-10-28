use super::*;

// Synthetic public identities and the existing bound factory/hub harness only.
// Hub tests inject RTP directly; the QUIC test uses actual loopback UDP sockets.
// No test captures devices or private data.
fn screen_harness(
    count: usize,
) -> (
    tempfile::TempDir,
    Factory,
    super::super::Server,
    Vec<PeerId>,
) {
    assert!(count < config::ROOM_MAX_MEMBERS);
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
async fn max_room_all_kinds_and_queue_bounds_survive_duplicate_and_excess_sessions() {
    let (_root, factory, server, peers) = screen_harness(config::ROOM_MAX_MEMBERS - 1);
    let shared = &factory.shared;
    let hub = &shared.media;
    let mut subscriptions = Vec::new();
    let kinds = [Kind::Audio, Kind::Camera, Kind::Screen];
    assert_eq!(kinds.len(), config::MEDIA_SOURCE_KINDS);
    for (index, peer) in peers.iter().enumerate() {
        for (source, kind) in kinds.into_iter().enumerate() {
            let subscription = factory.subscribe_media(*peer, kind).await.unwrap();
            assert!(factory.subscribe_media(*peer, kind).await.is_err());
            // IDs collide across peers, but not across kinds of a single peer.
            let offer = Offer {
                id: [source as u8; 16],
                kind,
            };
            hub.offer(shared, *peer, 11 + index, offer.clone()).unwrap();
            subscriptions.push((*peer, 11 + index, offer, subscription));
        }
    }
    let before = factory.media_status().unwrap();
    assert_eq!(before.subscriptions, config::MEDIA_MAX_STREAMS);
    assert_eq!(before.active_tracks, config::MEDIA_MAX_STREAMS);
    assert!(matches!(
        factory.subscribe_media(peers[0], Kind::Screen).await,
        Err(Error::Node(_))
    ));
    for id in [[9; 16], [2; 16]] {
        // Neither another ID nor the same ID on another connection may take over.
        assert!(matches!(
            hub.offer(
                shared,
                peers[0],
                99,
                Offer {
                    id,
                    kind: Kind::Screen
                }
            ),
            Err(Error::Node(_))
        ));
    }
    let outsider = PeerId([0xff; 32]);
    assert!(!peers.contains(&outsider));
    assert!(matches!(
        factory.subscribe_media(outsider, Kind::Screen).await,
        Err(Error::Node(_))
    ));
    assert!(matches!(
        hub.offer(shared, outsider, 100, screen_offer()),
        Err(Error::Denied)
    ));

    // Reserve all three local sources while all remote peer/kind slots are live.
    // These are publication-side Usage reservations, not actual RTP publications.
    let reservations: Vec<_> = (0..config::MEDIA_MAX_PUBLICATIONS)
        .map(|_| hub.use_media(Arc::downgrade(shared)).unwrap())
        .collect();
    assert_eq!(
        factory.media_status().unwrap().sessions,
        config::MEDIA_MAX_USERS
    );
    assert!(matches!(
        hub.use_media(Arc::downgrade(shared)),
        Err(Error::Node(_))
    ));
    let after = factory.media_status().unwrap();
    assert_eq!(after.subscriptions, before.subscriptions);
    assert_eq!(after.active_tracks, before.active_tracks);
    assert_eq!(after.dropped_packets, before.dropped_packets);
    for (index, (peer, connection, offer, subscription)) in subscriptions.iter_mut().enumerate() {
        let mut bytes = offer.id.to_vec();
        bytes.extend([
            0x80,
            offer.kind.codec().payload_type(),
            0,
            1,
            0,
            0,
            0,
            1,
            1,
            2,
            3,
            4,
        ]);
        bytes.resize(16 + config::MEDIA_PACKET_BYTES, index as u8);
        hub.receive(shared, *peer, 99, &bytes);
        assert!(subscription.receiver.try_recv().is_err());
        for _ in 0..config::MEDIA_PACKET_QUEUE + 2 {
            hub.receive(shared, *peer, *connection, &bytes);
        }
        assert_eq!(subscription.receiver.len(), config::MEDIA_PACKET_QUEUE);
    }
    assert_eq!(
        hub.dropped.load(Ordering::Relaxed),
        config::MEDIA_MAX_STREAMS * 2
    );
    assert_eq!(
        subscriptions
            .iter()
            .map(|(_, _, _, sub)| sub.receiver.len())
            .sum::<usize>(),
        config::MEDIA_MAX_STREAMS * config::MEDIA_PACKET_QUEUE
    );
    for (index, (_, _, offer, subscription)) in subscriptions.iter_mut().enumerate() {
        for _ in 0..config::MEDIA_PACKET_QUEUE {
            let frame = subscription.next().await.unwrap();
            assert_eq!(frame.track, offer.id);
            assert_eq!(frame.packet.codec(), offer.kind.codec());
            assert_eq!(frame.packet.bytes().len(), config::MEDIA_PACKET_BYTES);
            assert_eq!(frame.packet.bytes()[12], index as u8);
        }
        assert!(subscription.receiver.try_recv().is_err());
    }
    drop(reservations);
    assert_eq!(factory.media_status().unwrap().sessions, before.sessions);
    drop(subscriptions);
    assert_eq!(factory.media_status().unwrap().sessions, 0);
    // Teardown and generation invalidation leave no live tracks behind.
    hub.invalidate_sessions();
    assert_eq!(factory.media_status().unwrap().active_tracks, 0);
    close_screen_harness(factory, server).await;
}

#[tokio::test(flavor = "current_thread")]
async fn local_publication_reservations_cannot_consume_remote_subscription_budget() {
    let hub = Hub::new();
    let local: Vec<_> = (0..config::MEDIA_MAX_PUBLICATIONS)
        .map(|_| hub.use_media(Weak::new()).unwrap())
        .collect();
    assert!(hub.use_media(Weak::new()).is_err());
    let remote: Vec<_> = (0..config::MEDIA_MAX_STREAMS)
        .map(|_| hub.reserve(Weak::new(), false).unwrap())
        .collect();
    assert!(hub.reserve(Weak::new(), false).is_err());
    assert_eq!(hub.status(0).unwrap().sessions, config::MEDIA_MAX_USERS);
    drop(local);
    assert!(hub.reserve(Weak::new(), false).is_err());
    let replacement = hub.use_media(Weak::new()).unwrap();
    drop(remote);
    drop(replacement);
    assert_eq!(hub.status(0).unwrap().sessions, 0);
    hub.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn three_peer_quic_full_meeting_publishes_and_receives_all_kinds_with_control_progress() {
    use super::super::super::{LogStore, StateMachine};
    use crate::rooms::{RootBundle, mls::CryptoMember};
    use futures_util::FutureExt;
    use std::collections::BTreeSet;

    let root = tempfile::tempdir().unwrap();
    let identities: Vec<_> = (0..3)
        .map(|i| Arc::new(Identity::create(&root.path().join(format!("peer-{i}"))).unwrap()))
        .collect();
    let ids: Vec<_> = identities
        .iter()
        .map(|i| PeerId::from_peer(&i.peer()))
        .collect();
    let peers: BTreeMap<_, _> = ids
        .iter()
        .copied()
        .zip(identities.iter().map(|i| i.peer()))
        .collect();
    let anchor = RootBundle::create(&identities[0], "synthetic media meeting")
        .unwrap()
        .verify(&identities[0].peer())
        .unwrap();
    let creator = CryptoMember::new(&identities[0], &anchor, vec![])
        .unwrap()
        .public_identity()
        .unwrap();
    let mut factories = Vec::new();
    let mut servers = Vec::new();
    let mut nodes = BTreeMap::new();
    for identity in identities {
        let id = PeerId::from_peer(&identity.peer());
        let (factory, server) = Factory::bind(
            anchor.id(),
            identity,
            peers.clone(),
            "127.0.0.1:0".parse().unwrap(),
        )
        .unwrap();
        nodes.insert(
            id,
            Node {
                endpoint: Some(factory.local_addr().unwrap()),
                ..Default::default()
            },
        );
        factories.push(factory);
        servers.push(server);
    }
    let mut rafts = Vec::new();
    let mut tasks = Vec::new();
    for (i, server) in servers.into_iter().enumerate() {
        let store = LogStore::create(&root.path().join(format!("control-{i}"))).unwrap();
        let machine = StateMachine::create(store.clone(), &anchor, creator.clone())
            .await
            .unwrap();
        let raft = Raft::new(
            ids[i],
            Arc::new(configuration().unwrap()),
            factories[i].clone(),
            store,
            machine.clone(),
        )
        .await
        .unwrap();
        tasks.push(tokio::spawn(server.run(raft.clone(), machine)));
        rafts.push(raft);
    }
    let outcome = std::panic::AssertUnwindSafe(async {
        rafts[0].initialize(nodes.clone()).await.unwrap();
        for raft in &rafts {
            tokio::time::timeout(
                Duration::from_secs(5),
                raft.metrics().wait_for(|m| {
                    m.current_leader.is_some()
                        && m.membership_config.membership().nodes().count() == 3
                }),
            )
            .await
            .unwrap()
            .unwrap();
        }
        let kinds = [Kind::Audio, Kind::Camera, Kind::Screen];
        let mut subscriptions = Vec::new();
        for (i, factory) in factories.iter().enumerate() {
            let remote: BTreeSet<_> = ids.iter().copied().filter(|id| *id != ids[i]).collect();
            factory.shared.media_epoch.store(7, Ordering::SeqCst);
            *factory.shared.media.lease.write().unwrap() = Some(Lease {
                policy: MediaPolicy {
                    epoch: 7,
                    inbound: remote.clone(),
                    outbound: remote.clone(),
                },
                until: Instant::now() + config::MEDIA_POLICY_LEASE,
            });
            for peer in remote {
                for kind in kinds {
                    subscriptions.push((
                        i,
                        peer,
                        kind,
                        factory.subscribe_media(peer, kind).await.unwrap(),
                    ));
                }
            }
            // This transport fixture has no room actor. Keep its explicit synthetic
            // consent lease, rather than letting the absent actor overwrite it.
            factory
                .shared
                .media
                .state
                .lock()
                .unwrap()
                .refresh
                .as_ref()
                .unwrap()
                .abort();
            assert_eq!(factory.media_status().unwrap().subscriptions, 6);
        }
        let mut publications = Vec::new();
        for (i, factory) in factories.iter().enumerate() {
            for kind in kinds {
                publications.push((
                    i,
                    kind,
                    factory
                        .publish_media(kind, vec![], rafts[i].metrics())
                        .await
                        .unwrap(),
                ));
            }
            assert!(
                factory
                    .publish_media(Kind::Screen, vec![], rafts[i].metrics())
                    .await
                    .is_err()
            );
            assert_eq!(factory.media_status().unwrap().sessions, 9);
        }
        // A fourth publication/duplicate offer has not replaced established sources.
        // Send and consume concurrently on every endpoint, with an actual control RPC.
        let send = async {
            for sequence in 1u16..=3 {
                let results = futures_util::future::join_all(publications.iter_mut().map(
                    |(i, kind, publication)| {
                        let mut bytes = vec![0x80, kind.codec().payload_type()];
                        bytes.extend(sequence.to_be_bytes());
                        bytes.extend([0, 0, 0, 1, 1, 2, 3, 4, *i as u8]);
                        let packet = Packet::parse(kind.codec(), bytes).unwrap();
                        async move {
                            publication.send(&packet).await.unwrap();
                        }
                    },
                ))
                .await;
                assert_eq!(results.len(), 9);
            }
        };
        let receive = async {
            futures_util::future::join_all(subscriptions.iter_mut().map(
                |(_, peer, kind, subscription)| {
                    let sender = ids.iter().position(|id| id == peer).unwrap();
                    async move {
                        for sequence in 1u16..=3 {
                            let frame = subscription.next().await.unwrap();
                            assert_eq!(frame.packet.codec(), kind.codec());
                            assert_eq!(&frame.packet.bytes()[2..4], &sequence.to_be_bytes());
                            assert_eq!(frame.packet.bytes()[12], sender as u8);
                        }
                    }
                },
            ))
            .await;
        };
        let control = async {
            let response = factories[0]
                .read_barrier(ids[1], &nodes[&ids[1]], Duration::from_secs(1))
                .await;
            assert!(
                response.is_ok(),
                "control RPC must progress during media: {response:?}"
            );
        };
        tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(send, receive, control);
        })
        .await
        .unwrap();
        for factory in &factories {
            assert_eq!(factory.media_status().unwrap().active_tracks, 6);
        }
        for (_, _, publication) in publications {
            publication.finish().await.unwrap();
        }
        drop(subscriptions);
    })
    .catch_unwind()
    .await;
    for raft in rafts {
        raft.shutdown().await.unwrap();
    }
    for task in tasks {
        task.abort();
        let _ = task.await;
    }
    for factory in factories {
        factory.shared.media.shutdown().await.unwrap();
        let drivers = factory.driver_owner();
        factory.close();
        drop(factory);
        drivers.terminate();
        drivers.released().await.unwrap();
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
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
    let mut subscription = factory.subscribe_media(peer, Kind::Screen).await.unwrap();
    let received = &mut subscription.receiver;
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
    drop(subscription);
    let drivers = factory.driver_owner();
    factory.close();
    drop(server);
    drop(factory);
    drivers.terminate();
    drivers.released().await.unwrap();
}
