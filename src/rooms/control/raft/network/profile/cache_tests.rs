use super::*;

fn fixture() -> (Cache, PeerId, Instant) {
    let own = PeerId([1; 32]);
    let peer = PeerId([2; 32]);
    let mut cache = Cache::new([3; 32], own);
    cache.register(4, &[own, peer]).unwrap();
    (cache, peer, Instant::now())
}
fn response(epoch: u64, name: Option<&str>) -> Result<ProfileMetadata> {
    Ok(ProfileMetadata {
        version: 1,
        epoch,
        name: name.map(str::to_owned),
    })
}

#[test]
fn clocked_ttl_none_errors_backoff_and_singleflight_never_return_expired_names() {
    let (mut cache, peer, now) = fixture();
    let ticket = cache.reserve(peer, now).unwrap();
    assert!(!cache.due(peer, now));
    assert!(cache.reserve(peer, now).is_err());
    cache.complete(ticket, response(4, Some("Alice")), now);
    assert_eq!(cache.names(now)[&peer], "Alice");
    let before = now + config::ROOM_PROFILE_CACHE_TTL - Duration::from_nanos(1);
    assert!(!cache.due(peer, before));
    assert_eq!(cache.names(before).len(), 1);
    let expired = now + config::ROOM_PROFILE_CACHE_TTL;
    assert!(cache.names(expired).is_empty());
    assert!(cache.due(peer, expired));
    let ticket = cache.reserve(peer, expired).unwrap();
    cache.complete(ticket, response(4, None), expired);
    assert!(cache.names(expired).is_empty());
    assert!(!cache.due(
        peer,
        expired + config::ROOM_PROFILE_CACHE_BACKOFF - Duration::from_nanos(1)
    ));
    let retry = expired + config::ROOM_PROFILE_CACHE_BACKOFF;
    let ticket = cache.reserve(peer, retry).unwrap();
    cache.complete(ticket, Err(Error::Timeout), retry);
    assert!(cache.names(retry).is_empty());
    assert!(!cache.due(peer, retry));
    assert!(cache.due(peer, retry + config::ROOM_PROFILE_CACHE_BACKOFF));
    assert_eq!(cache.counters.started, 3);
    assert_eq!(cache.counters.completed, 3);
}

#[test]
fn epochs_membership_rollbacks_restart_and_stop_discard_delayed_results() {
    let (mut cache, peer, now) = fixture();
    let own = cache.own;
    let old = cache.reserve(peer, now).unwrap();
    assert!(cache.observe(5, &[own, peer]).unwrap());
    assert!(!cache.observe(4, &[own, peer]).unwrap());
    // Abort is requested, but no replacement is allowed until old work is gone.
    assert!(!cache.due(peer, now));
    cache.complete(old, response(4, Some("old epoch")), now);
    assert!(cache.names(now).is_empty());
    assert_eq!(cache.counters.discarded, 1);
    let flight = cache.reserve(peer, now).unwrap();
    assert!(cache.observe(5, &[own]).unwrap());
    assert!(!cache.observe(5, &[own, peer]).unwrap());
    cache.complete(flight, response(5, Some("removed")), now);
    assert!(cache.names(now).is_empty());
    assert_eq!(cache.counters.discarded, 2);
    cache.register(5, &[own, peer]).unwrap();
    let flight = cache.reserve(peer, now).unwrap();
    cache.suspend();
    cache.register(5, &[own, peer]).unwrap();
    cache.complete(flight, response(5, Some("previous registration")), now);
    assert!(cache.names(now).is_empty());
    assert_eq!(cache.counters.discarded, 3);
    let flight = cache.reserve(peer, now).unwrap();
    cache.suspend();
    cache.complete(flight, response(5, Some("stopped")), now);
    assert!(!cache.active && cache.names(now).is_empty());
    assert!(cache.register(4, &[own, peer]).is_err());
    assert_eq!(cache.counters.discarded, 4);
}

#[test]
fn result_binding_cardinality_self_filtering_and_cancellation_are_exact() {
    let (mut cache, peer, now) = fixture();
    assert_eq!(cache.members, BTreeSet::from([peer]));
    assert!(member_set(cache.own, &[peer, peer]).is_err());
    assert!(member_set(cache.own, &vec![peer; config::ROOM_MAX_MEMBERS + 1]).is_err());
    for response in [
        response(3, Some("wrong epoch")),
        response(4, Some("invalid\nname")),
        Ok(ProfileMetadata {
            version: 2,
            epoch: 4,
            name: Some("unsupported".into()),
        }),
    ] {
        let ticket = cache.reserve(peer, now).unwrap();
        cache.complete(ticket, response, now);
        assert!(cache.names(now).is_empty());
        cache.entries.clear();
    }
    let ticket = cache.reserve(peer, now).unwrap();
    let forged = Ticket {
        room: [9; 32],
        ..ticket
    };
    cache.complete(forged, response(4, Some("different room")), now);
    assert_eq!(cache.flights.len(), 1);
    cache.cancelled(ticket, now);
    assert!(cache.flights.is_empty());
    assert!(!cache.due(peer, now));
    assert_eq!(cache.counters.cancelled, 1);
}

