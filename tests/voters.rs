#![cfg(target_os = "linux")]
//! Real QUIC/MLS fixture. Initial trust approves only the creator controller;
//! data admission adds learners, and ONLY an explicit signed operation adds votes.
use kk::{
    Error, Identity,
    contacts::Registry,
    rooms::{
        Grant, Rights, RootBundle,
        control::raft::{
            Node, PeerId, network,
            service::{ControlNode, Mode, Settings},
        },
        group::Group,
        mls::CryptoMember,
        runtime::{Actor, Room},
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

#[tokio::test]
async fn explicit_controller_approval_survives_creator_loss_and_keeps_application_roles() {
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
    let anchor = RootBundle::create(&identities[0], "Explicit controller lifecycle")
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
    let origin = actor.public_identity().unwrap();
    let root = Arc::new(
        ControlNode::start(
            identities[0].clone(),
            &directory.path().join("control-0"),
            Settings {
                anchor: anchor.clone(),
                listen: "127.0.0.1:0".parse().unwrap(),
                peers: BTreeMap::from([(ids[0], identities[0].peer())]),
                timing: network::configuration().unwrap(),
            },
            Mode::Create(origin.clone()),
        )
        .await
        .unwrap(),
    );
    root.initialize(BTreeMap::from([(
        ids[0],
        Node::for_peer(&identities[0].peer(), Some(root.local_addr().unwrap())),
    )]))
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while root.current_leader().await != Some(ids[0]) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut owner = Room::new(
        identities[0].clone(),
        root.clone(),
        Group::create(actor).unwrap(),
        Registry::open(directory.path().join("profile-0")).unwrap(),
    )
    .unwrap();
    let mut controls = vec![root];
    let mut rooms = Vec::new();
    for index in 1..3 {
        let role = if index == 1 {
            Rights::MODERATOR
        } else {
            Rights::PARTICIPANT
        };
        let grant = Grant::issue(
            &anchor,
            &[],
            &identities[0],
            &identities[index].peer(),
            role,
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
        let peer = Arc::new(
            ControlNode::start(
                identities[index].clone(),
                &directory.path().join(format!("control-{index}")),
                Settings {
                    anchor: anchor.clone(),
                    listen: "127.0.0.1:0".parse().unwrap(),
                    peers: controls[0]
                        .approved_control_peers()
                        .unwrap()
                        .into_iter()
                        .chain([(ids[index], identities[index].peer())])
                        .collect(),
                    timing: network::configuration().unwrap(),
                },
                Mode::Create(origin.clone()),
            )
            .await
            .unwrap(),
        );
        controls[0]
            .provision_learner(
                ids[index],
                Node::for_peer(&identities[index].peer(), Some(peer.local_addr().unwrap())),
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        let room = Room::join(
            identities[index].clone(),
            peer.clone(),
            candidate,
            &identities[0].peer(),
            Registry::open(directory.path().join(format!("profile-{index}"))).unwrap(),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        rooms.push(room);
        controls.push(peer);
    }
    for control in &controls {
        assert_eq!(
            control
                .control_membership()
                .membership()
                .voter_ids()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([ids[0]])
        );
    }
    let participant = rooms.pop().unwrap();
    let moderator = rooms.pop().unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            let (a_handle, a_actor, _) = Actor::new(owner);
            let a_task = tokio::task::spawn_local(a_actor.run());
            let (b_handle, b_actor, _) = Actor::new(moderator);
            let b_task = tokio::task::spawn_local(b_actor.run());
            let (c_handle, c_actor, _) = Actor::new(participant);
            let c_task = tokio::task::spawn_local(c_actor.run());
            let all = ids.iter().copied().collect::<BTreeSet<_>>();
            assert!(matches!(
                c_handle
                    .approve_controllers(all.clone(), Duration::from_secs(2))
                    .await,
                Err(Error::Denied)
            ));
            assert!(
                b_handle
                    .approve_controllers(BTreeSet::new(), Duration::from_secs(2))
                    .await
                    .is_err()
            );
            assert_eq!(controls[1].current_leader().await, Some(ids[0]));
            b_handle
                .approve_controllers(all.clone(), Duration::from_secs(10))
                .await
                .unwrap();
            for control in &controls {
                control
                    .fresh_catch_up(None, Duration::from_secs(5))
                    .await
                    .unwrap();
                assert_eq!(
                    control
                        .control_membership()
                        .membership()
                        .voter_ids()
                        .collect::<BTreeSet<_>>(),
                    all
                );
            }
            // A valid transport/voting role still cannot promote a participant's
            // administrative application authority.
            assert!(matches!(
                c_handle
                    .approve_controllers(all, Duration::from_secs(2))
                    .await,
                Err(Error::Denied)
            ));
            a_handle.shutdown(Duration::from_secs(5)).await.unwrap();
            drop(a_task.await.unwrap());
            let creator = controls.remove(0);
            match Arc::try_unwrap(creator) {
                Ok(node) => node.shutdown().await.unwrap(),
                Err(_) => panic!("creator control owner leaked"),
            }
            tokio::time::timeout(Duration::from_secs(8), async {
                loop {
                    if controls[0]
                        .current_leader()
                        .await
                        .is_some_and(|leader| leader != ids[0])
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            })
            .await
            .unwrap();
            // B and C supply the surviving majority of the original three. An
            // explicit joint->uniform transition can now retire A's vote.
            let remaining = BTreeSet::from([ids[1], ids[2]]);
            b_handle
                .approve_controllers(remaining.clone(), Duration::from_secs(10))
                .await
                .unwrap();
            for control in &controls {
                control
                    .fresh_catch_up(None, Duration::from_secs(5))
                    .await
                    .unwrap();
                let membership = control.control_membership();
                assert_eq!(membership.membership().get_joint_config().len(), 1);
                assert_eq!(
                    membership.membership().voter_ids().collect::<BTreeSet<_>>(),
                    remaining
                );
                assert!(membership.membership().get_node(&ids[0]).is_some()); // Explicit retain-as-learner policy.
            }
            let wire = b_handle
                .encrypt_text(
                    "surviving quorum without creator".to_owned(),
                    true,
                    Duration::from_secs(5),
                )
                .await
                .unwrap();
            // Use the returned owning room after orderly actor stop for native MLS
            // receive validation; controller roles did not change its data grant.
            c_handle.shutdown(Duration::from_secs(5)).await.unwrap();
            let mut receiver = c_task.await.unwrap();
            assert!(
                receiver
                    .decrypt_text(&identities[1].peer(), &wire, Duration::from_secs(5))
                    .await
                    .is_ok()
            );
            drop(receiver);
            b_handle.shutdown(Duration::from_secs(5)).await.unwrap();
            drop(b_task.await.unwrap());
            for control in controls {
                match Arc::try_unwrap(control) {
                    Ok(node) => node.shutdown().await.unwrap(),
                    Err(_) => panic!("remaining control owner leaked"),
                }
            }
        })
        .await;
}
