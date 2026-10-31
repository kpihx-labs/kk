#![cfg(target_os = "linux")]
mod support;

use kk::{
    Error, Identity, config,
    contacts::Registry,
    pairing::{self, EnrollmentStage, PairServer, RoomJoinOptions},
    rooms::{
        Rights, RootBundle,
        control::raft::{
            Node, PeerId, network,
            service::{ControlNode, Mode, Settings},
        },
        enrollment::Authority,
        group::Group,
        mls::CryptoMember,
        runtime::{Actor, Room},
    },
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, time::Duration};

#[tokio::test]
async fn backend_enrollment_orchestrators_preserve_reader_rights_and_no_global_contacts() {
    use kk::rooms::enrollment::{self, CandidateOptions};
    let directory = support::private_root();
    let a = Arc::new(Identity::create(&directory.path().join("a")).unwrap());
    let b = Arc::new(Identity::create(&directory.path().join("b")).unwrap());
    let anchor = RootBundle::create(&a, "Backend enrollment flow")
        .unwrap()
        .verify(&a.peer())
        .unwrap();
    let actor =
        CryptoMember::create_private(&a, &anchor, vec![], &directory.path().join("private-a"))
            .unwrap();
    let origin = actor.public_identity().unwrap();
    let id = PeerId::from_peer(&a.peer());
    let control = Arc::new(
        ControlNode::start(
            a.clone(),
            &directory.path().join("control-a"),
            Settings {
                anchor: anchor.clone(),
                listen: "127.0.0.1:0".parse().unwrap(),
                peers: BTreeMap::from([(id, a.peer())]),
                timing: network::configuration().unwrap(),
            },
            Mode::Create(origin),
        )
        .await
        .unwrap(),
    );
    control
        .initialize(BTreeMap::from([(
            id,
            Node {
                endpoint: Some(control.local_addr().unwrap()),
                ..Default::default()
            },
        )]))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while control.current_leader().await != Some(id) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let owner = Room::new(
        a.clone(),
        control.clone(),
        Group::create(actor).unwrap(),
        Registry::open(directory.path().join("profile-a")).unwrap(),
    )
    .unwrap();
    let (mut owner, mut joined) = tokio::task::LocalSet::new()
        .run_until(async {
            let (handle, actor, _events) = Actor::new(owner);
            let running = tokio::task::spawn_local(actor.run());
            let mut completed = None;
            for canceled in [true, false] {
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
                    expected_fingerprint: Some(a.peer().fingerprint()),
                    code: server.code().unwrap().clone(),
                    data_port: 45461,
                    deadline: Duration::from_secs(30),
                };
                let (server, client) = tokio::join!(
                    server.run_room_session(&a, 45460, |_| async { Ok(true) }, |_| Ok(())),
                    pairing::join_room_session(options, &b, |_| async { Ok(true) }, |_| Ok(())),
                );
                let (admitted, joined) = tokio::join!(
                    enrollment::admit(
                        server.unwrap(),
                        &handle,
                        Rights::READER,
                        Duration::from_secs(15)
                    ),
                    async {
                        let prepared = enrollment::prepare(
                            client.unwrap(),
                            b.clone(),
                            CandidateOptions {
                                private_state: directory.path().join("private-b"),
                                control_state: directory.path().join("control-b"),
                                profile: directory.path().join("profile-b"),
                                listen: "127.0.0.1:0".parse().unwrap(),
                                rights: Rights::READER,
                                deadline: Duration::from_secs(15),
                            },
                        )
                        .await?;
                        assert_eq!(prepared.authority().anchor().id(), anchor.id());
                        assert!(!directory.path().join("private-b").exists());
                        assert!(!directory.path().join("control-b").exists());
                        if canceled {
                            prepared.join_until(std::future::ready(Ok(()))).await
                        } else {
                            prepared.join().await
                        }
                    },
                );
                if canceled {
                    assert!(admitted.is_err());
                    assert!(matches!(joined, Err(Error::Cancelled)));
                    assert!(!directory.path().join("private-b").exists());
                    assert!(!directory.path().join("control-b").exists());
                    continue;
                }
                admitted.unwrap();
                completed = Some(joined.unwrap());
            }
            let joined = completed.unwrap();
            handle.shutdown(Duration::from_secs(5)).await.unwrap();
            (running.await.unwrap(), joined)
        })
        .await;
    let reader = joined.room.capabilities();
    assert!(!reader.publish && !reader.invite && !reader.remove && !reader.control);
    assert!(reader.admission_roles.is_empty());
    let moderator = owner.capabilities();
    assert!(moderator.publish && moderator.invite && moderator.remove && moderator.control);
    assert_eq!(moderator.admission_roles.len(), 4);
    assert!(matches!(
        joined
            .room
            .encrypt_text("reader cannot publish", false, Duration::from_secs(5))
            .await,
        Err(Error::Denied)
    ));
    let wire = owner
        .encrypt_text("reader can receive", true, Duration::from_secs(5))
        .await
        .unwrap();
    let text = joined
        .room
        .decrypt_text(&a.peer(), &wire, Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(text.content.text(), "reader can receive");
    assert!(matches!(
        joined
            .room
            .authorize_file(&a.peer(), false, Duration::from_secs(5))
            .await,
        Err(Error::Denied)
    ));
    joined
        .room
        .authorize_file(&a.peer(), true, Duration::from_secs(5))
        .await
        .unwrap();
    let reader_policy = joined
        .room
        .media_policy(Duration::from_secs(5))
        .await
        .unwrap();
    assert!(reader_policy.inbound.contains(&id));
    assert!(reader_policy.outbound.is_empty());
    let owner_policy = owner.media_policy(Duration::from_secs(5)).await.unwrap();
    assert!(owner_policy.outbound.contains(&joined.control.id()));
    assert!(!owner_policy.inbound.contains(&joined.control.id()));
    assert_eq!(
        joined
            .control
            .control_membership()
            .membership()
            .voter_ids()
            .collect::<Vec<_>>(),
        vec![id]
    );
    assert!(
        Registry::open(directory.path().join("profile-b"))
            .unwrap()
            .list()
            .unwrap()
            .is_empty()
    );
    drop(owner);
    drop(joined.room);
    match Arc::try_unwrap(joined.control) {
        Ok(node) => node.shutdown().await.unwrap(),
        Err(_) => panic!("candidate owner leaked"),
    }
    match Arc::try_unwrap(control) {
        Ok(node) => node.shutdown().await.unwrap(),
        Err(_) => panic!("inviter owner leaked"),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Enrollment {
    package: Vec<u8>,
    endpoint: SocketAddr,
}

#[tokio::test]
async fn proven_session_delivers_authority_and_key_package_then_learner_joins_without_a_vote() {
    let directory = support::private_root();
    let a = Arc::new(Identity::create(&directory.path().join("a")).unwrap());
    let b = Arc::new(Identity::create(&directory.path().join("b")).unwrap());
    let a_id = PeerId::from_peer(&a.peer());
    let b_id = PeerId::from_peer(&b.peer());
    let root = RootBundle::create(&a, "Authenticated enrollment").unwrap();
    let anchor = root.verify(&a.peer()).unwrap();
    let actor =
        CryptoMember::create_private(&a, &anchor, vec![], &directory.path().join("private-a"))
            .unwrap();
    let creator = actor.public_identity().unwrap();
    let control = Arc::new(
        ControlNode::start(
            a.clone(),
            &directory.path().join("control-a"),
            Settings {
                anchor: anchor.clone(),
                listen: "127.0.0.1:0".parse().unwrap(),
                peers: BTreeMap::from([(a_id, a.peer())]),
                timing: network::configuration().unwrap(),
            },
            Mode::Create(creator.clone()),
        )
        .await
        .unwrap(),
    );
    control
        .initialize(BTreeMap::from([(
            a_id,
            Node {
                endpoint: Some(control.local_addr().unwrap()),
                ..Default::default()
            },
        )]))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while control.current_leader().await != Some(a_id) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let ra = Registry::open(directory.path().join("profile-a")).unwrap();
    let rb = Registry::open(directory.path().join("profile-b")).unwrap();
    let owner = Room::new(
        a.clone(),
        control.clone(),
        Group::create(actor).unwrap(),
        ra,
    )
    .unwrap();
    let (mut owner, mut participant, peer_control) = tokio::task::LocalSet::new()
        .run_until(async {
            let (owner, actor, _events) = Actor::new(owner);
            let running = tokio::task::spawn_local(actor.run());
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
                expected_fingerprint: Some(a.peer().fingerprint()),
                code: server.code().unwrap().clone(),
                data_port: 45461,
                deadline: Duration::from_secs(30),
            };
            let (accepted, joined) = tokio::join!(
                server.run_room_session(&a, 45460, |_| async { Ok(true) }, |_| Ok(())),
                pairing::join_room_session(options, &b, |_| async { Ok(true) }, |_| Ok(()))
            );
            let mut server = accepted.unwrap();
            let mut client = joined.unwrap();
            let scoped_peer = server.peer().clone();
            assert_eq!(scoped_peer.peer.fingerprint(), b.peer().fingerprint());
            let server_flow = async {
                server
                    .respond(
                        EnrollmentStage::Authority,
                        Duration::from_secs(10),
                        |_| async {
                            assert!(matches!(
                                owner
                                    .provision_scoped_learner(
                                        scoped_peer.clone(),
                                        Node {
                                            endpoint: Some("127.0.0.1:1".parse().unwrap()),
                                            ..Default::default()
                                        },
                                        Duration::from_secs(1)
                                    )
                                    .await,
                                Err(Error::Denied)
                            ));
                            owner
                                .issue_enrollment_authority(
                                    scoped_peer.clone(),
                                    Rights::PARTICIPANT,
                                    Duration::from_secs(5),
                                )
                                .await?
                                .encode()
                        },
                    )
                    .await
                    .unwrap();
                server
                    .respond(
                        EnrollmentStage::KeyPackage,
                        Duration::from_secs(10),
                        |bytes| {
                            let owner = &owner;
                            let control = &control;
                            let a = &a;
                            async move {
                                let incoming: Enrollment =
                                    serde_json::from_slice(&bytes).map_err(|_| Error::Protocol)?;
                                let mut wrong = scoped_peer.clone();
                                wrong.peer = a.peer();
                                assert!(matches!(
                                    owner
                                        .prepare_scoped_admission(
                                            wrong,
                                            incoming.package.clone(),
                                            Duration::from_secs(5)
                                        )
                                        .await,
                                    Err(Error::Authentication)
                                ));
                                owner
                                    .prepare_scoped_admission(
                                        scoped_peer.clone(),
                                        incoming.package,
                                        Duration::from_secs(5),
                                    )
                                    .await?;
                                let receipt = owner.submit_prepared(Duration::from_secs(5)).await?;
                                assert!(receipt.accepted);
                                owner
                                    .provision_scoped_learner(
                                        scoped_peer.clone(),
                                        Node {
                                            endpoint: Some(incoming.endpoint),
                                            ..Default::default()
                                        },
                                        Duration::from_secs(5),
                                    )
                                    .await?;
                                assert_eq!(
                                    control
                                        .control_membership()
                                        .membership()
                                        .voter_ids()
                                        .collect::<Vec<_>>(),
                                    vec![a_id]
                                );
                                Ok(b"committed and replicated; not a vote".to_vec())
                            }
                        },
                    )
                    .await
                    .unwrap();
                server.close().await.unwrap();
            };
            let client_flow = async {
                let bytes = client
                    .request(
                        EnrollmentStage::Authority,
                        b"request public authority",
                        Duration::from_secs(10),
                    )
                    .await
                    .unwrap();
                let authority = Authority::decode(&bytes).unwrap();
                let mut altered = serde_json::to_value(&authority).unwrap();
                altered["body"]["seeds"][0]["endpoint"] = "127.0.0.1:9".into();
                let altered = Authority::decode(&serde_json::to_vec(&altered).unwrap()).unwrap();
                assert!(
                    altered
                        .verify(
                            client.peer().room,
                            &client.peer().peer,
                            &b.peer(),
                            Rights::PARTICIPANT
                        )
                        .is_err()
                );
                assert!(
                    authority
                        .verify(
                            [42; 32],
                            &client.peer().peer,
                            &b.peer(),
                            Rights::PARTICIPANT
                        )
                        .is_err()
                );
                assert!(
                    authority
                        .verify(
                            client.peer().room,
                            &b.peer(),
                            &b.peer(),
                            Rights::PARTICIPANT
                        )
                        .is_err()
                );
                assert!(
                    authority
                        .verify(
                            client.peer().room,
                            &client.peer().peer,
                            &a.peer(),
                            Rights::PARTICIPANT
                        )
                        .is_err()
                );
                assert!(
                    authority
                        .verify(
                            client.peer().room,
                            &client.peer().peer,
                            &b.peer(),
                            Rights::MODERATOR
                        )
                        .is_err()
                );
                let approved = authority
                    .verify(
                        client.peer().room,
                        &client.peer().peer,
                        &b.peer(),
                        Rights::PARTICIPANT,
                    )
                    .unwrap();
                let mut candidate = approved
                    .create_candidate(&b, &directory.path().join("private-b"))
                    .unwrap();
                let package = candidate.key_package().unwrap();
                let peer = client.peer().peer.clone();
                let participant = Arc::new(
                    ControlNode::start(
                        b.clone(),
                        &directory.path().join("control-b"),
                        Settings {
                            anchor: approved.anchor().clone(),
                            listen: "127.0.0.1:0".parse().unwrap(),
                            peers: approved.control_peers(&b).unwrap(),
                            timing: network::configuration().unwrap(),
                        },
                        Mode::Create(approved.creator().clone()),
                    )
                    .await
                    .unwrap(),
                );
                // No manual root/grant/key-package files, and NO preinitialized vote for B.
                let payload = serde_json::to_vec(&Enrollment {
                    package,
                    endpoint: participant.local_addr().unwrap(),
                })
                .unwrap();
                assert!(payload.len() < config::ROOM_MAX_WIRE_BYTES);
                let answer = client
                    .request(
                        EnrollmentStage::KeyPackage,
                        &payload,
                        Duration::from_secs(10),
                    )
                    .await
                    .unwrap();
                assert_eq!(answer.as_slice(), b"committed and replicated; not a vote");
                // Replication acknowledgement is not the recipient's cached
                // metrics publication. Wait for its own authoritative apply.
                participant
                    .fresh_catch_up(None, Duration::from_secs(5))
                    .await
                    .unwrap();
                assert_eq!(
                    participant
                        .control_membership()
                        .membership()
                        .voter_ids()
                        .collect::<Vec<_>>(),
                    vec![a_id]
                );
                assert!(
                    participant
                        .control_membership()
                        .membership()
                        .get_node(&b_id)
                        .is_some()
                );
                let joined = Room::join(
                    b.clone(),
                    participant.clone(),
                    candidate,
                    &peer,
                    rb,
                    Duration::from_secs(5),
                )
                .await
                .unwrap();
                client.close().await.unwrap();
                (joined, participant)
            };
            let ((), (participant, peer_control)) = tokio::join!(server_flow, client_flow);
            owner.shutdown(Duration::from_secs(5)).await.unwrap();
            (running.await.unwrap(), participant, peer_control)
        })
        .await;
    assert_eq!(participant.epoch(), owner.epoch());
    let wire = participant
        .encrypt_text(
            "new learner owns its own private group keys",
            true,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    let text = owner
        .decrypt_text(&b.peer(), &wire, Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(
        text.content.text(),
        "new learner owns its own private group keys"
    );
    assert!(
        Registry::open(directory.path().join("profile-a"))
            .unwrap()
            .list()
            .unwrap()
            .is_empty()
    );
    assert!(
        Registry::open(directory.path().join("profile-b"))
            .unwrap()
            .list()
            .unwrap()
            .is_empty()
    );
    drop(participant);
    drop(owner);
    match Arc::try_unwrap(peer_control) {
        Ok(node) => node.shutdown().await.unwrap(),
        Err(_) => panic!("learner owner leaked"),
    }
    match Arc::try_unwrap(control) {
        Ok(node) => node.shutdown().await.unwrap(),
        Err(_) => panic!("creator owner leaked"),
    }
}