#[tokio::test]
async fn cache_work_uses_owner_cancellation_and_sealed_spawn_drops_reservations() {
    let directory = crate::test_support::tempdir().unwrap();
    let identity = Arc::new(Identity::create(&directory.path().join("identity")).unwrap());
    let own = PeerId::from_peer(&identity.peer());
    let peer = PeerId([2; 32]);
    let (factory, server) = Factory::bind(
        [3; 32],
        identity.clone(),
        BTreeMap::from([(own, identity.peer())]),
        "127.0.0.1:0".parse().unwrap(),
    )
    .unwrap();
    let ticket = {
        let mut cache = factory.shared.profile_cache.lock().unwrap();
        cache.register(4, &[own, peer]).unwrap();
        cache.reserve(peer, Instant::now()).unwrap()
    };
    let refresh = Refresh {
        shared: Arc::downgrade(&factory.shared),
        ticket,
    };
    let (entered, waiting) = tokio::sync::oneshot::channel();
    factory.shared.drivers.spawn(async move {
        let _refresh = refresh;
        entered.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    waiting.await.unwrap();
    factory.shared.drivers.terminate();
    factory.shared.drivers.released().await.unwrap();
    assert_eq!(factory.profile_cache_probe().3, 1);
    assert_eq!(factory.profile_cache_probe().4, 0);
    let ticket = factory
        .shared
        .profile_cache
        .lock()
        .unwrap()
        .reserve(peer, Instant::now() + config::ROOM_PROFILE_CACHE_BACKOFF)
        .unwrap();
    let refresh = Refresh {
        shared: Arc::downgrade(&factory.shared),
        ticket,
    };
    assert!(
        factory
            .shared
            .drivers
            .spawn(async move {
                let _refresh = refresh;
                std::future::pending::<()>().await;
            })
            .is_none()
    );
    assert_eq!(factory.profile_cache_probe().3, 2);
    assert_eq!(factory.profile_cache_probe().4, 0);
    factory.close();
    drop(server);
}

#[test]
fn shared_raw_budget_is_reserved_before_spawn_and_has_no_waiter_queue() {
    let own = PeerId([1; 32]);
    let members: Vec<_> = (1..=8).map(|id| PeerId([id; 32])).collect();
    let mut caches = [Cache::new([3; 32], own), Cache::new([4; 32], own)];
    for cache in &mut caches {
        cache.register(4, &members).unwrap();
    }
    let budget = Semaphore::new(config::ROOM_PROFILE_RPCS);
    let now = Instant::now();
    let mut admitted = Vec::new();
    for peer in &members[1..] {
        if let Ok(work) = caches[0].admit(*peer, now, &budget) {
            admitted.push(work);
        }
    }
    assert_eq!(admitted.len(), 4);
    assert_eq!(budget.available_permits(), 0);
    for peer in &members[1..] {
        assert!(caches[0].admit(*peer, now, &budget).is_err());
        assert!(caches[1].admit(*peer, now, &budget).is_err());
    }
    assert_eq!(caches[0].counters.started, 4);
    assert_eq!(caches[1].counters.started, 0);
    drop(admitted);
    assert_eq!(budget.available_permits(), 4);
    // The second room obeys the same budget, and failed admission backs off.
    assert!(caches[1].admit(members[1], now, &budget).is_err());
    assert!(
        caches[1]
            .admit(
                members[1],
                now + config::ROOM_PROFILE_CACHE_BACKOFF,
                &budget
            )
            .is_ok()
    );
}

#[test]
fn committed_route_snapshots_cannot_roll_back_or_publish_delayed_route_results() {
    let (mut cache, peer, now) = fixture();
    let log = |term, index| {
        openraft::LogId::new(openraft::CommittedLeaderId::new(term, cache.own), index)
    };
    let old_log = log(1, 3);
    let new_log = log(1, 5);
    let conflict = log(2, 5);
    let old_route = BTreeMap::from([(
        peer,
        Node {
            endpoint: Some("127.0.0.1:10001".parse().unwrap()),
            ..Default::default()
        },
    )]);
    let new_route = BTreeMap::from([(
        peer,
        Node {
            endpoint: Some("127.0.0.1:10002".parse().unwrap()),
            ..Default::default()
        },
    )]);
    assert!(
        cache
            .install_routes(Some(old_log), old_route.clone())
            .unwrap()
    );
    let old = cache.reserve(peer, now).unwrap();
    assert!(
        cache
            .install_routes(Some(new_log), new_route.clone())
            .unwrap()
    );
    assert!(
        !cache
            .install_routes(Some(old_log), old_route.clone())
            .unwrap()
    );
    assert!(!cache.install_routes(None, old_route.clone()).unwrap());
    assert!(
        cache
            .install_routes(Some(conflict), new_route.clone())
            .is_err()
    );
    assert!(cache.install_routes(Some(new_log), old_route).is_err());
    cache.complete(old, response(4, Some("old route")), now);
    assert!(cache.names(now).is_empty());
    assert_eq!(cache.counters.discarded, 1);
    assert_eq!(cache.routes, new_route);
    // Lifecycle reset invalidates data but must retain the route high-water mark.
    cache.suspend();
    cache.register(4, &[cache.own, peer]).unwrap();
    assert!(
        !cache
            .install_routes(Some(old_log), BTreeMap::new())
            .unwrap()
    );
    assert!(cache.install_routes(Some(new_log), new_route).unwrap());
}
