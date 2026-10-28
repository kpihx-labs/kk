#![cfg(unix)]
//! Kill the actual recipient CLI after replicated admission but BEFORE its stage
//! reply/private activation. Recovery must use its original keys and saved scope.
mod support;
use kk::{
    Identity,
    contacts::Registry,
    pairing::{EnrollmentStage, Invite, PairServer},
    rooms::{
        Rights, RootBundle,
        control::raft::{
            Node, PeerId, network,
            service::{ControlNode, Mode, Settings},
        },
        group::Group,
        mls::CryptoMember,
        runtime::{Actor, Event, Room},
    },
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

const WAIT: Duration = Duration::from_secs(30);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Package {
    package: Vec<u8>,
    endpoint: std::net::SocketAddr,
}

async fn command(profile: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(profile)
        .args(args)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn killed_cli_recovers_committed_enrollment_without_recreating_identity_or_mls_keys() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let root = support::private_root();
            let profile_b = root.path().join("recipient");
            command(&profile_b, &["init"]).await;
            let b = kk::profile::identity(&profile_b, false).unwrap();
            let identity =
                Arc::new(Identity::create(&root.path().join("issuer-identity")).unwrap());
            let anchor = RootBundle::create(&identity, "Actual interrupted CLI enrollment")
                .unwrap()
                .verify(&identity.peer())
                .unwrap();
            let member = CryptoMember::create_private(
                &identity,
                &anchor,
                vec![],
                &root.path().join("issuer-private"),
            )
            .unwrap();
            let own = PeerId::from_peer(&identity.peer());
            let control = Arc::new(
                ControlNode::start(
                    identity.clone(),
                    &root.path().join("issuer-control"),
                    Settings {
                        anchor: anchor.clone(),
                        listen: "127.0.0.1:0".parse().unwrap(),
                        peers: BTreeMap::from([(own, identity.peer())]),
                        timing: network::configuration().unwrap(),
                    },
                    Mode::Create(member.public_identity().unwrap()),
                )
                .await
                .unwrap(),
            );
            control
                .initialize(BTreeMap::from([(
                    own,
                    Node::for_peer(&identity.peer(), Some(control.local_addr().unwrap())),
                )]))
                .await
                .unwrap();
            let room = Room::new(
                identity.clone(),
                control.clone(),
                Group::create(member).unwrap(),
                Registry::open(root.path().join("issuer-profile")).unwrap(),
            )
            .unwrap();
            let (handle, actor, mut events) = Actor::new(room);
            let running = tokio::task::spawn_local(actor.run());
            control.register_room(handle.clone()).await.unwrap();
            let pairing = PairServer::bind_room("127.0.0.1:0".parse().unwrap(), WAIT, anchor.id())
                .await
                .unwrap();
            let offers = root.path().join("private-invitations");
            kk::profile::private_directory(&offers, true).unwrap();
            let invitation = offers.join("invitation.json");
            Invite::from_server(&pairing, &identity)
                .unwrap()
                .export(&invitation)
                .unwrap();
            let address = std::net::UdpSocket::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .to_string();
            let recipient = Command::new(env!("CARGO_BIN_EXE_kk"))
                .arg("--profile")
                .arg(&profile_b)
                .args([
                    "room",
                    "join",
                    "team",
                    "--invitation",
                    invitation.to_str().unwrap(),
                    "--bind",
                    &address,
                    "--confirm",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let mut session = pairing
                .run_room_session(
                    &identity,
                    control.local_addr().unwrap().port(),
                    |_| async { Ok(true) },
                    |_| Ok(()),
                )
                .await
                .unwrap();
            let peer = session.peer().clone();
            session
                .respond(EnrollmentStage::Authority, WAIT, |_| async {
                    handle
                        .issue_enrollment_authority(peer.clone(), Rights::PARTICIPANT, WAIT)
                        .await?
                        .encode()
                })
                .await
                .unwrap();
            let _reply_queued_locally = session
                .respond(
                    EnrollmentStage::KeyPackage,
                    Duration::from_secs(10),
                    |bytes| {
                        let peer = &peer;
                        let handle = &handle;
                        let profile_b = &profile_b;
                        async move {
                            let offer: Package = serde_json::from_slice(&bytes).unwrap();
                            handle
                                .prepare_scoped_admission(peer.clone(), offer.package, WAIT)
                                .await?;
                            assert!(handle.submit_prepared(WAIT).await?.accepted);
                            handle
                                .provision_scoped_learner(
                                    peer.clone(),
                                    Node::for_peer(&peer.peer, Some(offer.endpoint)),
                                    WAIT,
                                )
                                .await?;
                            assert!(profile_b.join("rooms/team/manifest.json").exists());
                            let mut recipient = recipient;
                            recipient.start_kill().unwrap();
                            assert!(!recipient.wait().await.unwrap().success());
                            // The recipient cannot receive this phase reply or create Group.
                            Ok::<_, kk::Error>(b"recipient already killed".to_vec())
                        }
                    },
                )
                .await;
            // A local QUIC write may succeed after the recipient dies; that
            // cannot prove receipt or private activation. Check durable state.
            drop(session);
            let private = profile_b.join("rooms/team/private");
            let candidate = CryptoMember::open_private(&b, &anchor, &private).unwrap();
            assert!(!Group::has_membership(&candidate).unwrap());
            let original = serde_json::to_vec(&candidate.public_identity().unwrap()).unwrap();
            drop(candidate);
            let mut recovered = Command::new(env!("CARGO_BIN_EXE_kk"))
                .arg("--profile")
                .arg(&profile_b)
                .args(["room", "serve", "team"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let mut output = BufReader::new(recovered.stdout.take().unwrap());
            tokio::time::timeout(WAIT, async {
                loop {
                    let mut line = String::new();
                    if output.read_line(&mut line).await.unwrap() == 0 {
                        let mut error = String::new();
                        use tokio::io::AsyncReadExt;
                        if let Some(mut stderr) = recovered.stderr.take() {
                            stderr.read_to_string(&mut error).await.unwrap();
                        }
                        panic!("CLI recovery exited: {error}");
                    }
                    if line.starts_with("ROOM SERVING") {
                        break;
                    }
                }
            })
            .await
            .unwrap();
            command(
                &profile_b,
                &["room", "send", "team", "recovered without key reset"],
            )
            .await;
            let event = tokio::time::timeout(WAIT, events.recv())
                .await
                .unwrap()
                .unwrap();
            let Event::ReferencedText { text, .. } = event else {
                panic!("missing authenticated reference")
            };
            assert_eq!(text.content.text(), "recovered without key reset");
            command(&profile_b, &["room", "stop", "team"]).await;
            assert!(recovered.wait().await.unwrap().success());
            let member = CryptoMember::open_private(&b, &anchor, &private).unwrap();
            assert!(Group::has_membership(&member).unwrap());
            assert_eq!(
                serde_json::to_vec(&member.public_identity().unwrap()).unwrap(),
                original
            );
            assert!(Group::restore(member).unwrap().is_active());
            control.unregister_room().unwrap();
            handle.shutdown(WAIT).await.unwrap();
            drop(running.await.unwrap());
            drop(handle);
            match Arc::try_unwrap(control) {
                Ok(control) => control.shutdown().await.unwrap(),
                Err(_) => panic!("recovery fixture retained a control owner"),
            }
            Ok::<(), kk::Error>(())
        })
        .await
        .unwrap();
}
