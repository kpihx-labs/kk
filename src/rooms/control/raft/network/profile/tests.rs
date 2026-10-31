use super::*;

#[derive(Deserialize)]
enum LegacyRequest {
    ReadBarrier,
}

#[tokio::test]
async fn optional_failures_preserve_cached_mtls_link_and_an_independent_live_stream() {
    for conversation in [false, true] {
        for failure in ["legacy", "reset", "malformed", "timeout", "oversized"] {
            let root = crate::test_support::tempdir().unwrap();
            let alice = Arc::new(Identity::create(&root.path().join("alice")).unwrap());
            let bob = Arc::new(Identity::create(&root.path().join("bob")).unwrap());
            let a = PeerId::from_peer(&alice.peer());
            let b = PeerId::from_peer(&bob.peer());
            let peers = BTreeMap::from([(a, alice.peer()), (b, bob.peer())]);
            let (factory, _unused) = Factory::bind(
                [17; 32],
                alice,
                peers.clone(),
                "127.0.0.1:0".parse().unwrap(),
            )
            .unwrap();
            let (remote, server) =
                Factory::bind([17; 32], bob, peers, "127.0.0.1:0".parse().unwrap()).unwrap();
            let client = Client {
                shared: Arc::downgrade(&factory.shared),
                target: b,
                node: Node {
                    endpoint: Some(remote.local_addr().unwrap()),
                    ..Default::default()
                },
            };
            let (release, released) = tokio::sync::oneshot::channel();
            // Raw transport fixture only: not a private-room authorization proof.
            let serving = tokio::spawn(async move {
                let connection = server
                    .shared
                    .endpoint
                    .accept()
                    .await
                    .unwrap()
                    .await
                    .unwrap();
                assert_eq!(
                    PeerId::from_peer(&transport::peer_identity(&connection).unwrap()),
                    a
                );
                let (mut active_send, mut active_recv) = connection.accept_bi().await.unwrap();
                let mut marker = [0];
                active_recv.read_exact(&mut marker).await.unwrap();
                assert_eq!(marker, [42]);
                let (mut send, mut recv) = connection.accept_bi().await.unwrap();
                let envelope: Envelope = read_frame(&mut recv).await.unwrap();
                assert!(if conversation {
                    matches!(envelope.request, Request::ConversationCapabilities)
                } else {
                    matches!(envelope.request, Request::ProfileGet)
                });
                match failure {
                    "legacy" => {
                        // Model the old enum decoder, which cannot recognize the
                        // optional variant and drops only this response stream.
                        assert!(
                            serde_json::from_value::<LegacyRequest>(
                                serde_json::to_value(&envelope.request).unwrap()
                            )
                            .is_err()
                        );
                        drop(send);
                    }
                    "reset" => {
                        send.reset(1u32.into()).unwrap();
                    }
                    "malformed" => {
                        send.write_all(&1u32.to_be_bytes()).await.unwrap();
                        send.write_all(b"{").await.unwrap();
                        send.finish().unwrap();
                    }
                    "oversized" => {
                        // No body sent: rejection must happen before body allocation/read.
                        send.write_all(
                            &((config::ROOM_PROFILE_WIRE_BYTES + 1) as u32).to_be_bytes(),
                        )
                        .await
                        .unwrap();
                    }
                    "timeout" => {}
                    _ => unreachable!(),
                }
                let (mut next_send, mut next_recv) = connection.accept_bi().await.unwrap();
                let next: Envelope = read_frame(&mut next_recv).await.unwrap();
                assert!(matches!(next.request, Request::ReadBarrier));
                send_frame(&mut next_send, &Response::ReadBarrier(Reply::Applied(None)))
                    .await
                    .unwrap();
                next_send.finish().unwrap();
                active_send.write_all(b"still live").await.unwrap();
                active_send.finish().unwrap();
                let _ = released.await;
            });
            let connection = client.connection(&factory.shared).await.unwrap();
            let stable = connection.stable_id();
            let (mut active_send, mut active_recv) = connection.open_bi().await.unwrap();
            active_send.write_all(&[42]).await.unwrap();
            let result: Result<(serde_json::Value, _)> = client
                .exchange_policy(
                    if conversation {
                        Request::ConversationCapabilities
                    } else {
                        Request::ProfileGet
                    },
                    RPCOption::new(Duration::from_millis(100)),
                    ExchangePolicy::OptionalProfile,
                )
                .await;
            assert!(result.is_err(), "{failure}");
            if failure == "oversized" {
                assert!(matches!(
                    result,
                    Err(Error::Limit {
                        max: config::ROOM_PROFILE_WIRE_BYTES,
                        ..
                    })
                ));
            }
            if failure == "timeout" {
                assert!(matches!(result, Err(Error::Timeout)));
            }
            assert_eq!(
                client
                    .connection(&factory.shared)
                    .await
                    .unwrap()
                    .stable_id(),
                stable,
                "{failure}"
            );
            assert!(connection.close_reason().is_none());
            assert!(matches!(
                client
                    .exchange(Request::ReadBarrier, RPCOption::new(Duration::from_secs(2)))
                    .await
                    .unwrap(),
                Response::ReadBarrier(Reply::Applied(None))
            ));
            assert_eq!(active_recv.read_to_end(32).await.unwrap(), b"still live");
            active_send.finish().unwrap();
            release.send(()).unwrap();
            serving.await.unwrap();
        }
    }
}

#[test]
fn metadata_is_strict_versioned_bounded_and_validated() {
    assert!(
        serde_json::from_value::<ProfileMetadata>(serde_json::json!({"version":1,"epoch":1}))
            .is_err()
    );
    for value in [
        serde_json::json!({"version":2,"epoch":1,"name":"Alice"}),
        serde_json::json!({"version":1,"epoch":1,"name":"bad\nname"}),
        serde_json::json!({"version":1,"epoch":1,"name":"x".repeat(65)}),
    ] {
        assert!(
            serde_json::from_value::<ProfileMetadata>(value)
                .unwrap()
                .validate()
                .is_err()
        );
    }
    assert!(
        serde_json::from_value::<ProfileMetadata>(
            serde_json::json!({"version":1,"epoch":1,"name":null,"author":"forged"})
        )
        .is_err()
    );
    let response = ProfileMetadata {
        version: 1,
        epoch: u64::MAX,
        name: Some("界".repeat(64)),
    };
    response.validate().unwrap();
    let mut size = Size(0);
    serde_json::to_writer(&mut size, &response).unwrap();
    assert_eq!(size.0, serde_json::to_vec(&response).unwrap().len());
    assert!(
        serde_json::to_writer(&mut Size(0), &"x".repeat(config::ROOM_PROFILE_WIRE_BYTES)).is_err()
    );
}
