use super::*;
use crate::{
    contacts::{Contact, Permissions, Registry},
    rooms::{
        Grant, Rights, RootBundle,
        group::Group,
        mls::CryptoMember,
        runtime::{Actor, Room},
    },
};

const WAIT: Duration = Duration::from_secs(5);
const ALPN: &[u8] = b"kk-room-control/1";

async fn paused_publication_races(
    controls: &[Arc<ControlNode>],
    identities: &[Arc<Identity>],
    directory: &std::path::Path,
) {
    let target = controls[0].id();
    let node = Node::for_peer(
        &identities[0].peer(),
        Some(controls[0].local_addr().unwrap()),
    );
    for mode in ["sdk", "directory", "timeout"] {
        if mode == "directory" {
            crate::profile::Settings::update_with(directory, |settings| {
                settings.display_name = "Old directory name".into();
                settings.share_display_name = true;
                Ok(())
            })
            .unwrap();
            controls[0]
                .configure_profile_directory(Some(directory.to_path_buf()))
                .unwrap();
        } else {
            controls[0]
                .configure_profile_name("Old SDK name".into(), true)
                .unwrap();
        }
        let (reached, mut resume) = controls[0].network.pause_profile_publication();
        let (result, stable) = tokio::time::timeout(Duration::from_secs(7), async {
            tokio::join!(controls[1].peer_profile(target, WAIT), async {
                reached.await.unwrap(); // after BOTH membership checks, before first response poll
                let stable = controls[1].network.profile_link_id(target).unwrap();
                // A required RPC on the very same cached QUIC connection must
                // progress while the optional response is paused.
                controls[1]
                    .network
                    .read_barrier(target, &node, WAIT)
                    .await
                    .unwrap();
                if mode == "timeout" {
                    tokio::time::timeout(Duration::from_secs(4), resume.closed())
                        .await
                        .unwrap();
                } else {
                    let source = controls[0].network.clone();
                    let directory = directory.to_path_buf();
                    tokio::time::timeout(
                        Duration::from_secs(1),
                        tokio::task::spawn_blocking(move || {
                            if mode == "sdk" {
                                source.configure_profile_directory(None).unwrap();
                            } else {
                                let output = std::process::Command::new(
                                    std::env::current_exe().unwrap(),
                                )
                                .args([
                                    "--exact",
                                    "profile::settings::publication_tests::settings_opt_out_child",
                                    "--nocapture",
                                ])
                                .env("KK_TEST_PROFILE_PUBLICATION_DIRECTORY", directory)
                                .output()
                                .unwrap();
                                assert!(
                                    output.status.success(),
                                    "{}",
                                    String::from_utf8_lossy(&output.stderr)
                                );
                            }
                        }),
                    )
                    .await
                    .unwrap()
                    .unwrap();
                    resume.send(()).unwrap();
                }
                stable
            })
        })
        .await
        .unwrap();
        assert!(
            result.is_err(),
            "{mode}: a paused old-name response must not be accepted"
        );
        assert_eq!(controls[1].network.profile_link_id(target), Some(stable));
        controls[1]
            .network
            .read_barrier(target, &node, WAIT)
            .await
            .unwrap();
        if mode != "timeout" {
            assert_eq!(
                controls[1].peer_profile(target, WAIT).await.unwrap().name,
                None
            );
        }
    }
    crate::profile::Settings::update_with(directory, |settings| {
        settings.display_name = "Directory Alice".into();
        settings.share_display_name = true;
        Ok(())
    })
    .unwrap();
    controls[0]
        .configure_profile_directory(Some(directory.to_path_buf()))
        .unwrap();
}

