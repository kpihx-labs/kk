#![cfg(target_os = "linux")]
//! Explicitly preapproved initial controller pair. The third device is NOT a
//! preapproved peer/voter; its admission and TLS pin arrive through the protocol.
mod support;
use kk::{
    Error, Identity,
    contacts::Registry,
    pairing::{self, PairServer, RoomJoinOptions},
    rooms::{
        Grant, Rights, RootBundle,
        control::raft::{
            Node, PeerId, network,
            service::{ControlNode, Mode, Settings},
        },
        enrollment::{self, CandidateOptions},
        group::Group,
        mls::CryptoMember,
        runtime::{Actor, Event, Room},
    },
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

#[tokio::test]
async fn follower_inviter_forwards_signed_learner_intent_and_cold_scope_recovers_from_metadata() {
    let directory = support::private_root();
    let a = Arc::new(Identity::create(&directory.path().join("a")).unwrap());
    let b = Arc::new(Identity::create(&directory.path().join("b")).unwrap());
    let c = Arc::new(Identity::create(&directory.path().join("c")).unwrap());
    let a_id = PeerId::from_peer(&a.peer());
    let b_id = PeerId::from_peer(&b.peer());
    let c_id = PeerId::from_peer(&c.peer());
    let anchor = RootBundle::create(&a, "Forwarded enrollment")
        .unwrap()
        .verify(&a.peer())
        .unwrap();
    let actor =
        CryptoMember::create_private(&a, &anchor, vec![], &directory.path().join("private-a"))
            .unwrap();
    let origin = actor.public_identity().unwrap();
    let initial = BTreeMap::from([(a_id, a.peer()), (b_id, b.peer())]);
    let settings = |listen| Settings {
        anchor: anchor.clone(),
        listen,
        peers: initial.clone(),
        timing: network::configuration().unwrap(),
    };
    let control_a = Arc::new(
        ControlNode::start(
            a.clone(),
            &directory.path().join("control-a"),
            settings("127.0.0.1:0".parse().unwrap()),
            Mode::Create(origin.clone()),
        )
        .await
        .unwrap(),
    );
    let control_b = Arc::new(
        ControlNode::start(
            b.clone(),
            &directory.path().join("control-b"),
            settings("127.0.0.1:0".parse().unwrap()),
            Mode::Create(origin.clone()),
        )
        .await
        .unwrap(),
    );
    let b_address = control_b.local_addr().unwrap();
    control_a
        .initialize(BTreeMap::from([
            (
                a_id,
                Node::for_peer(&a.peer(), Some(control_a.local_addr().unwrap())),
            ),
            (b_id, Node::for_peer(&b.peer(), Some(b_address))),
        ]))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while control_b.current_leader().await != Some(a_id) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut owner = Room::new(
        a.clone(),
        control_a.clone(),
        Group::create(actor).unwrap(),
        Registry::open(directory.path().join("profile-a")).unwrap(),
    )
    .unwrap();
    let grant = Grant::issue(&anchor, &[], &a, &b.peer(), Rights::MODERATOR).unwrap();
    let mut candidate = CryptoMember::create_private(
        &b,
        &anchor,
        vec![grant],
        &directory.path().join("private-b"),
    )
    .unwrap();
    let package = candidate.key_package().unwrap();
    owner
        .prepare_admission(&package, Duration::from_secs(5))
        .await
        .unwrap();
    assert!(
        owner
            .submit_prepared(Duration::from_secs(5))
            .await
            .unwrap()
            .accepted
    );
    let moderator = Room::join(
        b.clone(),
        control_b.clone(),
        candidate,
        &a.peer(),
        Registry::open(directory.path().join("profile-b")).unwrap(),
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            let (a_handle, a_actor, _a_events) = Actor::new(owner);
            let a_task = tokio::task::spawn_local(a_actor.run());
            let (b_handle, b_actor, mut b_events) = Actor::new(moderator);
            let b_task = tokio::task::spawn_local(b_actor.run());
            control_b.register_room(b_handle.clone()).await.unwrap();
            assert_eq!(control_b.current_leader().await, Some(a_id));
            let server = PairServer::bind_room(
                "127.0.0.1:0".parse().unwrap(),
                Duration::from_secs(30),
                anchor.id(),
            )
            .await
            .unwrap();
            let options = RoomJoinOptions {
                room: anchor.id(),
                address: server.local_addr().unwrap(),
                expected_id: Some(server.id()),
                expected_fingerprint: Some(b.peer().fingerprint()),
                code: server.code().unwrap().clone(),
                data_port: 45461,
                deadline: Duration::from_secs(30),
            };
            let (server, client) = tokio::join!(
                server.run_room_session(&b, 45460, |_| async { Ok(true) }, |_| Ok(())),
                pairing::join_room_session(options, &c, |_| async { Ok(true) }, |_| Ok(()))
            );
            let (admitted, joined) = tokio::join!(
                enrollment::admit(
                    server.unwrap(),
                    &b_handle,
                    Rights::PARTICIPANT,
                    Duration::from_secs(15)
                ),
                enrollment::join(
                    client.unwrap(),
                    c.clone(),
                    CandidateOptions {
                        private_state: directory.path().join("private-c"),
                        control_state: directory.path().join("control-c"),
                        profile: directory.path().join("profile-c"),
                        listen: "127.0.0.1:0".parse().unwrap(),
                        rights: Rights::PARTICIPANT,
                        deadline: Duration::from_secs(15)
                    }
                ),
            );
            admitted.unwrap();
            let mut joined = joined.unwrap();
            assert_eq!(
                joined
                    .control
                    .control_membership()
                    .membership()
                    .voter_ids()
                    .collect::<Vec<_>>(),
                vec![a_id, b_id]
                    .into_iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
            );
            assert!(
                control_a
                    .control_membership()
                    .membership()
                    .get_node(&c_id)
                    .unwrap()
                    .peer(c_id)
                    .is_ok()
            );
            assert!(matches!(
                joined
                    .control
                    .provision_learner(
                        a_id,
                        Node::for_peer(&a.peer(), Some(control_a.local_addr().unwrap())),
                        Duration::from_secs(2)
                    )
                    .await,
                Err(Error::Denied)
            ));
            let wire = joined
                .room
                .encrypt_text(
                    "new peer reaches the nonleader inviter",
                    true,
                    Duration::from_secs(5),
                )
                .await
                .unwrap();
            joined
                .control
                .send_room_text(b_id, &wire, Duration::from_secs(5))
                .await
                .unwrap();
            let Event::ReferencedText { text, .. } =
                tokio::time::timeout(Duration::from_secs(2), b_events.recv())
                    .await
                    .unwrap()
                    .unwrap()
            else {
                panic!("missing authenticated reference")
            };
            assert_eq!(
                text.content.text(),
                "new peer reaches the nonleader inviter"
            );
            let old_cipher = wire;
            b_handle
                .prepare_departure(c.peer(), Duration::from_secs(5))
                .await
                .unwrap();
            assert!(
                b_handle
                    .submit_prepared(Duration::from_secs(5))
                    .await
                    .unwrap()
                    .accepted
            );
            // Reconstruct B with ORIGINAL a/b settings, not a host-supplied C pin.
            // The committed controller node descriptor and room view own recovery.
            control_b.unregister_room().unwrap();
            b_handle.shutdown(Duration::from_secs(5)).await.unwrap();
            drop(b_task.await.unwrap());
            match Arc::try_unwrap(control_b) {
                Ok(node) => node.shutdown().await.unwrap(),
                Err(_) => panic!("moderator owner leaked"),
            }
            let resumed = Arc::new(
                ControlNode::start(
                    b.clone(),
                    &directory.path().join("control-b"),
                    settings(b_address),
                    Mode::Resume,
                )
                .await
                .unwrap(),
            );
            assert!(
                resumed
                    .approved_control_peers()
                    .unwrap()
                    .contains_key(&c_id)
            );
            assert!(
                resumed
                    .public_view()
                    .await
                    .unwrap()
                    .member_rights(&c_id.0)
                    .is_none()
            );
            let actor =
                CryptoMember::open_private(&b, &anchor, &directory.path().join("private-b"))
                    .unwrap();
            let room = Room::new(
                b.clone(),
                resumed.clone(),
                Group::restore(actor).unwrap(),
                Registry::open(directory.path().join("profile-b")).unwrap(),
            )
            .unwrap();
            let (handle, actor, mut events) = Actor::new(room);
            let task = tokio::task::spawn_local(actor.run());
            resumed.register_room(handle.clone()).await.unwrap();
            // C remains an explicitly configured metadata learner, not a DATA
            // member. Its TLS pin must recover, but old group data is denied.
            assert!(
                joined
                    .control
                    .receive_welcome(Duration::from_secs(5))
                    .await
                    .unwrap()
                    .is_none()
            );
            assert!(matches!(
                joined
                    .control
                    .send_room_text(b_id, &old_cipher, Duration::from_secs(5))
                    .await,
                Err(Error::Denied)
            ));
            let wire = a_handle
                .encrypt_text(
                    "cold scoped pin is not a global contact".to_owned(),
                    false,
                    Duration::from_secs(5),
                )
                .await
                .unwrap();
            control_a
                .send_room_text(b_id, &wire, Duration::from_secs(5))
                .await
                .unwrap();
            let Event::ReferencedText { text, .. } = events.recv().await.unwrap() else {
                panic!("missing authenticated reference")
            };
            assert_eq!(
                text.content.text(),
                "cold scoped pin is not a global contact"
            );
            assert!(
                Registry::open(directory.path().join("profile-b"))
                    .unwrap()
                    .list()
                    .unwrap()
                    .is_empty()
            );
            resumed.unregister_room().unwrap();
            handle.shutdown(Duration::from_secs(5)).await.unwrap();
            drop(task.await.unwrap());
            match Arc::try_unwrap(resumed) {
                Ok(node) => node.shutdown().await.unwrap(),
                Err(_) => panic!("resumed owner leaked"),
            }
            drop(joined.room);
            match Arc::try_unwrap(joined.control) {
                Ok(node) => node.shutdown().await.unwrap(),
                Err(_) => panic!("candidate owner leaked"),
            }
            a_handle.shutdown(Duration::from_secs(5)).await.unwrap();
            drop(a_task.await.unwrap());
            match Arc::try_unwrap(control_a) {
                Ok(node) => node.shutdown().await.unwrap(),
                Err(_) => panic!("creator owner leaked"),
            }
        })
        .await;
}
