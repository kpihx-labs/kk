use super::*;
use crate::rooms::{Grant, Rights, RootBundle, mls::CryptoMember};
use futures_util::FutureExt;

fn private_fixture_root() -> tempfile::TempDir {
    crate::test_support::tempdir().unwrap()
}

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
        stage: Option<&std::cell::Cell<&'static str>>,
    ) -> (quinn::SendStream, quinn::RecvStream, [u8; 32]) {
        let trace = |description| {
            if let Some(stage) = stage {
                stage.set(description);
            }
        };
        trace("fresh offer: open QUIC stream");
        let (mut send, mut recv) = connection.open_bi().await.unwrap();
        trace("fresh offer: write control envelope");
        write_frame(
            &mut send,
            serde_json::json!({"room": room, "request": "RoomFile"}),
        )
        .await;
        trace("fresh offer: read accepted response");
        if send.id().index() < 3 {
            eprintln!(
                "paced peer {:?}/{:?}: await Accepted",
                connection.remote_address(),
                send.id()
            );
        }
        assert_eq!(
            read_frame(&mut recv).await,
            serde_json::json!({"RoomFile": {"Accepted": room}})
        );
        let hash: [u8; 32] = Sha256::digest(bytes).into();
        trace("fresh offer: write binary file header");
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

    async fn revoked_body_outcome(
        send: &mut quinn::SendStream,
        recv: &mut quinn::RecvStream,
        payload: &[u8],
        trace: impl Fn(&'static str),
    ) -> std::result::Result<Vec<u8>, quinn::ReadToEndError> {
        let body = async {
            trace("revoked transfer body write");
            let _ = send.write_all(payload).await;
            trace("revoked transfer body write returned");
            let _ = send.finish();
        };
        // A rejecting receiver need not drain the old body to free our credit.
        // Continue attempting it, but observe its terminal response concurrently.
        tokio::select! {
            ack = recv.read_to_end(32) => ack,
            () = body => {
                trace("revoked transfer reaches EOF without ACK");
                recv.read_to_end(32).await
            }
        }
    }

    #[tokio::test]
    async fn revoked_wire_peer_observes_eof_without_remaining_upload_credit() {
        let root = private_fixture_root();
        let alice = Identity::create(&root.path().join("alice")).unwrap();
        let bob = Identity::create(&root.path().join("bob")).unwrap();
        let server =
            crate::transport::server("127.0.0.1:0".parse().unwrap(), &bob, &alice.peer(), ALPN)
                .unwrap();
        let address = server.local_addr().unwrap();
        let client = crate::transport::client(address, &alice, &bob.peer(), ALPN).unwrap();
        let (release, held) = tokio::sync::oneshot::channel();
        let reject = async {
            let connection = crate::transport::accept(&server, &alice.peer())
                .await
                .unwrap();
            let (mut response, request) = connection.accept_bi().await.unwrap();
            response.finish().unwrap();
            // Hold the unread request until the caller has observed EOF, not
            // STOP_SENDING or a freed upload window from dropping this handle.
            held.await.unwrap();
            drop(request);
            connection.close(0u32.into(), b"");
        };
        let attempt = async {
            let connection = client
                .connect(address, crate::config::TLS_NAME)
                .unwrap()
                .await
                .unwrap();
            let (mut send, mut recv) = connection.open_bi().await.unwrap();
            let payload = vec![0x69; crate::config::WINDOW_BYTES as usize * 4];
            let outcome = tokio::time::timeout(
                DEADLINE,
                revoked_body_outcome(&mut send, &mut recv, &payload, |_| {}),
            )
            .await;
            release.send(()).unwrap();
            assert!(
                matches!(outcome, Ok(Ok(ref bytes)) if bytes.is_empty()),
                "terminal response must not wait for upload credit: {outcome:?}"
            );
            connection.close(0u32.into(), b"");
        };
        tokio::time::timeout(DEADLINE * 2, async {
            tokio::join!(reject, attempt);
        })
        .await
        .unwrap();
        server.close(0u32.into(), b"");
        client.close(0u32.into(), b"");
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
        receive_case_policy(
            regrant_before_body,
            at_publication,
            crate::transport::Policy::OneShot,
        )
        .await;
    }

    #[tokio::test]
    async fn room_transport_reuses_connection_after_live_receive_withdrawal_and_regrant() {
        receive_case_policy(true, false, crate::transport::Policy::Room).await;
    }

    async fn receive_case_policy(
        regrant_before_body: bool,
        at_publication: bool,
        policy: crate::transport::Policy,
    ) {
        let stage = std::cell::Cell::new("create fixture");
        let started = std::time::Instant::now();
        tokio::time::timeout(
            DEADLINE * 4,
            tokio::task::LocalSet::new().run_until(async {
                let root = private_fixture_root();
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
                let mut endpoint = crate::transport::client(
                    control.local_addr().unwrap(),
                    &bob,
                    &alice.peer(),
                    ALPN,
                )
                .unwrap();
                endpoint.set_default_client_config(crate::transport::client_config(
                    &bob, &alice.peer(), ALPN, policy,
                ).unwrap());
                stage.set("connect peer");
                let connection = endpoint
                    .connect(control.local_addr().unwrap(), crate::config::TLS_NAME)
                    .unwrap()
                    .await
                    .unwrap();
                let trace = |phase| {
                    stage.set(phase);
                    eprintln!("paced receive {:?}: {phase} at {:?}", connection.remote_address(), started.elapsed());
                };
                stage.set("first offer and resume barrier");
                let (mut send, mut recv, hash) =
                    offer(&connection, control.room_id(), &payload, None).await;
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
                trace("withdraw consent");
                assert!(!control.configure_files(None, size, size * 2).unwrap());
                assert!(!control.files_enabled().unwrap());
                if regrant_before_body {
                    assert!(
                        control
                            .configure_files(Some(inbox.clone()), size, size * 2)
                            .unwrap()
                    );
                }
                trace("withdrawal and optional regrant returned");
                let ack = if let Some(release) = publication_release {
                    // Withdrawal has returned while the old commit worker is
                    // still paused outside its publication gate.
                    release.send(()).unwrap();
                    trace("revoked transfer reaches EOF without ACK");
                    recv.read_to_end(32).await
                } else {
                    revoked_body_outcome(&mut send, &mut recv, &payload, &trace).await
                };
                trace("revoked transfer EOF returned");
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
                // This attempt has reached its terminal no-ACK outcome. Retire
                // both stream handles before the next effect on this connection;
                // shadowing them below would retain them until the scope ends.
                drop(send);
                drop(recv);

                assert_eq!(control.files_enabled().unwrap(), regrant_before_body);
                trace("enable receiving for fresh offer");
                control
                    .configure_files(Some(inbox.clone()), size, size * 2)
                    .unwrap();

                // A distinct new offer succeeds, proving regrant is effective rather
                // than merely leaving the entire receiver permanently disabled.
                let fresh = vec![0x71; payload.len()];
                trace("open fresh offer and await acceptance");
                let (mut send, mut recv, fresh_hash) =
                    offer(&connection, control.room_id(), &fresh, Some(&stage)).await;
                stage.set("fresh offer accepted; await resume prefix");
                resume_barrier(&mut recv).await;
                stage.set("fresh payload write and finish");
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
                drop(send);
                drop(recv);
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
                    offer(&connection, control.room_id(), &vec![0x23; payload.len()], None).await;
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
                let root = private_fixture_root();
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
                let (mut send, mut recv, hash) = offer(&connection, room_id, &payload, None).await;
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
                let root = private_fixture_root();
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
        let started = std::time::Instant::now();
        tokio::time::timeout(
            DEADLINE * 4,
            tokio::task::LocalSet::new().run_until(async {
                let root = private_fixture_root();
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
                    let trace = |phase| eprintln!("paced fetch {address} (regrant={regrant_before_body}, publication={at_publication}): {phase} at {:?}", started.elapsed());
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
                    trace("resume prefix returned; wait body release");
                    ready.send(()).unwrap();
                    released.await.unwrap();
                    trace("body release returned");
                    let ack = revoked_body_outcome(&mut send, &mut recv, &payload, trace).await;
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
    async fn saturated_file_lane_reports_busy_without_overriding_consent_or_audience() {
        tokio::time::timeout(DEADLINE * 4, tokio::task::LocalSet::new().run_until(async {
            let root = private_fixture_root();
            let (control, alice, bob, room) = fixture(root.path()).await;
            let (handle, actor, _events) = Actor::new(room);
            let running = tokio::task::spawn_local(actor.run());
            handle.ready(DEADLINE).await.unwrap();
            control.register_room(handle.clone()).await.unwrap();
            let inbox = root.path().join("inbox");
            let budget = crate::config::NODE_MAX_FILES as u64;
            control.enable_files(inbox.clone(), 1, budget).unwrap();
            let source = root.path().join("source");
            std::fs::write(&source, [42]).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            let info = control.network.share_file(
                &source,
                std::collections::BTreeSet::from([PeerId::from_peer(&bob.peer())]),
            ).await.unwrap();
            let endpoint = crate::transport::client(
                control.local_addr().unwrap(), &bob, &alice.peer(), ALPN,
            ).unwrap();
            let connection = endpoint.connect(control.local_addr().unwrap(), crate::config::TLS_NAME)
                .unwrap().await.unwrap();
            let mut held = Vec::new();
            for index in 0..crate::config::NODE_MAX_FILES {
                let (send, mut recv, hash) = offer(&connection, control.room_id(), &[index as u8], None).await;
                resume_barrier(&mut recv).await;
                held.push((send, recv, hash, index as u8));
            }
            // Both directions share the same bounded lane. Rejections before an
            // offer header must not spend another receive reservation.
            for request in [serde_json::json!("RoomFile"), serde_json::json!({"FileFetch": info.sha256})] {
                let (mut send, mut recv) = connection.open_bi().await.unwrap();
                write_frame(&mut send, serde_json::json!({"room": control.room_id(), "request": request})).await;
                assert_eq!(read_frame(&mut recv).await, serde_json::json!({"RoomFile": "Busy"}));
                assert!(recv.read_to_end(1).await.unwrap().is_empty());
            }
            control.network.unshare_file(info.sha256).unwrap();
            let (mut send, mut recv) = connection.open_bi().await.unwrap();
            write_frame(&mut send, serde_json::json!({"room": control.room_id(), "request": {"FileFetch": info.sha256}})).await;
            assert_eq!(read_frame(&mut recv).await, serde_json::json!({"RoomFile": "Denied"}));
            control.configure_files(None, 1, budget).unwrap();
            let (mut send, mut recv) = connection.open_bi().await.unwrap();
            write_frame(&mut send, serde_json::json!({"room": control.room_id(), "request": "RoomFile"})).await;
            assert_eq!(read_frame(&mut recv).await, serde_json::json!({"RoomFile": "Denied"}));
            control.configure_files(Some(inbox.clone()), 1, budget).unwrap();
            // The old streams stay revoked after the regrant, not merely busy.
            for (mut send, mut recv, _, byte) in held {
                let _ = send.write_all(&[byte]).await;
                let _ = send.finish();
                assert!(recv.read_to_end(32).await.map_or(true, |ack| ack.is_empty()));
            }
            assert_eq!(std::fs::read(&source).unwrap(), [42]);
            assert!(std::fs::read_dir(&inbox).unwrap().all(|entry| {
                !entry.unwrap().file_name().to_string_lossy().ends_with(".bin")
            }));
            connection.close(0u32.into(), b"");
            endpoint.close(0u32.into(), b"");
            control.unregister_room().unwrap();
            handle.shutdown(DEADLINE).await.unwrap();
            drop(running.await.unwrap());
            shutdown(control).await;
        })).await.unwrap();
    }

    #[tokio::test]
    async fn forward_received_retains_the_verified_inode_after_path_replacement_and_withdrawal() {
        forward_received_fixture(false).await;
    }

    #[tokio::test]
    async fn forward_received_after_file_runtime_restart_keeps_reception_off() {
        forward_received_fixture(true).await;
    }

    async fn forward_received_fixture(restart: bool) {
        tokio::time::timeout(
            DEADLINE * 4,
            tokio::task::LocalSet::new().run_until(async {
                let root = private_fixture_root();
                let (control, alice, bob, room) = fixture(root.path()).await;
                let (handle, actor, _events) = Actor::new(room);
                let running = tokio::task::spawn_local(actor.run());
                handle.ready(DEADLINE).await.unwrap();
                control.register_room(handle.clone()).await.unwrap();
                let inbox = root.path().join("inbox");
                control.enable_files(inbox.clone(), 64, 64).unwrap();
                let bytes = b"retained verified copy";
                let digest: [u8; 32] = Sha256::digest(bytes).into();
                let info = crate::files::FileInfo {
                    bytes: bytes.len() as u64,
                    sha256: digest,
                };
                let id = format!("{}-{}.bin", alice.peer().fingerprint(), info.hash_hex());
                let original = inbox.join(&id);
                std::fs::write(&original, bytes).unwrap();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&original, std::fs::Permissions::from_mode(0o600))
                        .unwrap();
                }
                // Stopping incoming reception must neither delete a saved copy nor
                // disable a new, independently authorized outgoing operation.
                control.configure_files(None, 64, 64).unwrap();
                let restarted = if restart {
                    control.network.close();
                    // A new file/transport runtime has no inherited Consent,
                    // receive budget, configured inbox, or active grant.
                    let (factory, server) = crate::rooms::control::raft::network::Factory::bind(
                        control.room_id(),
                        alice.clone(),
                        BTreeMap::from([
                            (PeerId::from_peer(&alice.peer()), alice.peer()),
                            (PeerId::from_peer(&bob.peer()), bob.peer()),
                        ]),
                        "127.0.0.1:0".parse().unwrap(),
                    )
                    .unwrap();
                    factory.register_room(handle.clone()).await.unwrap();
                    assert!(!factory.files_enabled().unwrap());
                    Some((factory, server))
                } else {
                    None
                };
                let forwarder = restarted
                    .as_ref()
                    .map(|(factory, _)| factory)
                    .unwrap_or(&control.network);
                let endpoint = crate::transport::server(
                    "127.0.0.1:0".parse().unwrap(),
                    &bob,
                    &alice.peer(),
                    ALPN,
                )
                .unwrap();
                let node = Node {
                    endpoint: Some(endpoint.local_addr().unwrap()),
                    ..Default::default()
                };
                let target = PeerId::from_peer(&bob.peer());
                if restart {
                    assert!(matches!(
                        forwarder
                            .room_file_received(target, &node, &id, false, DEADLINE)
                            .await,
                        Err(Error::Configuration(_))
                    ));
                    let missing = root.path().join("absent-source-root");
                    assert!(!forwarder.bind_received_root(missing.clone(), 64).unwrap());
                    assert!(
                        !missing.exists(),
                        "read-only binding must not create an inbox"
                    );
                    assert!(forwarder.bind_received_root(inbox.clone(), 64).unwrap());
                    assert!(forwarder.bind_received_root(inbox.clone(), 64).unwrap());
                    assert!(forwarder.bind_received_root(inbox.clone(), 32).is_err());
                    assert!(
                        forwarder
                            .bind_received_root(root.path().to_owned(), 64)
                            .is_err()
                    );
                    assert!(
                        !forwarder.files_enabled().unwrap(),
                        "source binding must not grant reception"
                    );
                }
                let room_id = control.room_id();
                let alice_peer = alice.peer();
                let destination = root.path().join("forwarded.bin");
                let policy = crate::files::ReceivePolicy::accept(destination.clone(), 64).unwrap();
                let retained = root.path().join("retained-original.bin");
                let remote_original = original.clone();
                let remote_retained = retained.clone();
                let remote = tokio::task::spawn_local(async move {
                    let connection = endpoint.accept().await.unwrap().await.unwrap();
                    let (mut send, mut recv) = connection.accept_bi().await.unwrap();
                    assert_eq!(
                        read_frame(&mut recv).await,
                        serde_json::json!({"room": room_id, "request": "RoomFile"})
                    );
                    // Connection creation follows source verification. Replace the
                    // pathname before permitting the real sender to start its body.
                    std::fs::rename(&remote_original, remote_retained).unwrap();
                    std::fs::write(&remote_original, b"unverified replacement").unwrap();
                    write_frame(
                        &mut send,
                        serde_json::json!({"RoomFile": {"Accepted": room_id}}),
                    )
                    .await;
                    let receipt =
                        crate::files::stream::receive(&alice_peer, &policy, recv, send, || Ok(()))
                            .await
                            .unwrap();
                    // Retain the peer until its exact receipt has reached the sender.
                    (endpoint, connection, receipt)
                });
                let receipt = forwarder
                    .room_file_received(target, &node, &id, false, DEADLINE)
                    .await
                    .unwrap();
                let (endpoint, connection, remote_receipt) = remote.await.unwrap();
                assert_eq!(receipt.info.sha256, digest);
                assert_eq!(remote_receipt.info.sha256, digest);
                assert_eq!(std::fs::read(destination).unwrap(), bytes);
                assert_eq!(std::fs::read(retained).unwrap(), bytes);
                assert_eq!(std::fs::read(original).unwrap(), b"unverified replacement");
                assert!(!forwarder.files_enabled().unwrap());
                if restart {
                    forwarder.unregister_room().unwrap();
                    forwarder.close();
                }
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

    #[tokio::test]
    async fn delayed_availability_cannot_outlive_room_registration() {
        tokio::time::timeout(
            DEADLINE * 4,
            tokio::task::LocalSet::new().run_until(async {
                let root = private_fixture_root();
                let (control, alice, bob, room) = fixture(root.path()).await;
                let (handle, actor, _events) = Actor::new(room);
                let running = tokio::task::spawn_local(actor.run());
                handle.ready(DEADLINE).await.unwrap();
                control.register_room(handle.clone()).await.unwrap();
                let endpoint = crate::transport::server(
                    "127.0.0.1:0".parse().unwrap(),
                    &bob,
                    &alice.peer(),
                    ALPN,
                )
                .unwrap();
                let node = Node {
                    endpoint: Some(endpoint.local_addr().unwrap()),
                    ..Default::default()
                };
                let target = PeerId::from_peer(&bob.peer());
                let room_id = control.room_id();
                let (ready, barrier) = tokio::sync::oneshot::channel();
                let (release, released) = tokio::sync::oneshot::channel();
                let remote = tokio::task::spawn_local(async move {
                    let connection = endpoint.accept().await.unwrap().await.unwrap();
                    let (mut send, mut recv) = connection.accept_bi().await.unwrap();
                    let hash = [7; 32];
                    assert_eq!(
                        read_frame(&mut recv).await,
                        serde_json::json!({"room": room_id, "request": {"FileAvailability": hash}})
                    );
                    assert!(recv.read_to_end(1).await.unwrap().is_empty());
                    ready.send(()).unwrap();
                    released.await.unwrap();
                    write_frame(
                        &mut send,
                        serde_json::json!({"FileAvailability": {"bytes": 1, "sha256": hash}}),
                    )
                    .await;
                    send.finish().unwrap();
                    connection.closed().await;
                });
                let query = control
                    .network
                    .file_availability(target, &node, [7; 32], DEADLINE);
                let detach = async {
                    barrier.await.unwrap();
                    control.unregister_room().unwrap();
                    release.send(()).unwrap();
                };
                let (result, ()) = tokio::join!(query, detach);
                assert!(
                    matches!(result, Err(Error::Denied)),
                    "detached room accepted a delayed hint: {result:?}"
                );
                handle.shutdown(DEADLINE).await.unwrap();
                drop(running.await.unwrap());
                shutdown(control).await;
                remote.await.unwrap();
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
                let root = private_fixture_root();
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
                        offer(&connection, control.room_id(), &[], None).await;
                    resume_barrier(&mut recv).await;
                    send.finish().unwrap();
                    assert_eq!(recv.read_to_end(32).await.unwrap(), hash);
                    control.configure_files(None, 0, 0).unwrap();
                    control.configure_files(Some(inbox.clone()), 0, 0).unwrap();
                }
                // Zero bytes cannot exhaust the byte budget. Exactly the configured
                // number of successful real offers must exhaust the attempt budget.
                let (_send, mut recv, _) = offer(&connection, control.room_id(), &[], None).await;
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
    let directory = private_fixture_root();
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
    let directory = private_fixture_root();
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
