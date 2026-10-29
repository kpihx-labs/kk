use super::*;
use crate::rooms::{Grant, Rights, RootBundle, mls::CryptoMember};
use futures_util::FutureExt;

// A deliberately paced, pinned wire peer makes the resume response a barrier:
// the real service has selected its inbox and spent its reservation, but cannot
// consume any body bytes until the test releases them. No timing-based races.
mod consent {
    use super::*;
    use crate::rooms::{
        group::Group,
        runtime::{Actor, Room},
    };
    use sha2::{Digest, Sha256};

    const DEADLINE: Duration = Duration::from_secs(8);
    const ALPN: &[u8] = b"kk-room-control/1";

    async fn fixture(root: &Path) -> (Arc<ControlNode>, Arc<Identity>, Identity, Room) {
        let alice = Arc::new(Identity::create(&root.join("alice")).unwrap());
        let bob = Identity::create(&root.join("bob")).unwrap();
        let anchor = RootBundle::create(&alice, "Live file consent regression")
            .unwrap()
            .verify(&alice.peer())
            .unwrap();
        let private =
            CryptoMember::create_private(&alice, &anchor, vec![], &root.join("private")).unwrap();
        let creator = private.public_identity().unwrap();
        let group = Group::create(private).unwrap();
        let id = PeerId::from_peer(&alice.peer());
        let control = Arc::new(
            ControlNode::start(
                alice.clone(),
                &root.join("control"),
                Settings {
                    anchor: anchor.clone(),
                    listen: "127.0.0.1:0".parse().unwrap(),
                    peers: BTreeMap::from([
                        (id, alice.peer()),
                        (PeerId::from_peer(&bob.peer()), bob.peer()),
                    ]),
                    timing: timing(),
                },
                Mode::Create(creator),
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
        let mut metrics = control.raft.metrics();
        tokio::time::timeout(DEADLINE, metrics.wait_for(|m| m.current_leader == Some(id)))
            .await
            .unwrap()
            .unwrap();
        let mut room = Room::new(
            alice.clone(),
            control.clone(),
            group,
            crate::contacts::Registry::open(root.join("profile")).unwrap(),
        )
        .unwrap();
        let grant = Grant::issue(&anchor, &[], &alice, &bob.peer(), Rights::PARTICIPANT).unwrap();
        let mut candidate = CryptoMember::new(&bob, &anchor, vec![grant]).unwrap();
        let package = candidate.key_package().unwrap();
        room.prepare_admission(&package, DEADLINE).await.unwrap();
        assert!(room.submit_prepared(DEADLINE).await.unwrap().accepted);
        room.synchronize(DEADLINE).await.unwrap();
        assert_eq!(room.peers().len(), 2);
        (control, alice, bob, room)
    }

    async fn write_frame(send: &mut quinn::SendStream, frame: serde_json::Value) {
        let bytes = serde_json::to_vec(&frame).unwrap();
        send.write_all(&(bytes.len() as u32).to_be_bytes())
            .await
            .unwrap();
        send.write_all(&bytes).await.unwrap();
    }

    async fn read_frame(recv: &mut quinn::RecvStream) -> serde_json::Value {
        let mut length = [0; 4];
        recv.read_exact(&mut length).await.unwrap();
        let length = u32::from_be_bytes(length) as usize;
        assert!(length <= crate::config::ROOM_CONTROL_RPC_BYTES);
        let mut bytes = vec![0; length];
        recv.read_exact(&mut bytes).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn offer(
        connection: &quinn::Connection,
        room: [u8; 32],
        bytes: &[u8],
    ) -> (quinn::SendStream, quinn::RecvStream, [u8; 32]) {
        let (mut send, mut recv) = connection.open_bi().await.unwrap();
        write_frame(
            &mut send,
            serde_json::json!({"room": room, "request": "RoomFile"}),
        )
        .await;
        assert_eq!(
            read_frame(&mut recv).await,
            serde_json::json!({"RoomFile": {"Accepted": room}})
        );
        let hash: [u8; 32] = Sha256::digest(bytes).into();
        send.write_all(&[0]).await.unwrap();
        send.write_all(&(bytes.len() as u64).to_be_bytes())
            .await
            .unwrap();
        send.write_all(&hash).await.unwrap();
        (send, recv, hash)
    }

    async fn resume_barrier(recv: &mut quinn::RecvStream) {
        let mut resume = [0; 40];
        recv.read_exact(&mut resume).await.unwrap();
        assert_eq!(&resume[..8], &0u64.to_be_bytes());
        assert_eq!(&resume[8..], Sha256::digest([]).as_slice());
    }

    async fn shutdown(control: Arc<ControlNode>) {
        match Arc::try_unwrap(control) {
            Ok(control) => control.shutdown().await.unwrap(),
            Err(_) => panic!("file test leaked a service owner"),
        }
    }

    #[tokio::test]
    async fn live_receive_withdrawal_does_not_revive_after_regrant_or_refund_budget() {
        receive_case(true, false).await;
    }

    #[tokio::test]
    async fn live_receive_withdrawal_stops_transfer_without_refunding_budget() {
        receive_case(false, false).await;
    }

    #[tokio::test]
    async fn receive_cannot_publish_after_final_authorization_then_withdrawal() {
        receive_case(false, true).await;
    }

    #[tokio::test]
    async fn receive_cannot_publish_after_final_authorization_then_regrant() {
        receive_case(true, true).await;
    }

    async fn receive_case(regrant_before_body: bool, at_publication: bool) {
        let stage = std::cell::Cell::new("create fixture");
        tokio::time::timeout(
            DEADLINE * 4,
            tokio::task::LocalSet::new().run_until(async {
                let root = tempfile::tempdir().unwrap();
                let (control, alice, bob, room) = fixture(root.path()).await;
                let (handle, actor, _events) = Actor::new(room);
                let running = tokio::task::spawn_local(actor.run());
                stage.set("ready and register actor");
                handle.ready(DEADLINE).await.unwrap();
                control.register_room(handle.clone()).await.unwrap();
                let payload = vec![0x6d; crate::config::FILE_CHUNK_BYTES * 2];
                let size = payload.len() as u64;
                let inbox = root.path().join("inbox");
                control.enable_files(inbox.clone(), size, size * 2).unwrap();
                let pause =
                    at_publication.then(|| control.network.pause_file_publication().unwrap());
                let endpoint = crate::transport::client(
                    control.local_addr().unwrap(),
                    &bob,
                    &alice.peer(),
                    ALPN,
                )
                .unwrap();
                stage.set("connect peer");
                let connection = endpoint
                    .connect(control.local_addr().unwrap(), crate::config::TLS_NAME)
                    .unwrap()
                    .await
                    .unwrap();
                stage.set("first offer and resume barrier");
                let (mut send, mut recv, hash) =
                    offer(&connection, control.room_id(), &payload).await;
                resume_barrier(&mut recv).await;
                let publication_release = if let Some((ready, release, _finished)) = pause {
                    stage.set("reach publication barrier");
                    send.write_all(&payload).await.unwrap();
                    send.finish().unwrap();
                    // Reached only after full-body verification, file syncing
                    // and the final async authorization have all succeeded.
                    ready.await.unwrap();
                    Some(release)
                } else {
                    None
                };
                assert!(!control.configure_files(None, size, size * 2).unwrap());
                assert!(!control.files_enabled().unwrap());
                if regrant_before_body {
                    assert!(
                        control
                            .configure_files(Some(inbox.clone()), size, size * 2)
                            .unwrap()
                    );
                }
                if let Some(release) = publication_release {
                    // Withdrawal has returned while the old commit worker is
                    // still paused outside its publication gate.
                    release.send(()).unwrap();
                } else {
                    let _ = send.write_all(&payload).await;
                    let _ = send.finish();
                }
                stage.set("revoked transfer reaches EOF without ACK");
                let ack = recv.read_to_end(32).await;
                assert!(
                    ack.as_ref().map_or(true, |bytes| bytes.is_empty()),
                    "revoked receive sent ACK bytes: {ack:?}"
                );
                let destination = inbox.join(format!(
                    "{}-{}.bin",
                    bob.peer().fingerprint(),
                    crate::files::FileInfo {
                        bytes: size,
                        sha256: hash,
                    }
                    .hash_hex()
                ));
                assert!(
                    !destination.exists(),
                    "revoked receive committed its destination"
                );
                if at_publication {
                    stage.set("verify retained private staging");
                    assert_staging_retained(&destination, &payload).await;
                }

                assert_eq!(control.files_enabled().unwrap(), regrant_before_body);
                control
                    .configure_files(Some(inbox.clone()), size, size * 2)
                    .unwrap();

                // A distinct new offer succeeds, proving regrant is effective rather
                // than merely leaving the entire receiver permanently disabled.
                let fresh = vec![0x71; payload.len()];
                stage.set("fresh offer and resume barrier");
                let (mut send, mut recv, fresh_hash) =
                    offer(&connection, control.room_id(), &fresh).await;
                resume_barrier(&mut recv).await;
                send.write_all(&fresh).await.unwrap();
                send.finish().unwrap();
                stage.set("fresh transfer verified ACK");
                assert_eq!(recv.read_to_end(32).await.unwrap(), fresh_hash);
                let fresh_destination = inbox.join(format!(
                    "{}-{}.bin",
                    bob.peer().fingerprint(),
                    crate::files::FileInfo {
                        bytes: size,
                        sha256: fresh_hash,
                    }
                    .hash_hex()
                ));
                assert_eq!(tokio::fs::read(&fresh_destination).await.unwrap(), fresh);
                control.configure_files(None, size, size * 2).unwrap();
                control
                    .configure_files(Some(inbox.clone()), size, size * 2)
                    .unwrap();
                assert_eq!(
                    tokio::fs::read(&fresh_destination).await.unwrap(),
                    fresh,
                    "withdrawal must not invalidate files committed before it"
                );
                stage.set("exhausted budget refuses another reservation");
                let (_send, mut recv, _) =
                    offer(&connection, control.room_id(), &vec![0x23; payload.len()]).await;
                let mut resume = [0; 40];
                assert!(
                    recv.read_exact(&mut resume).await.is_err(),
                    "withdrawal refunded an aborted or completed byte reservation"
                );
                assert!(
                    control
                        .configure_files(Some(inbox), size, size * 2 + 1)
                        .is_err()
                );
                connection.close(0u32.into(), b"");
                endpoint.close(0u32.into(), b"");
                control.unregister_room().unwrap();
                stage.set("actor shutdown");
                handle.shutdown(DEADLINE).await.unwrap();
                stage.set("actor join");
                drop(running.await.unwrap());
                stage.set("control shutdown");
                shutdown(control).await;
            }),
        )
        .await
        .unwrap_or_else(|error| {
            panic!(
                "receive consent fixture timed out at {} (regrant={regrant_before_body}, publication={at_publication}): {error}",
                stage.get()
            )
        });
    }

    #[tokio::test]
    async fn live_fetch_withdrawal_does_not_revive_after_regrant() {
        fetch_case(true, false).await;
    }

    #[tokio::test]
    async fn live_fetch_withdrawal_stops_transfer() {
        fetch_case(false, false).await;
    }

    #[tokio::test]
    async fn fetch_cannot_publish_after_final_authorization_then_withdrawal() {
        fetch_case(false, true).await;
    }

    #[tokio::test]
    async fn fetch_cannot_publish_after_final_authorization_then_regrant() {
        fetch_case(true, true).await;
    }

    async fn assert_staging_retained(destination: &Path, expected: &[u8]) {
        for (suffix, expected_bytes) in [(".kk-part", Some(expected)), (".kk-meta", None)] {
            let mut path = destination.as_os_str().to_os_string();
            path.push(suffix);
            let bytes = tokio::fs::read(Path::new(&path)).await.unwrap();
            if let Some(expected) = expected_bytes {
                assert_eq!(bytes, expected, "verified staging bytes must be retained");
            } else {
                assert_eq!(bytes.len(), 73, "resume binding must be retained");
            }
        }
    }

    #[derive(Clone, Copy)]
    enum Stop {
        Unregister,
        Close,
        FailClosed,
        OwnerDrop,
    }

    #[tokio::test]
    async fn final_receive_worker_cannot_publish_after_unregister_and_regrant() {
        stopped_receive(Stop::Unregister).await;
    }

    #[tokio::test]
    async fn final_receive_worker_cannot_publish_after_close() {
        stopped_receive(Stop::Close).await;
    }

    #[tokio::test]
    async fn final_receive_worker_cannot_publish_after_fail_closed() {
        stopped_receive(Stop::FailClosed).await;
    }

    #[tokio::test]
    async fn final_receive_worker_cannot_publish_after_owner_drop_and_recreate() {
        stopped_receive(Stop::OwnerDrop).await;
    }

    async fn stopped_receive(stop: Stop) {
        tokio::time::timeout(
            DEADLINE * 4,
            tokio::task::LocalSet::new().run_until(async {
                let root = tempfile::tempdir().unwrap();
                let (control, alice, bob, room) = fixture(root.path()).await;
                let room_id = control.room_id();
                let network = control.network.clone();
                let (handle, actor, _events) = Actor::new(room);
                let running = tokio::task::spawn_local(actor.run());
                handle.ready(DEADLINE).await.unwrap();
                control.register_room(handle.clone()).await.unwrap();
                let payload = vec![0x39; crate::config::FILE_CHUNK_BYTES + 1];
                let size = payload.len() as u64;
                let inbox = root.path().join("inbox");
                control.enable_files(inbox.clone(), size, size).unwrap();
                drop(network.clone());
                assert!(
                    network.files_enabled().unwrap(),
                    "temporary factory clones must not stop the owner"
                );
                let (ready, release, finished) = network.pause_file_publication().unwrap();
                let endpoint = crate::transport::client(
                    control.local_addr().unwrap(),
                    &bob,
                    &alice.peer(),
                    ALPN,
                )
                .unwrap();
                let connection = endpoint
                    .connect(control.local_addr().unwrap(), crate::config::TLS_NAME)
                    .unwrap()
                    .await
                    .unwrap();
                let (mut send, mut recv, hash) = offer(&connection, room_id, &payload).await;
                resume_barrier(&mut recv).await;
                send.write_all(&payload).await.unwrap();
                send.finish().unwrap();
                ready.await.unwrap();
                // Release the private actor's ControlNode reference, not its
                // registered handle or the transfer already in final publication.
                handle.shutdown(DEADLINE).await.unwrap();
                drop(running.await.unwrap());
                let mut owner = Some(control);
                match stop {
                    Stop::Unregister => network.unregister_room().unwrap(),
                    Stop::Close => network.close(),
                    Stop::FailClosed => network.fail_closed(),
                    Stop::OwnerDrop => drop(owner.take().unwrap()),
                }
                assert!(!network.files_enabled().unwrap());
                let replacement = if matches!(stop, Stop::OwnerDrop) {
                    // A fresh owner may reuse the same inbox while an old detached
                    // worker still exists. Its grant must not revive that worker.
                    let (next, server) = network::Factory::bind(
                        room_id,
                        alice.clone(),
                        BTreeMap::from([
                            (PeerId::from_peer(&alice.peer()), alice.peer()),
                            (PeerId::from_peer(&bob.peer()), bob.peer()),
                        ]),
                        "127.0.0.1:0".parse().unwrap(),
                    )
                    .unwrap();
                    next.enable_files(inbox.clone(), size, size).unwrap();
                    Some((next, server))
                } else {
                    None
                };
                if matches!(stop, Stop::Unregister) {
                    network
                        .configure_files(Some(inbox.clone()), size, size)
                        .unwrap();
                } else {
                    assert!(matches!(
                        network.configure_files(Some(inbox.clone()), size, size),
                        Err(Error::Cancelled)
                    ));
                    assert!(matches!(
                        network.enable_files(inbox.clone(), size, size),
                        Err(Error::Cancelled)
                    ));
                }
                // All stop boundaries above have returned before the worker runs.
                release.send(()).unwrap();
                assert!(
                    !finished.await.unwrap(),
                    "old worker published after its owner stopped"
                );
                let destination = inbox.join(format!(
                    "{}-{}.bin",
                    bob.peer().fingerprint(),
                    crate::files::FileInfo {
                        bytes: size,
                        sha256: hash
                    }
                    .hash_hex()
                ));
                assert!(!destination.exists());
                assert_staging_retained(&destination, &payload).await;
                let ack = recv.read_to_end(32).await;
                assert!(ack.as_ref().map_or(true, |bytes| bytes.is_empty()));
                drop(replacement);
                connection.close(0u32.into(), b"");
                endpoint.close(0u32.into(), b"");
                if let Some(control) = owner {
                    shutdown(control).await;
                }
            }),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn cancelled_final_fetch_worker_cannot_publish_while_owner_remains_active() {
        cancelled_fetch(false).await;
    }

    #[tokio::test]
    async fn cancelled_final_fetch_worker_cannot_publish_after_shutdown_and_recreate() {
        cancelled_fetch(true).await;
    }

    async fn cancelled_fetch(stop_owner: bool) {
        tokio::time::timeout(
            DEADLINE * 4,
            tokio::task::LocalSet::new().run_until(async {
                let root = tempfile::tempdir().unwrap();
                let (control, alice, bob, room) = fixture(root.path()).await;
                let room_id = control.room_id();
                let network = control.network.clone();
                let (handle, actor, _events) = Actor::new(room);
                let running = tokio::task::spawn_local(actor.run());
                handle.ready(DEADLINE).await.unwrap();
                control.register_room(handle.clone()).await.unwrap();
                let payload = vec![0x4b; crate::config::FILE_CHUNK_BYTES + 3];
                let size = payload.len() as u64;
                let hash: [u8; 32] = Sha256::digest(&payload).into();
                let expected = payload.clone();
                let inbox = root.path().join("inbox");
                control.enable_files(inbox.clone(), size, size).unwrap();
                let (ready, release, finished) = network.pause_file_publication().unwrap();
                let endpoint = crate::transport::server(
                    "127.0.0.1:0".parse().unwrap(),
                    &bob,
                    &alice.peer(),
                    ALPN,
                )
                .unwrap();
                let address = endpoint.local_addr().unwrap();
                let remote = tokio::task::spawn_local(async move {
                    let connection = endpoint.accept().await.unwrap().await.unwrap();
                    let (mut send, mut recv) = connection.accept_bi().await.unwrap();
                    assert_eq!(
                        read_frame(&mut recv).await,
                        serde_json::json!({"room": room_id, "request": {"FileFetch": hash}})
                    );
                    write_frame(
                        &mut send,
                        serde_json::json!({"RoomFile": {"Accepted": room_id}}),
                    )
                    .await;
                    send.write_all(&[0]).await.unwrap();
                    send.write_all(&size.to_be_bytes()).await.unwrap();
                    send.write_all(&hash).await.unwrap();
                    resume_barrier(&mut recv).await;
                    send.write_all(&payload).await.unwrap();
                    send.finish().unwrap();
                    let ack = recv.read_to_end(32).await;
                    assert!(ack.as_ref().map_or(true, |bytes| bytes.is_empty()));
                    endpoint.close(0u32.into(), b"");
                });
                let caller = network.clone();
                let target = PeerId::from_peer(&bob.peer());
                let fetch = tokio::task::spawn_local(async move {
                    caller
                        .fetch_file(
                            target,
                            &Node {
                                endpoint: Some(address),
                                ..Default::default()
                            },
                            hash,
                            DEADLINE,
                        )
                        .await
                });
                ready.await.unwrap();
                fetch.abort();
                assert!(fetch.await.unwrap_err().is_cancelled());
                assert!(
                    network.files_enabled().unwrap(),
                    "cancelling one transfer must not revoke room consent"
                );
                handle.shutdown(DEADLINE).await.unwrap();
                drop(running.await.unwrap());
                let owner = if stop_owner {
                    shutdown(control).await;
                    assert!(!network.files_enabled().unwrap());
                    None
                } else {
                    Some(control)
                };
                let replacement = if stop_owner {
                    let (next, server) = network::Factory::bind(
                        room_id,
                        alice.clone(),
                        BTreeMap::from([
                            (PeerId::from_peer(&alice.peer()), alice.peer()),
                            (target, bob.peer()),
                        ]),
                        "127.0.0.1:0".parse().unwrap(),
                    )
                    .unwrap();
                    next.enable_files(inbox.clone(), size, size).unwrap();
                    Some((next, server))
                } else {
                    None
                };
                release.send(()).unwrap();
                assert!(
                    !finished.await.unwrap(),
                    "cancelled detached worker published"
                );
                let destination = inbox.join(format!(
                    "{}-{}.bin",
                    bob.peer().fingerprint(),
                    crate::files::FileInfo {
                        bytes: size,
                        sha256: hash
                    }
                    .hash_hex()
                ));
                assert!(!destination.exists());
                assert_staging_retained(&destination, &expected).await;
                remote.await.unwrap();
                drop(replacement);
                if let Some(control) = owner {
                    shutdown(control).await;
                }
            }),
        )
        .await
        .unwrap();
    }

    async fn fetch_case(regrant_before_body: bool, at_publication: bool) {
        tokio::time::timeout(
            DEADLINE * 4,
            tokio::task::LocalSet::new().run_until(async {
                let root = tempfile::tempdir().unwrap();
                let (control, alice, bob, room) = fixture(root.path()).await;
                let (handle, actor, _events) = Actor::new(room);
                let running = tokio::task::spawn_local(actor.run());
                handle.ready(DEADLINE).await.unwrap();
                control.register_room(handle.clone()).await.unwrap();
                let payload = vec![0x47; crate::config::FILE_CHUNK_BYTES * 2];
                let size = payload.len() as u64;
                let hash: [u8; 32] = Sha256::digest(&payload).into();
                let inbox = root.path().join("inbox");
                control.enable_files(inbox.clone(), size, size).unwrap();
                let pause =
                    at_publication.then(|| control.network.pause_file_publication().unwrap());
                let endpoint = crate::transport::server(
                    "127.0.0.1:0".parse().unwrap(),
                    &bob,
                    &alice.peer(),
                    ALPN,
                )
                .unwrap();
                let address = endpoint.local_addr().unwrap();
                let room_id = control.room_id();
                let expected_payload = payload.clone();
                let (ready, barrier) = tokio::sync::oneshot::channel();
                let (release, released) = tokio::sync::oneshot::channel();
                let remote = tokio::task::spawn_local(async move {
                    let connection = endpoint.accept().await.unwrap().await.unwrap();
                    let (mut send, mut recv) = connection.accept_bi().await.unwrap();
                    assert_eq!(
                        read_frame(&mut recv).await,
                        serde_json::json!({"room": room_id, "request": {"FileFetch": hash}})
                    );
                    write_frame(
                        &mut send,
                        serde_json::json!({"RoomFile": {"Accepted": room_id}}),
                    )
                    .await;
                    send.write_all(&[0]).await.unwrap();
                    send.write_all(&size.to_be_bytes()).await.unwrap();
                    send.write_all(&hash).await.unwrap();
                    resume_barrier(&mut recv).await;
                    ready.send(()).unwrap();
                    released.await.unwrap();
                    let _ = send.write_all(&payload).await;
                    let _ = send.finish();
                    let ack = recv.read_to_end(32).await;
                    assert!(
                        ack.as_ref().map_or(true, |bytes| bytes.is_empty()),
                        "revoked fetch sent ACK bytes: {ack:?}"
                    );
                    endpoint.close(0u32.into(), b"");
                });
                // Use the service's actual network owner with an explicit pinned
                // address. The synthetic peer is not a Raft controller; public
                // configured-node routing is deliberately outside this regression.
                let remote_node = Node {
                    endpoint: Some(address),
                    ..Default::default()
                };
                let fetch = control.network.fetch_file(
                    PeerId::from_peer(&bob.peer()),
                    &remote_node,
                    hash,
                    DEADLINE,
                );
                let revoke = async {
                    barrier.await.unwrap();
                    let mut body_release = Some(release);
                    let publication_release = if let Some((ready, release, _finished)) = pause {
                        body_release.take().unwrap().send(()).unwrap();
                        ready.await.unwrap();
                        Some(release)
                    } else {
                        None
                    };
                    control.configure_files(None, size, size).unwrap();
                    if regrant_before_body {
                        control
                            .configure_files(Some(inbox.clone()), size, size)
                            .unwrap();
                    }
                    if let Some(release) = publication_release {
                        release.send(()).unwrap();
                    }
                    if let Some(release) = body_release {
                        release.send(()).unwrap();
                    }
                };
                let (result, ()) = tokio::join!(fetch, revoke);
                assert!(
                    matches!(result, Err(Error::Denied)),
                    "fetch must fail at the consent guard: {result:?}"
                );
                remote.await.unwrap();
                let destination = inbox.join(format!(
                    "{}-{}.bin",
                    bob.peer().fingerprint(),
                    crate::files::FileInfo {
                        bytes: size,
                        sha256: hash,
                    }
                    .hash_hex()
                ));
                assert!(!destination.exists());
                if at_publication {
                    assert_staging_retained(&destination, &expected_payload).await;
                }
                assert_eq!(control.files_enabled().unwrap(), regrant_before_body);
                control.unregister_room().unwrap();
                handle.shutdown(DEADLINE).await.unwrap();
                drop(running.await.unwrap());
                shutdown(control).await;
            }),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn live_zero_byte_receives_preserve_attempt_budget_across_regrants() {
        tokio::time::timeout(
            DEADLINE * 4,
            tokio::task::LocalSet::new().run_until(async {
                let root = tempfile::tempdir().unwrap();
                let (control, alice, bob, room) = fixture(root.path()).await;
                let (handle, actor, _events) = Actor::new(room);
                let running = tokio::task::spawn_local(actor.run());
                handle.ready(DEADLINE).await.unwrap();
                control.register_room(handle.clone()).await.unwrap();
                let inbox = root.path().join("inbox");
                control.enable_files(inbox.clone(), 0, 0).unwrap();
                let endpoint = crate::transport::client(
                    control.local_addr().unwrap(),
                    &bob,
                    &alice.peer(),
                    ALPN,
                )
                .unwrap();
                let connection = endpoint
                    .connect(control.local_addr().unwrap(), crate::config::TLS_NAME)
                    .unwrap()
                    .await
                    .unwrap();
                for _ in 0..crate::config::NODE_FILE_COUNT_BUDGET {
                    let (mut send, mut recv, hash) =
                        offer(&connection, control.room_id(), &[]).await;
                    resume_barrier(&mut recv).await;
                    send.finish().unwrap();
                    assert_eq!(recv.read_to_end(32).await.unwrap(), hash);
                    control.configure_files(None, 0, 0).unwrap();
                    control.configure_files(Some(inbox.clone()), 0, 0).unwrap();
                }
                // Zero bytes cannot exhaust the byte budget. Exactly the configured
                // number of successful real offers must exhaust the attempt budget.
                let (_send, mut recv, _) = offer(&connection, control.room_id(), &[]).await;
                let mut resume = [0; 40];
                assert!(
                    recv.read_exact(&mut resume).await.is_err(),
                    "regrant reset the zero-byte offer count"
                );
                assert!(control.files_enabled().unwrap());
                connection.close(0u32.into(), b"");
                endpoint.close(0u32.into(), b"");
                control.unregister_room().unwrap();
                handle.shutdown(DEADLINE).await.unwrap();
                drop(running.await.unwrap());
                shutdown(control).await;
            }),
        )
        .await
        .unwrap();
    }
}

fn timing() -> openraft::Config {
    let mut config = network::configuration().unwrap();
    config.heartbeat_interval = 50;
    config.election_timeout_min = 200;
    config.election_timeout_max = 400;
    config
}
async fn wait_leader(node: &ControlNode) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while node.current_leader().await.is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn foreground_service_resumes_exact_public_receipts_without_reinitializing() {
    let directory = tempfile::tempdir().unwrap();
    let alice = Arc::new(Identity::create(&directory.path().join("alice")).unwrap());
    let bob = Identity::create(&directory.path().join("bob")).unwrap();
    let anchor = RootBundle::create(&alice, "Service resume fixture")
        .unwrap()
        .verify(&alice.peer())
        .unwrap();
    let creator = CryptoMember::new(&alice, &anchor, vec![])
        .unwrap()
        .public_identity()
        .unwrap();
    let grant = Grant::issue(&anchor, &[], &alice, &bob.peer(), Rights::PARTICIPANT).unwrap();
    let mut bob_actor = CryptoMember::new(&bob, &anchor, vec![grant]).unwrap();
    let package = bob_actor.key_package().unwrap();
    let id = PeerId::from_peer(&alice.peer());
    let peers = BTreeMap::from([(id, alice.peer())]);
    let path = directory.path().join("control");
    let settings = || Settings {
        anchor: anchor.clone(),
        listen: "127.0.0.1:0".parse().unwrap(),
        peers: peers.clone(),
        timing: timing(),
    };
    let node = ControlNode::start(alice.clone(), &path, settings(), Mode::Create(creator))
        .await
        .unwrap();
    // A request already waiting BEFORE initialization must survive the no-leader
    // interval. This is deterministic; a longer VM timeout alone would not fix it.
    let (fresh, ()) = tokio::join!(node.fresh_catch_up(None, Duration::from_secs(5)), async {
        tokio::time::sleep(Duration::from_millis(120)).await;
        node.initialize(BTreeMap::from([(
            id,
            Node {
                endpoint: Some(node.local_addr().unwrap()),
                ..Default::default()
            },
        )]))
        .await
        .unwrap();
        wait_leader(&node).await;
    });
    let (view, records) = fresh.unwrap();
    assert_eq!(view.epoch(), 0);
    assert!(!records.is_empty());
    let proposal = Proposal::add(
        &alice,
        anchor.id(),
        0,
        anchor.id(),
        b"opaque service fixture",
        &package,
    )
    .unwrap();
    let receipt = node
        .submit_public(proposal.clone(), Duration::from_secs(5))
        .await
        .unwrap();
    assert!(receipt.accepted && !receipt.replayed);
    node.shutdown().await.unwrap();
    let resumed = ControlNode::start(alice, &path, settings(), Mode::Resume)
        .await
        .unwrap();
    wait_leader(&resumed).await;
    assert!(
        resumed
            .initialize(BTreeMap::from([(id, Node::default())]))
            .await
            .is_err()
    );
    let replay = resumed
        .submit_public(proposal, Duration::from_secs(5))
        .await
        .unwrap();
    assert!(replay.accepted && replay.replayed);
    assert_eq!(replay.first_log, receipt.first_log);
    assert_eq!(resumed.public_view().await.unwrap().epoch(), 1);
    resumed.shutdown().await.unwrap();
}

#[tokio::test]
async fn original_author_routes_to_remote_leader_over_actual_mutual_tls() {
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
    let peers: BTreeMap<_, _> = ids
        .iter()
        .copied()
        .zip(identities.iter().map(|identity| identity.peer()))
        .collect();
    let anchor = RootBundle::create(&identities[0], "Service remote fixture")
        .unwrap()
        .verify(&identities[0].peer())
        .unwrap();
    let creator = CryptoMember::new(&identities[0], &anchor, vec![])
        .unwrap()
        .public_identity()
        .unwrap();
    let grant = Grant::issue(
        &anchor,
        &[],
        &identities[0],
        &identities[1].peer(),
        Rights::MODERATOR,
    )
    .unwrap();
    let mut bob = CryptoMember::new(&identities[1], &anchor, vec![grant]).unwrap();
    let package = bob.key_package().unwrap();
    let mut nodes = Vec::new();
    let mut voters = BTreeMap::new();
    for (index, identity) in identities.iter().enumerate() {
        let settings = Settings {
            anchor: anchor.clone(),
            listen: "127.0.0.1:0".parse().unwrap(),
            peers: peers.clone(),
            timing: timing(),
        };
        let node = ControlNode::start(
            identity.clone(),
            &directory.path().join(format!("control-{index}")),
            settings,
            Mode::Create(creator.clone()),
        )
        .await
        .unwrap();
        voters.insert(
            node.id(),
            Node {
                endpoint: Some(node.local_addr().unwrap()),
                ..Default::default()
            },
        );
        nodes.push(node);
    }
    let result = std::panic::AssertUnwindSafe(async {
        nodes[0].initialize(voters).await.unwrap();
        wait_leader(&nodes[0]).await;
        let proposal = Proposal::add(
            &identities[0],
            anchor.id(),
            0,
            anchor.id(),
            b"opaque remote service fixture",
            &package,
        )
        .unwrap();
        assert!(
            nodes[0]
                .submit_public(proposal, Duration::from_secs(5))
                .await
                .unwrap()
                .accepted
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            while nodes[1].public_view().await.unwrap().epoch() != 1
                || nodes[1].current_leader().await.is_none()
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let view = nodes[1].public_view().await.unwrap();
        let removal = Proposal::remove(
            &identities[1],
            anchor.id(),
            1,
            view.head(),
            b"opaque remote removal",
            &identities[0].peer(),
        )
        .unwrap();
        let receipt = nodes[1]
            .submit_public(removal.clone(), Duration::from_secs(5))
            .await
            .unwrap();
        assert!(receipt.accepted);
        let replay = nodes[1]
            .submit_public(removal, Duration::from_secs(5))
            .await
            .unwrap();
        assert!(replay.accepted && replay.replayed);
        assert_eq!(receipt.first_log, replay.first_log);
        // A follower must go through the pinned remote leader and apply its
        // quorum read position before exposing its matching local record/view.
        let (fresh, records) = nodes[1]
            .fresh_catch_up(None, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(fresh.epoch(), 2);
        assert_eq!(records.last().unwrap().epoch(), fresh.epoch());
        let cursor = records.last().unwrap().position().unwrap();
        let (same, pending) = nodes[1]
            .fresh_catch_up(Some(cursor), Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(same.epoch(), 2);
        assert!(pending.iter().all(|record| record.epoch() == 2));
        // Old keys and a cached leader hint do not make an isolated node fresh.
        let leader_id = nodes[0].current_leader().await.unwrap();
        let isolated = nodes
            .iter()
            .position(|node| node.id() == leader_id)
            .unwrap();
        nodes[isolated]
            .network
            .replace_peers(BTreeMap::from([(
                ids[isolated],
                identities[isolated].peer(),
            )]))
            .await
            .unwrap();
        assert!(
            nodes[isolated]
                .fresh_catch_up(Some(cursor), Duration::from_millis(600))
                .await
                .is_err()
        );
        nodes[isolated]
            .network
            .replace_peers(peers.clone())
            .await
            .unwrap();
    })
    .catch_unwind()
    .await;
    for node in nodes {
        node.shutdown().await.unwrap();
    }
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
