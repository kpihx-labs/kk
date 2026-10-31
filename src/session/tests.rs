use super::*;

#[tokio::test]
async fn authenticated_but_malformed_frames_get_no_receipt() {
    let root = crate::test_support::tempdir().unwrap();
    let a = Identity::create(&root.path().join("a")).unwrap();
    let b = Identity::create(&root.path().join("b")).unwrap();
    let frames = [
        vec![],
        vec![0, 0],
        vec![0, 0, 0, 0, 1],    // Trailing data after an empty message.
        vec![0, 0, 0, 1, 0xff], // Invalid UTF-8.
        vec![0, 0, 0, 2, b'x'], // Truncated payload.
        ((config::MAX_MESSAGE_BYTES + 1) as u32)
            .to_be_bytes()
            .to_vec(),
    ];
    for mut frame in frames {
        frame.insert(0, 0);
        let listener = Listener::bind("127.0.0.1:0".parse().unwrap(), &b, &a.peer()).unwrap();
        let address = listener.local_addr().unwrap();
        let endpoint = client_endpoint(address, &a, &b.peer()).unwrap();
        let attack = async {
            let connection = endpoint
                .connect(address, config::TLS_NAME)
                .unwrap()
                .await
                .unwrap();
            let (mut request, mut response) = connection.open_bi().await.unwrap();
            request.write_all(&frame).await.unwrap();
            request.finish().unwrap();
            let response = response.read_to_end(32).await;
            assert!(response.is_err() || response.unwrap().is_empty());
        };
        let ((), received) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(attack, listener.receive(Duration::from_secs(2)))
        })
        .await
        .expect("bounded malformed-frame exchange");
        assert!(matches!(
            received,
            Err(Error::Protocol | Error::MessageTooLarge)
        ));
    }
}

#[tokio::test]
async fn a_false_receipt_cannot_report_success() {
    let root = crate::test_support::tempdir().unwrap();
    let a = Identity::create(&root.path().join("a")).unwrap();
    let b = Identity::create(&root.path().join("b")).unwrap();
    let listener = Listener::bind("127.0.0.1:0".parse().unwrap(), &b, &a.peer()).unwrap();
    let address = listener.local_addr().unwrap();
    let fake_receiver = async {
        let connection = listener.endpoint.accept().await.unwrap().await.unwrap();
        let (mut response, mut request) = connection.accept_bi().await.unwrap();
        request
            .read_to_end(config::MAX_MESSAGE_BYTES + config::MESSAGE_HEADER_BYTES)
            .await
            .unwrap();
        response.write_all(&[0; 32]).await.unwrap();
        response.finish().unwrap();
        connection.closed().await;
    };
    let peer = b.peer();
    let ((), sent) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(
            fake_receiver,
            send(address, &a, &peer, "hello", Duration::from_secs(2))
        )
    })
    .await
    .expect("bounded false-receipt exchange");
    assert!(matches!(sent, Err(Error::Protocol)));
}

#[tokio::test]
async fn a_silent_authenticated_peer_is_still_subject_to_the_deadline() {
    let root = crate::test_support::tempdir().unwrap();
    let a = Identity::create(&root.path().join("a")).unwrap();
    let b = Identity::create(&root.path().join("b")).unwrap();
    let listener = Listener::bind("127.0.0.1:0".parse().unwrap(), &b, &a.peer()).unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = client_endpoint(address, &a, &b.peer()).unwrap();
    let silent_peer = async {
        let connection = endpoint
            .connect(address, config::TLS_NAME)
            .unwrap()
            .await
            .unwrap();
        connection.closed().await;
    };
    let ((), received) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(silent_peer, listener.receive(Duration::from_millis(100)))
    })
    .await
    .expect("bounded authenticated idle exchange");
    assert!(matches!(received, Err(Error::Timeout)));
}
