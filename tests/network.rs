#![cfg(unix)]

use std::time::Duration;

use kk::{Error, Identity, Listener, config, send};

fn identities() -> (tempfile::TempDir, Identity, Identity) {
    let root = tempfile::tempdir().unwrap();
    let a = Identity::create(&root.path().join("a")).unwrap();
    let b = Identity::create(&root.path().join("b")).unwrap();
    (root, a, b)
}

#[tokio::test]
async fn mutually_authenticated_messages_include_empty_unicode_and_the_limit() {
    let (_root, a, b) = identities();
    for text in [
        String::new(),
        "hello\nbonjour\n\u{00e9}".into(),
        "x".repeat(config::MAX_MESSAGE_BYTES),
    ] {
        let listener = Listener::bind("127.0.0.1:0".parse().unwrap(), &b, &a.peer()).unwrap();
        let address = listener.local_addr().unwrap();
        let peer = b.peer();
        let (sent, received) = tokio::join!(
            send(address, &a, &peer, &text, config::DEFAULT_TIMEOUT),
            listener.receive(config::DEFAULT_TIMEOUT),
        );
        sent.unwrap();
        assert_eq!(received.unwrap().text(), text);
    }
}

#[tokio::test]
async fn both_sides_reject_the_wrong_identity() {
    let (root, a, b) = identities();
    let stranger = Identity::create(&root.path().join("stranger")).unwrap();
    for (sender, expected_server) in [(&stranger, b.peer()), (&a, stranger.peer())] {
        let listener = Listener::bind("127.0.0.1:0".parse().unwrap(), &b, &a.peer()).unwrap();
        let (sent, received) = tokio::join!(
            send(
                listener.local_addr().unwrap(),
                sender,
                &expected_server,
                "must not be accepted",
                Duration::from_secs(2)
            ),
            listener.receive(Duration::from_secs(2)),
        );
        assert!(matches!(sent, Err(Error::Authentication)), "{sent:?}");
        assert!(received.is_err());
    }
}

#[tokio::test]
async fn oversize_and_invalid_deadlines_fail_before_connecting() {
    let (_root, a, b) = identities();
    let address = "127.0.0.1:1".parse().unwrap();
    let text = "x".repeat(config::MAX_MESSAGE_BYTES + 1);
    assert!(matches!(
        send(address, &a, &b.peer(), &text, config::DEFAULT_TIMEOUT).await,
        Err(Error::MessageTooLarge)
    ));
    for deadline in [
        Duration::ZERO,
        config::MAX_FILE_TIMEOUT + Duration::from_secs(1),
    ] {
        assert!(matches!(
            send(address, &a, &b.peer(), "", deadline).await,
            Err(Error::Configuration(_))
        ));
    }
}

#[tokio::test]
async fn listener_deadline_is_bounded_and_releases_its_socket() {
    let (_root, a, b) = identities();
    let listener = Listener::bind("127.0.0.1:0".parse().unwrap(), &b, &a.peer()).unwrap();
    let address = listener.local_addr().unwrap();
    assert!(matches!(
        listener.receive(Duration::from_millis(20)).await,
        Err(Error::Timeout)
    ));
    tokio::task::yield_now().await;
    let _rebound = std::net::UdpSocket::bind(address).unwrap();
}

#[tokio::test]
async fn dropping_a_receive_future_cancels_and_releases_its_socket() {
    let (_root, a, b) = identities();
    let listener = Listener::bind("127.0.0.1:0".parse().unwrap(), &b, &a.peer()).unwrap();
    let address = listener.local_addr().unwrap();
    let result = tokio::time::timeout(
        Duration::from_millis(20),
        listener.receive(config::DEFAULT_TIMEOUT),
    )
    .await;
    assert!(result.is_err());
    tokio::task::yield_now().await;
    let _rebound = std::net::UdpSocket::bind(address).unwrap();
}

#[tokio::test]
async fn ipv6_peers_exchange_without_dns() {
    let (_root, a, b) = identities();
    let listener = Listener::bind("[::1]:0".parse().unwrap(), &b, &a.peer()).unwrap();
    let peer = b.peer();
    let (sent, received) = tokio::join!(
        send(
            listener.local_addr().unwrap(),
            &a,
            &peer,
            "IPv6",
            config::DEFAULT_TIMEOUT
        ),
        listener.receive(config::DEFAULT_TIMEOUT),
    );
    sent.unwrap();
    assert_eq!(received.unwrap().text(), "IPv6");
}

#[tokio::test]
async fn a_silent_udp_peer_cannot_block_a_sender_forever() {
    let (_root, a, b) = identities();
    let silent = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    assert!(matches!(
        send(
            silent.local_addr().unwrap(),
            &a,
            &b.peer(),
            "hello",
            Duration::from_millis(20)
        )
        .await,
        Err(Error::Timeout)
    ));
}
