use super::*;

#[tokio::test]
async fn response_eof_cancels_a_body_blocked_by_receiver_flow_control() {
    response_close_case(false).await;
}

#[tokio::test]
async fn premature_valid_receipt_cannot_complete_a_flow_control_blocked_body() {
    response_close_case(true).await;
}

async fn response_close_case(premature_receipt: bool) {
    let root = tempfile::tempdir().unwrap();
    let sender = Identity::create(&root.path().join("sender")).unwrap();
    let recipient = Identity::create(&root.path().join("recipient")).unwrap();
    let source = root.path().join("source");
    let payload = vec![0x5a; config::WINDOW_BYTES as usize * 4];
    std::fs::write(&source, &payload).unwrap();
    let endpoint = transport::server(
        "127.0.0.1:0".parse().unwrap(),
        &recipient,
        &sender.peer(),
        ALPN,
    )
    .unwrap();
    let address = endpoint.local_addr().unwrap();
    let (release, held) = tokio::sync::oneshot::channel();
    let rejecting_peer = async {
        let connection = transport::accept(&endpoint, &sender.peer()).await.unwrap();
        let (mut response, mut request) = connection.accept_bi().await.unwrap();
        request.read_exact(&mut [0; 41]).await.unwrap();
        response.write_all(&0u64.to_be_bytes()).await.unwrap();
        response.write_all(&Sha256::digest([])).await.unwrap();
        if premature_receipt {
            response.write_all(&Sha256::digest(&payload)).await.unwrap();
        }
        response.finish().unwrap();
        // Deliberately retain, but never drain or stop, the request. Its finite
        // receive window must not strand a sender that already received EOF.
        held.await.unwrap();
        drop(request);
        connection.close(0u32.into(), b"");
    };
    let sending = async {
        let outcome = tokio::time::timeout(
            Duration::from_secs(5),
            send_file(
                address,
                &sender,
                &recipient.peer(),
                &source,
                Duration::from_secs(2),
            ),
        )
        .await;
        release.send(()).unwrap();
        if premature_receipt {
            assert!(
                matches!(outcome, Ok(Err(Error::Timeout))),
                "an early valid receipt cannot substitute for full upload: {outcome:?}"
            );
        } else {
            assert!(
                matches!(outcome, Ok(Err(Error::Integrity))),
                "EOF must reject immediately rather than wait for body credit: {outcome:?}"
            );
        }
    };
    tokio::time::timeout(Duration::from_secs(12), async {
        tokio::join!(rejecting_peer, sending);
    })
    .await
    .unwrap();
    endpoint.close(0u32.into(), b"");
}

#[tokio::test]
async fn tampered_truncated_and_extra_payloads_never_publish_a_file() {
    for mode in ["digest", "truncated", "extra", "race"] {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let a = Identity::create(&root.path().join("a")).unwrap();
        let b = Identity::create(&root.path().join("b")).unwrap();
        let output = root.path().join("output");
        let receiver = FileReceiver::bind(
            "127.0.0.1:0".parse().unwrap(),
            &b,
            &a.peer(),
            ReceivePolicy::accept(output.clone(), 100).unwrap(),
        )
        .unwrap();
        let address = receiver.local_addr().unwrap();
        let endpoint = transport::client(address, &a, &b.peer(), ALPN).unwrap();
        let attack = async {
            let connection = endpoint
                .connect(address, config::TLS_NAME)
                .unwrap()
                .await
                .unwrap();
            let (mut request, mut response) = connection.open_bi().await.unwrap();
            request.write_all(&[0]).await.unwrap();
            request.write_all(&3u64.to_be_bytes()).await.unwrap();
            let digest = if mode == "digest" {
                [0; 32]
            } else {
                Sha256::digest(b"abc").into()
            };
            request.write_all(&digest).await.unwrap();
            response.read_exact(&mut [0; 40]).await.unwrap();
            let data: &[u8] = match mode {
                "truncated" => b"ab",
                "extra" => b"abcd",
                _ => b"abc",
            };
            request.write_all(data).await.unwrap();
            if mode == "race" {
                std::fs::write(&output, b"keep").unwrap();
            }
            request.finish().unwrap();
            let reply = response.read_to_end(32).await;
            assert!(reply.is_err() || reply.unwrap().is_empty());
        };
        let ((), received) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(attack, receiver.receive(Duration::from_secs(2)))
        })
        .await
        .unwrap();
        assert!(received.is_err());
        if mode == "race" {
            assert_eq!(std::fs::read(&output).unwrap(), b"keep");
        } else {
            assert!(!output.exists());
        }
    }
}
