use super::*;
use crate::{
    Identity,
    pairing::{self, PairServer, RoomJoinOptions},
};

async fn sessions() -> (tempfile::TempDir, RoomSession, RoomSession) {
    let directory = tempfile::tempdir().unwrap();
    let a = Identity::create(&directory.path().join("a")).unwrap();
    let b = Identity::create(&directory.path().join("b")).unwrap();
    let server = PairServer::bind_room(
        "127.0.0.1:0".parse().unwrap(),
        Duration::from_secs(20),
        [7; 32],
    )
    .await
    .unwrap();
    let options = RoomJoinOptions {
        room: [7; 32],
        address: server.local_addr().unwrap(),
        expected_id: Some(server.id()),
        expected_fingerprint: Some(a.peer().fingerprint()),
        code: server.code().unwrap().clone(),
        data_port: 45461,
        deadline: Duration::from_secs(20),
    };
    let (server, client) = tokio::join!(
        server.run_room_session(&a, 45460, |_| async { Ok(true) }, |_| Ok(())),
        pairing::join_room_session(options, &b, |_| async { Ok(true) }, |_| Ok(())),
    );
    (directory, server.unwrap(), client.unwrap())
}

#[tokio::test]
async fn stages_roles_and_large_metadata_are_bounded_without_weakening_pake_frames() {
    let (_directory, mut server, mut client) = sessions().await;
    let wait = Duration::from_secs(5);
    assert!(matches!(
        server.request(EnrollmentStage::Authority, b"", wait).await,
        Err(Error::Protocol)
    ));
    assert!(matches!(
        client
            .respond(EnrollmentStage::Authority, wait, |_| async {
                panic!("wrong-role callback")
            })
            .await,
        Err(Error::Protocol)
    ));
    assert!(matches!(
        client.request(EnrollmentStage::KeyPackage, b"", wait).await,
        Err(Error::Protocol)
    ));
    assert!(matches!(
        client
            .request(
                EnrollmentStage::Authority,
                &vec![0; config::ROOM_BOOTSTRAP_AUTHORITY_BYTES + 1],
                wait
            )
            .await,
        Err(Error::Limit { .. })
    ));
    let (sent, received) = tokio::join!(
        server.respond(EnrollmentStage::Authority, wait, |bytes| async move {
            assert_eq!(bytes.as_slice(), b"authority request");
            Ok(vec![42; config::ROOM_BOOTSTRAP_AUTHORITY_BYTES])
        }),
        client.request(EnrollmentStage::Authority, b"authority request", wait),
    );
    sent.unwrap();
    assert_eq!(
        received.unwrap().len(),
        config::ROOM_BOOTSTRAP_AUTHORITY_BYTES
    );
    assert!(matches!(
        client
            .request(EnrollmentStage::Authority, b"repeat", wait)
            .await,
        Err(Error::Protocol)
    ));
    let package = vec![7; config::ROOM_MAX_WIRE_BYTES];
    let (sent, received) = tokio::join!(
        server.respond(EnrollmentStage::KeyPackage, wait, |bytes| async move {
            assert_eq!(bytes.len(), config::ROOM_MAX_WIRE_BYTES);
            Ok(b"transport reply; not an MLS receipt".to_vec())
        }),
        client.request(EnrollmentStage::KeyPackage, &package, wait),
    );
    sent.unwrap();
    assert_eq!(
        received.unwrap().as_slice(),
        b"transport reply; not an MLS receipt"
    );
    assert!(matches!(
        client
            .request(EnrollmentStage::KeyPackage, b"repeat", wait)
            .await,
        Err(Error::Protocol)
    ));
    let (a, b) = tokio::join!(server.close(), client.close());
    a.unwrap();
    b.unwrap();
}

#[tokio::test]
async fn malformed_room_or_stage_never_becomes_bootstrap_authority() {
    for (room, stage) in [
        ([8; 32], EnrollmentStage::Authority),
        ([7; 32], EnrollmentStage::KeyPackage),
    ] {
        let (_directory, server, mut client) = sessions().await;
        let malicious = async {
            let (mut send, mut recv) = server.connection.accept_bi().await.unwrap();
            read(&mut recv, [7; 32], EnrollmentStage::Authority)
                .await
                .unwrap();
            write(&mut send, room, stage, b"incorrect header")
                .await
                .unwrap();
        };
        let ((), response) = tokio::join!(
            malicious,
            client.request(
                EnrollmentStage::Authority,
                b"request",
                Duration::from_secs(2)
            )
        );
        assert!(matches!(response, Err(Error::Protocol)));
        assert!(matches!(
            client
                .request(EnrollmentStage::Authority, b"retry", Duration::from_secs(1))
                .await,
            Err(Error::Protocol)
        ));
    }
}

#[tokio::test]
async fn expiry_and_caller_cancellation_cannot_silently_retry_a_stage() {
    let (_directory, _server, mut client) = sessions().await;
    // The peer is authenticated but deliberately does not process the request.
    assert!(
        tokio::time::timeout(
            Duration::from_millis(20),
            client.request(
                EnrollmentStage::Authority,
                b"request",
                Duration::from_secs(5)
            )
        )
        .await
        .is_err()
    );
    assert!(matches!(
        client
            .request(EnrollmentStage::Authority, b"retry", Duration::from_secs(1))
            .await,
        Err(Error::Protocol)
    ));
    let (_directory, _server, mut client) = sessions().await;
    client.expires = Instant::now();
    assert!(matches!(
        client
            .request(
                EnrollmentStage::Authority,
                b"request",
                Duration::from_secs(1)
            )
            .await,
        Err(Error::Timeout)
    ));
}