async fn background_cache_roundtrip(
    controls: &[Arc<ControlNode>],
    handles: &[crate::rooms::runtime::Handle],
) {
    let members = [controls[0].id(), controls[1].id()];
    let before = controls[0].network.profile_cache_probe();
    let owners = Arc::strong_count(&controls[0]);
    let started = std::time::Instant::now();
    // The synchronous SDK may be used outside an entered Tokio context; the
    // transport owner must schedule on its recorded runtime, not the caller's.
    assert!(
        std::thread::scope(|scope| scope
            .spawn(|| controls[0].cached_peer_profiles(1, &members))
            .join()
            .unwrap())
        .unwrap()
        .is_empty()
    );
    // No await: on this current-thread runtime, background tasks cannot execute
    // yet. Repeated status lookups must create just one reserved refresh.
    for _ in 0..128 {
        assert!(
            controls[0]
                .cached_peer_profiles(1, &members)
                .unwrap()
                .is_empty()
        );
    }
    assert!(started.elapsed() < Duration::from_millis(250));
    assert_eq!(controls[0].network.profile_cache_probe().0, before.0 + 1);
    assert_eq!(controls[0].network.profile_cache_probe().4, 1);
    assert_eq!(Arc::strong_count(&controls[0]), owners);
    for (index, expected) in [(0, "Bob"), (1, "Alice")] {
        let peer = members[1 - index];
        tokio::time::timeout(WAIT, async {
            loop {
                let names = controls[index].cached_peer_profiles(1, &members).unwrap();
                assert!(!names.contains_key(&members[index]));
                if names.get(&peer).is_some_and(|name| name == expected) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert!(
            controls[index]
                .cached_peer_profiles(0, &members)
                .unwrap()
                .is_empty()
        );
    }
    controls[1]
        .configure_profile_name("Bob".into(), false)
        .unwrap();
    controls[0].unregister_room().unwrap();
    assert!(
        controls[0]
            .cached_peer_profiles(1, &members)
            .unwrap()
            .is_empty()
    );
    controls[0].register_room(handles[0].clone()).await.unwrap();
    let completed = controls[0].network.profile_cache_probe().1;
    assert!(
        controls[0]
            .cached_peer_profiles(1, &members)
            .unwrap()
            .is_empty()
    );
    tokio::time::timeout(WAIT, async {
        while controls[0].network.profile_cache_probe().1 == completed {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        controls[0]
            .cached_peer_profiles(1, &members)
            .unwrap()
            .is_empty()
    );
    let count = controls[0].network.profile_cache_probe().0;
    for _ in 0..128 {
        assert!(
            controls[0]
                .cached_peer_profiles(1, &members)
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(
        controls[0].network.profile_cache_probe().0,
        count,
        "None must back off instead of busy-refreshing"
    );
    controls[1]
        .configure_profile_name("Bob".into(), true)
        .unwrap();
    // Removing a member from a same-epoch snapshot cannot be rolled back by
    // replaying the old full snapshot. Re-registration starts a new generation.
    assert!(
        controls[0]
            .cached_peer_profiles(1, &members[..1])
            .unwrap()
            .is_empty()
    );
    assert!(
        controls[0]
            .cached_peer_profiles(1, &members)
            .unwrap()
            .is_empty()
    );
    controls[0].unregister_room().unwrap();
    controls[0].register_room(handles[0].clone()).await.unwrap();
}

// Direct mTLS probe bypasses the getter's local precheck and exercises the
// server's room/TLS/private-membership gate itself.
async fn wire_get(
    control: &ControlNode,
    local: &Identity,
    remote: &Peer,
    envelope: serde_json::Value,
) -> Result<serde_json::Value> {
    tokio::time::timeout(WAIT, async {
        let config = crate::transport::server_config(
            local,
            std::slice::from_ref(remote),
            &[ALPN],
            crate::transport::Policy::Room,
        )?;
        let (endpoint, _drivers) =
            crate::transport::owned_server(config, "127.0.0.1:0".parse().unwrap())?;
        let client =
            crate::transport::client_config(local, remote, ALPN, crate::transport::Policy::Room)?;
        let connection = endpoint
            .connect_with(client, control.local_addr()?, config::TLS_NAME)
            .map_err(|_| Error::Network)?
            .await
            .map_err(crate::error::connection_error)?;
        crate::transport::verify_peer(&connection, remote)?;
        let (mut send, mut recv) = connection.open_bi().await.map_err(|_| Error::Network)?;
        let bytes = serde_json::to_vec(&envelope).unwrap();
        send.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
        send.write_all(&bytes).await?;
        send.finish().map_err(|_| Error::Network)?;
        let mut prefix = [0; 4];
        recv.read_exact(&mut prefix).await?;
        let length = u32::from_be_bytes(prefix) as usize;
        assert!(length <= config::ROOM_PROFILE_WIRE_BYTES);
        let mut bytes = vec![0; length];
        recv.read_exact(&mut bytes).await?;
        endpoint.close(0u32.into(), b"");
        serde_json::from_slice(&bytes).map_err(|_| Error::Protocol)
    })
    .await
    .map_err(|_| Error::Timeout)?
}

// Pinned raw responder with a REAL private-room authorizer on the getter side.
// The response is deliberately untrusted; this is not a valid remote room.
async fn untrusted_response(
    control: &ControlNode,
    local: &Identity,
    remote: &Identity,
    response: serde_json::Value,
    remove_during_request: Option<&crate::rooms::runtime::Handle>,
) -> Result<network::ProfileMetadata> {
    let config = crate::transport::server_config(
        remote,
        &[local.peer()],
        &[ALPN],
        crate::transport::Policy::Room,
    )
    .unwrap();
    let (endpoint, _drivers) =
        crate::transport::owned_server(config, "127.0.0.1:0".parse().unwrap()).unwrap();
    let node = Node::for_peer(&remote.peer(), Some(endpoint.local_addr().unwrap()));
    let target = PeerId::from_peer(&remote.peer());
    let (result, ()) = tokio::join!(control.network.peer_profile(target, &node, WAIT), async {
        let connection = endpoint.accept().await.unwrap().await.unwrap();
        crate::transport::verify_peer(&connection, &local.peer()).unwrap();
        let (mut send, mut recv) = connection.accept_bi().await.unwrap();
        let mut prefix = [0; 4];
        recv.read_exact(&mut prefix).await.unwrap();
        let length = u32::from_be_bytes(prefix) as usize;
        assert!(length < 1024);
        let mut body = vec![0; length];
        recv.read_exact(&mut body).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({"room": control.room_id(), "request": "ProfileGet"})
        );
        if let Some(handle) = remove_during_request {
            handle.prepare_departure(remote.peer(), WAIT).await.unwrap();
            assert!(handle.submit_prepared(WAIT).await.unwrap().accepted);
            handle.synchronize(WAIT).await.unwrap();
            assert_eq!(handle.status(WAIT).await.unwrap().epoch, 2);
        }
        let bytes = serde_json::to_vec(&response).unwrap();
        send.write_all(&(bytes.len() as u32).to_be_bytes())
            .await
            .unwrap();
        send.write_all(&bytes).await.unwrap();
        send.finish().unwrap();
        // Keep this pinned link alive until the client has consumed FIN.
        let _ = send.stopped().await;
    });
    endpoint.close(0u32.into(), b"");
    result
}

#[tokio::test]
async fn profile_get_uses_real_private_rooms_readers_mtls_and_live_privacy() {
    let phase = std::cell::Cell::new("initialization");
    tokio::time::timeout(
        Duration::from_secs(40),
        tokio::task::LocalSet::new().run_until(async {
            let root = tempfile::tempdir().unwrap();
            let identities: Vec<_> = (0..2)
                .map(|i| {
                    Arc::new(Identity::create(&root.path().join(format!("identity-{i}"))).unwrap())
                })
                .collect();
            let anchor = RootBundle::create(&identities[0], "Profile metadata test")
                .unwrap()
                .verify(&identities[0].peer())
                .unwrap();
            let private = CryptoMember::create_private(
                &identities[0],
                &anchor,
                vec![],
                &root.path().join("private-0"),
            )
            .unwrap();
            let creator = private.public_identity().unwrap();
            let group = Group::create(private).unwrap();
            let peers = identities
                .iter()
                .map(|i| (PeerId::from_peer(&i.peer()), i.peer()))
                .collect::<BTreeMap<_, _>>();
            let mut controls = Vec::new();
            let mut voters = BTreeMap::new();
            for (i, identity) in identities.iter().enumerate() {
                let mut timing = network::configuration().unwrap();
                timing.heartbeat_interval = 50;
                timing.election_timeout_min = 200;
                timing.election_timeout_max = 400;
                let control = Arc::new(
                    ControlNode::start(
                        identity.clone(),
                        &root.path().join(format!("control-{i}")),
                        Settings {
                            anchor: anchor.clone(),
                            listen: "127.0.0.1:0".parse().unwrap(),
                            peers: peers.clone(),
                            timing,
                        },
                        Mode::Create(creator.clone()),
                    )
                    .await
                    .unwrap(),
                );
                voters.insert(
                    control.id(),
                    Node {
                        endpoint: Some(control.local_addr().unwrap()),
                        ..Default::default()
                    },
                );
                controls.push(control);
            }
            phase.set("initial quorum");
            controls[0].initialize(voters).await.unwrap();
            let mut metrics = controls[0].raft.metrics();
            tokio::time::timeout(WAIT, metrics.wait_for(|m| m.current_leader.is_some()))
                .await
                .unwrap()
                .unwrap();
            let profile0 = root.path().join("profile-0");
            let mut owner = Room::new(
                identities[0].clone(),
                controls[0].clone(),
                group,
                Registry::open(profile0.clone()).unwrap(),
            )
            .unwrap();
            let grant = Grant::issue(
                &anchor,
                &[],
                &identities[0],
                &identities[1].peer(),
                Rights::READER,
            )
            .unwrap();
            let mut candidate = CryptoMember::create_private(
                &identities[1],
                &anchor,
                vec![grant],
                &root.path().join("private-1"),
            )
            .unwrap();
            owner
                .prepare_admission(&candidate.key_package().unwrap(), WAIT)
                .await
                .unwrap();
            assert!(owner.submit_prepared(WAIT).await.unwrap().accepted);
            let reader = Room::join(
                identities[1].clone(),
                controls[1].clone(),
                candidate,
                &identities[0].peer(),
                Registry::open(root.path().join("profile-1")).unwrap(),
                WAIT,
            )
            .await
            .unwrap();
            phase.set("actor registration");
            let mut handles = Vec::new();
            let mut actors = Vec::new();
            for (i, room) in [owner, reader].into_iter().enumerate() {
                let (handle, actor, _events) = Actor::new(room);
                actors.push(tokio::task::spawn_local(actor.run()));
                handle.ready(WAIT).await.unwrap();
                controls[i].register_room(handle.clone()).await.unwrap();
                handles.push(handle);
            }
            phase.set("authenticated metadata");
            let a = controls[0].id();
            let b = controls[1].id();
            // Default is no disclosure; a reader may GET without publisher rights.
            assert_eq!(controls[1].peer_profile(a, WAIT).await.unwrap().name, None);
            controls[0]
                .configure_profile_name("Alice".into(), true)
                .unwrap();
            let name = controls[1].peer_profile(a, WAIT).await.unwrap();
            assert_eq!(
                name,
                network::ProfileMetadata {
                    version: 1,
                    epoch: 1,
                    name: Some("Alice".into())
                }
            );
            assert_eq!(
                wire_get(
                    &controls[0],
                    &identities[1],
                    &identities[0].peer(),
                    serde_json::json!({"room": anchor.id(), "request": "ProfileGet"})
                )
                .await
                .unwrap(),
                serde_json::json!({"version":1,"epoch":1,"name":"Alice"})
            );
            assert!(
                wire_get(
                    &controls[0],
                    &identities[1],
                    &identities[0].peer(),
                    serde_json::json!({"room": vec![0u8;32], "request": "ProfileGet"})
                )
                .await
                .is_err()
            );
            assert!(
                wire_get(
                    &controls[0],
                    &identities[1],
                    &identities[0].peer(),
                    serde_json::json!({"room": anchor.id(), "request": "ProfileGet", "author": a.0})
                )
                .await
                .is_err()
            );
            controls[1]
                .configure_profile_name("Bob".into(), true)
                .unwrap();
            assert_eq!(
                controls[0]
                    .peer_profile(b, WAIT)
                    .await
                    .unwrap()
                    .name
                    .as_deref(),
                Some("Bob")
            );
            phase.set("background cache");
            background_cache_roundtrip(&controls, &handles).await;
            phase.set("directory disclosure");
            controls[0]
                .configure_profile_name("Alice".into(), false)
                .unwrap();
            assert_eq!(controls[1].peer_profile(a, WAIT).await.unwrap().name, None);
            assert!(
                controls[0]
                    .configure_profile_name("x".repeat(65), true)
                    .is_err()
            );
            assert!(matches!(
                untrusted_response(
                    &controls[0],
                    &identities[0],
                    &identities[1],
                    serde_json::json!({"version":1,"epoch":0,"name":"stale"}),
                    None
                )
                .await,
                Err(Error::Denied)
            ));

            // Configure an existing profile without ever loading its private key.
            crate::profile::private_directory(&profile0, false).unwrap();
            std::fs::create_dir(profile0.join("identity")).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                profile0.join("identity"),
                std::fs::Permissions::from_mode(0o700),
            )
            .unwrap();
            let cert = profile0.join("identity/certificate.der");
            std::fs::write(&cert, identities[0].peer().certificate_der()).unwrap();
            assert!(!profile0.join("identity/private-key.der").exists());
            crate::profile::Settings::update_with(&profile0, |s| {
                s.display_name = "Directory Alice".into();
                s.share_display_name = true;
                Ok(())
            })
            .unwrap();
            controls[0]
                .configure_profile_directory(Some(profile0.clone()))
                .unwrap();
            assert_eq!(
                controls[1]
                    .peer_profile(a, WAIT)
                    .await
                    .unwrap()
                    .name
                    .as_deref(),
                Some("Directory Alice")
            );
            crate::profile::Settings::update_with(&profile0, |s| {
                s.share_display_name = false;
                Ok(())
            })
            .unwrap();
            assert_eq!(controls[1].peer_profile(a, WAIT).await.unwrap().name, None);
            crate::profile::Settings::update_with(&profile0, |s| {
                s.share_display_name = true;
                Ok(())
            })
            .unwrap();
            std::fs::write(&cert, identities[1].peer().certificate_der()).unwrap();
            assert!(
                controls[0]
                    .configure_profile_directory(Some(profile0.clone()))
                    .is_err()
            );
            assert!(controls[1].peer_profile(a, WAIT).await.is_err()); // recheck already-selected source
            std::fs::write(&cert, identities[0].peer().certificate_der()).unwrap();
            assert_eq!(
                controls[1]
                    .peer_profile(a, WAIT)
                    .await
                    .unwrap()
                    .name
                    .as_deref(),
                Some("Directory Alice")
            );
            let settings_path = profile0.join("settings.json");
            let saved = std::fs::read(&settings_path).unwrap();
            std::fs::write(&settings_path, vec![b' '; 128 * 1024 + 1]).unwrap();
            assert!(controls[1].peer_profile(a, WAIT).await.is_err());
            std::fs::write(settings_path, saved).unwrap();
            phase.set("paused publication races");
            paused_publication_races(&controls, &identities, &profile0).await;

            // A real consented media publication keeps using its authenticated
            // link while an optional GET on that link fails. Synthetic RTP only;
            // there is no codec/device or physical capture claim here.
            phase.set("parallel media");
            let mut screen = controls[1]
                .subscribe_media(a, crate::media::Kind::Screen)
                .await
                .unwrap();
            let mut publication = controls[0]
                .publish_media(crate::media::Kind::Screen, vec![b])
                .await
                .unwrap();
            let packet = |sequence: u16| {
                let mut bytes = vec![0x80, crate::media::Kind::Screen.codec().payload_type()];
                bytes.extend(sequence.to_be_bytes());
                bytes.extend([0, 0, 0, 1, 1, 2, 3, 4, 0x5a]);
                crate::media::Packet::parse(crate::media::Kind::Screen.codec(), bytes).unwrap()
            };
            publication.send(&packet(1)).await.unwrap();
            assert_eq!(
                &tokio::time::timeout(WAIT, screen.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .packet
                    .bytes()[2..4],
                &1u16.to_be_bytes()
            );
            let profile1 = root.path().join("profile-1");
            crate::profile::private_directory(&profile1.join("identity"), true).unwrap();
            let cert1 = profile1.join("identity/certificate.der");
            std::fs::write(&cert1, identities[1].peer().certificate_der()).unwrap();
            controls[1]
                .configure_profile_directory(Some(profile1))
                .unwrap();
            std::fs::write(&cert1, identities[0].peer().certificate_der()).unwrap();
            assert!(controls[0].peer_profile(b, WAIT).await.is_err());
            publication.send(&packet(2)).await.unwrap();
            assert_eq!(
                &tokio::time::timeout(WAIT, screen.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .packet
                    .bytes()[2..4],
                &2u16.to_be_bytes()
            );
            publication.finish().await.unwrap();
            drop(screen);
            controls[1]
                .configure_profile_name("Bob".into(), true)
                .unwrap();

            phase.set("local veto and removal");
            // Local veto is enforced on both the receiving and disclosing endpoints.
            for (incoming, outgoing) in [(false, true), (true, false)] {
                let registry = Registry::open(profile0.clone()).unwrap();
                registry
                    .put(
                        "reader",
                        Contact {
                            certificate: identities[1].peer().certificate_der().to_vec(),
                            local_identity: identities[0].peer().fingerprint(),
                            incoming: Permissions {
                                rooms: incoming,
                                ..Default::default()
                            },
                            outgoing: Permissions {
                                rooms: outgoing,
                                ..Default::default()
                            },
                            endpoints: vec![],
                        },
                    )
                    .unwrap();
                assert!(controls[0].peer_profile(b, WAIT).await.is_err());
                assert!(controls[1].peer_profile(a, WAIT).await.is_err());
            }
            let outsider = Identity::create(&root.path().join("outsider")).unwrap();
            assert!(
                controls[0]
                    .peer_profile(PeerId::from_peer(&outsider.peer()), WAIT)
                    .await
                    .is_err()
            );
            let registry = Registry::open(profile0.clone()).unwrap();
            registry
                .put(
                    "reader",
                    Contact {
                        certificate: identities[1].peer().certificate_der().to_vec(),
                        local_identity: identities[0].peer().fingerprint(),
                        incoming: Permissions {
                            rooms: true,
                            ..Default::default()
                        },
                        outgoing: Permissions {
                            rooms: true,
                            ..Default::default()
                        },
                        endpoints: vec![],
                    },
                )
                .unwrap();
            // Commit a real private epoch change after the request has passed
            // pre-authorization. Even a valid-looking old response is discarded.
            assert!(
                untrusted_response(
                    &controls[0],
                    &identities[0],
                    &identities[1],
                    serde_json::json!({"version":1,"epoch":1,"name":"removed reader"}),
                    Some(&handles[0])
                )
                .await
                .is_err()
            );
            assert!(controls[0].peer_profile(b, WAIT).await.is_err());
            assert!(
                wire_get(
                    &controls[0],
                    &identities[1],
                    &identities[0].peer(),
                    serde_json::json!({"room": anchor.id(), "request": "ProfileGet"})
                )
                .await
                .is_err()
            );
            phase.set("shutdown actors");
            for (control, handle) in controls.iter().zip(&handles) {
                control.unregister_room().unwrap();
                handle.shutdown(WAIT).await.unwrap();
            }
            for actor in actors {
                drop(actor.await.unwrap());
            }
            drop(handles);
            drop(metrics);
            phase.set("shutdown controls");
            for control in controls {
                match Arc::try_unwrap(control) {
                    Ok(control) => control.shutdown().await.unwrap(),
                    Err(_) => panic!("profile fixture leaked control owner"),
                }
            }
        }),
    )
    .await
    .unwrap_or_else(|error| panic!("profile fixture timed out in {}: {error}", phase.get()));
}
