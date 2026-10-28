use super::*;
use crate::{
    contacts::{Contact, Permissions},
    rooms::{
        Grant, Rights, RootBundle,
        control::raft::{
            Node, PeerId, network,
            service::{Mode, Settings},
        },
        mls::CryptoMember,
    },
};
use std::collections::BTreeMap;

#[tokio::test]
async fn three_private_rooms_use_quorum_order_durable_nonce_and_local_veto_without_global_trust() {
    let directory = tempfile::tempdir().unwrap();
    let identities: Vec<_> = (0..3)
        .map(|index| {
            Arc::new(Identity::create(&directory.path().join(format!("identity-{index}"))).unwrap())
        })
        .collect();
    let anchor = RootBundle::create(&identities[0], "Actual private runtime fixture")
        .unwrap()
        .verify(&identities[0].peer())
        .unwrap();
    let actor = CryptoMember::create_private(
        &identities[0],
        &anchor,
        vec![],
        &directory.path().join("private-0"),
    )
    .unwrap();
    let creator = actor.public_identity().unwrap();
    let group = Group::create(actor).unwrap();
    let peers: BTreeMap<_, _> = identities
        .iter()
        .map(|identity| (PeerId::from_peer(&identity.peer()), identity.peer()))
        .collect();
    let mut controls = Vec::new();
    let mut voters = BTreeMap::new();
    for (index, identity) in identities.iter().enumerate() {
        let mut timing = network::configuration().unwrap();
        timing.heartbeat_interval = 50;
        timing.election_timeout_min = 200;
        timing.election_timeout_max = 400;
        let control = ControlNode::start(
            identity.clone(),
            &directory.path().join(format!("control-{index}")),
            Settings {
                anchor: anchor.clone(),
                listen: "127.0.0.1:0".parse().unwrap(),
                peers: peers.clone(),
                timing,
            },
            Mode::Create(creator.clone()),
        )
        .await
        .unwrap();
        voters.insert(
            control.id(),
            Node {
                endpoint: Some(control.local_addr().unwrap()),
                ..Default::default()
            },
        );
        controls.push(Arc::new(control));
    }
    controls[0].initialize(voters).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while controls[0].current_leader().await.is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let profile_path = directory.path().join("profile-0");
    let mut rooms = vec![
        Room::new(
            identities[0].clone(),
            controls[0].clone(),
            group,
            Registry::open(profile_path.clone()).unwrap(),
        )
        .unwrap(),
    ];
    for index in 1..3 {
        let grant = Grant::issue(
            &anchor,
            &[],
            &identities[0],
            &identities[index].peer(),
            if index == 1 {
                Rights::READER
            } else {
                Rights::PARTICIPANT
            },
        )
        .unwrap();
        let mut candidate = CryptoMember::create_private(
            &identities[index],
            &anchor,
            vec![grant],
            &directory.path().join(format!("private-{index}")),
        )
        .unwrap();
        let package = candidate.key_package().unwrap();
        let proposal = rooms[0]
            .prepare_admission(&package, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(
            rooms[0].prepared_control().unwrap().unwrap().id().unwrap(),
            proposal.id().unwrap()
        );
        let receipt = rooms[0]
            .submit_prepared(Duration::from_secs(5))
            .await
            .unwrap();
        assert!(receipt.accepted);
        let welcome = rooms[0]
            .committed_welcome(proposal.commit_bytes())
            .unwrap()
            .unwrap();
        let (replica, records) = controls[index]
            .fresh_catch_up(None, Duration::from_secs(5))
            .await
            .unwrap();
        let accepted = records
            .iter()
            .find(|record| Some(record.position().unwrap().log) == receipt.first_log)
            .unwrap();
        let replicated = replica
            .welcome_for(&identities[index].peer().fingerprint_bytes())
            .unwrap();
        assert_eq!(replicated.wire, welcome);
        assert_eq!(replicated.proposal, proposal.id().unwrap());
        assert_eq!(accepted.proposal_id(), Some(replicated.proposal));
        let leader = controls[index].current_leader().await.unwrap();
        let endpoint = controls
            .iter()
            .find(|control| control.id() == leader)
            .unwrap()
            .local_addr()
            .unwrap();
        let (wire_client, _unused_listener) = network::Factory::bind(
            anchor.id(),
            identities[index].clone(),
            peers.clone(),
            "127.0.0.1:0".parse().unwrap(),
        )
        .unwrap();
        let remote = wire_client
            .welcome_for_self(
                leader,
                &Node {
                    endpoint: Some(endpoint),
                    ..Default::default()
                },
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        let network::Reply::Applied(Some(delivered)) = remote else {
            panic!("recipient-bound remote Welcome did not commit");
        };
        assert_eq!(
            delivered.artifact.recipient,
            identities[index].peer().fingerprint_bytes()
        );
        assert_eq!(delivered.artifact.wire, replicated.wire);
        assert!(
            controls[0]
                .receive_welcome(Duration::from_secs(5))
                .await
                .unwrap()
                .is_none()
        ); // Cannot select Bob/Charlie.
        rooms.push(
            Room::join(
                identities[index].clone(),
                controls[index].clone(),
                candidate,
                &identities[0].peer(),
                Registry::open(directory.path().join(format!("profile-{index}"))).unwrap(),
                Duration::from_secs(5),
            )
            .await
            .unwrap(),
        );
        rooms[0]
            .acknowledge_welcome(proposal.commit_bytes())
            .unwrap();
    }
    for room in &mut rooms {
        room.synchronize(Duration::from_secs(5)).await.unwrap();
        assert_eq!(room.peers().len(), 3);
        assert_eq!(room.epoch(), 2);
    }
    // Owner, reader and participant may all exchange metadata. This does not
    // confer the publisher right needed for files or text.
    for (local, remote) in [(0, 1), (0, 2), (1, 0), (1, 2), (2, 0), (2, 1)] {
        assert_eq!(
            rooms[local]
                .authorize_profile_metadata(&identities[remote].peer(), Duration::from_secs(5))
                .await
                .unwrap(),
            2
        );
    }
    assert!(matches!(
        rooms[0]
            .authorize_file(&identities[1].peer(), true, Duration::from_secs(5))
            .await,
        Err(Error::Denied)
    ));
    assert!(matches!(
        rooms[1]
            .encrypt_text("reader cannot publish", false, Duration::from_secs(5))
            .await,
        Err(Error::Denied)
    ));
    let outsider = Identity::create(&directory.path().join("outsider")).unwrap();
    assert!(matches!(
        rooms[0]
            .authorize_profile_metadata(&outsider.peer(), Duration::from_secs(5))
            .await,
        Err(Error::Denied)
    ));
    assert!(matches!(
        rooms[0]
            .authorize_profile_metadata(&identities[0].peer(), Duration::from_secs(5))
            .await,
        Err(Error::Denied)
    ));
    let ciphertext = rooms[0]
        .encrypt_text("three member private payload", true, Duration::from_secs(5))
        .await
        .unwrap();
    assert!(
        rooms[2]
            .decrypt_text(&identities[1].peer(), &ciphertext, Duration::from_secs(5))
            .await
            .is_err()
    );
    for room in rooms.iter_mut().skip(1) {
        let text = room
            .decrypt_text(&identities[0].peer(), &ciphertext, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(text.content.text(), "three member private payload");
        assert!(
            room.decrypt_text(&identities[0].peer(), &ciphertext, Duration::from_secs(5))
                .await
                .is_err()
        );
    }
    for index in 0..3 {
        assert!(
            Registry::open(directory.path().join(format!("profile-{index}")))
                .unwrap()
                .list()
                .unwrap()
                .is_empty()
        );
    }
    let permissions = Permissions {
        text: true,
        files: false,
        rooms: true,
        media: false,
    };
    let policy = Registry::open(profile_path).unwrap();
    for (incoming, outgoing) in [(false, true), (true, false)] {
        policy
            .put(
                "bob",
                Contact {
                    certificate: identities[1].peer().certificate.as_ref().to_vec(),
                    local_identity: identities[0].peer().fingerprint(),
                    incoming: Permissions {
                        rooms: incoming,
                        ..permissions
                    },
                    outgoing: Permissions {
                        rooms: outgoing,
                        ..permissions
                    },
                    endpoints: vec![],
                },
            )
            .unwrap();
        assert!(matches!(
            rooms[0]
                .authorize_profile_metadata(&identities[1].peer(), Duration::from_secs(5))
                .await,
            Err(Error::Denied)
        ));
    }
    policy
        .put(
            "bob",
            Contact {
                certificate: identities[1].peer().certificate.as_ref().to_vec(),
                local_identity: identities[0].peer().fingerprint(),
                incoming: permissions,
                outgoing: permissions,
                endpoints: vec![],
            },
        )
        .unwrap();
    // The Rooms permission alone suffices even when Files and Media are denied.
    assert_eq!(
        rooms[0]
            .authorize_profile_metadata(&identities[1].peer(), Duration::from_secs(5))
            .await
            .unwrap(),
        2
    );
    policy.revoke("bob").unwrap();
    assert!(matches!(
        rooms[0]
            .authorize_profile_metadata(&identities[1].peer(), Duration::from_secs(5))
            .await,
        Err(Error::Revoked)
    ));
    // Sending only to Charlie would still expose the old epoch to revoked Bob.
    assert!(matches!(
        rooms[0]
            .encrypt_text("must not escape", false, Duration::from_secs(5))
            .await,
        Err(Error::Revoked)
    ));
    rooms[0]
        .prepare_departure(&identities[1].peer(), Duration::from_secs(5))
        .await
        .unwrap();
    assert!(
        rooms[0]
            .submit_prepared(Duration::from_secs(5))
            .await
            .unwrap()
            .accepted
    );
    assert!(matches!(
        rooms[0]
            .authorize_profile_metadata(&identities[1].peer(), Duration::from_secs(5))
            .await,
        Err(Error::Denied)
    ));
    assert!(matches!(
        rooms[1]
            .authorize_profile_metadata(&identities[0].peer(), Duration::from_secs(5))
            .await,
        Err(Error::Revoked)
    ));
    let next = rooms[0]
        .encrypt_text(
            "new epoch excludes revoked Bob",
            false,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert!(
        rooms[2]
            .decrypt_text(&identities[0].peer(), &next, Duration::from_secs(5))
            .await
            .is_ok()
    );
    assert!(
        rooms[1]
            .decrypt_text(&identities[0].peer(), &next, Duration::from_secs(5))
            .await
            .is_err()
    );
    let alice = rooms.remove(0);
    let (handle, actor, mut events) = Actor::new(alice);
    fn assert_send<T: Send>() {}
    assert_send::<Handle>();
    // No actor is running yet: excess commands fail instead of allocating an
    // unbounded backlog. Expired requests must not execute after startup.
    let queued = futures_util::future::join_all(
        (0..=config::ROOM_COMMAND_QUEUE).map(|_| handle.synchronize(Duration::from_millis(10))),
    )
    .await;
    assert!(
        queued
            .iter()
            .any(|result| matches!(result, Err(Error::Limit { .. })))
    );
    assert!(
        queued
            .iter()
            .any(|result| matches!(result, Err(Error::Timeout)))
    );
    let returned = tokio::task::LocalSet::new()
        .run_until(async {
            let running = tokio::task::spawn_local(actor.run());
            tokio::task::yield_now().await;
            assert_eq!(
                handle.status(Duration::from_secs(5)).await.unwrap().epoch,
                3
            );
            assert_eq!(
                handle
                    .authorize_profile_metadata(identities[2].peer(), Duration::from_secs(5))
                    .await
                    .unwrap(),
                3
            );
            assert!(matches!(
                handle
                    .authorize_profile_metadata(identities[1].peer(), Duration::from_secs(5))
                    .await,
                Err(Error::Denied)
            ));
            controls[0].register_room(handle.clone()).await.unwrap();
            assert!(controls[1].register_room(handle.clone()).await.is_err());
            let target = PeerId::from_peer(&identities[0].peer());
            let original = rooms[1]
                .encrypt_text("original peer binding", false, Duration::from_secs(5))
                .await
                .unwrap();
            assert!(matches!(
                controls[1]
                    .send_room_text(target, &original, Duration::from_secs(5))
                    .await,
                Err(Error::Denied)
            ));
            controls[2]
                .send_room_text(target, &original, Duration::from_secs(5))
                .await
                .unwrap();
            let Event::ReferencedText { text: original, .. } = events.recv().await.unwrap() else {
                panic!("missing authenticated reference")
            };
            assert_eq!(original.content.text(), "original peer binding");
            // The remaining direct sender is Charlie (Bob has been removed).
            for _ in 0..config::ROOM_EVENT_QUEUE {
                let wire = rooms[1]
                    .encrypt_text("bounded actor event", false, Duration::from_secs(5))
                    .await
                    .unwrap();
                controls[2]
                    .send_room_text(target, &wire, Duration::from_secs(5))
                    .await
                    .unwrap();
            }
            let wire = rooms[1]
                .encrypt_text(
                    "must survive queue-full retry",
                    true,
                    Duration::from_secs(5),
                )
                .await
                .unwrap();
            assert!(matches!(
                controls[2]
                    .send_room_text(target, &wire, Duration::from_secs(5))
                    .await,
                Err(Error::Node(_))
            ));
            let Event::ReferencedText { text: first, .. } = events.recv().await.unwrap() else {
                panic!("missing authenticated reference")
            };
            assert_eq!(first.content.text(), "bounded actor event");
            controls[2]
                .send_room_text(target, &wire, Duration::from_secs(5))
                .await
                .unwrap();
            for _ in 1..config::ROOM_EVENT_QUEUE {
                events.recv().await.unwrap();
            }
            let Event::ReferencedText { text: last, .. } = events.recv().await.unwrap() else {
                panic!("missing authenticated reference")
            };
            assert_eq!(last.content.text(), "must survive queue-full retry");
            controls[0].unregister_room().unwrap();
            handle.shutdown(Duration::from_secs(5)).await.unwrap();
            running.await.unwrap()
        })
        .await;
    rooms.push(returned);
    drop(rooms);
    for control in controls {
        match Arc::try_unwrap(control) {
            Ok(control) => control.shutdown().await.unwrap(),
            Err(_) => panic!("runtime leaked a public-control owner"),
        }
    }
}
