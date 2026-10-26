#![cfg(target_os = "linux")]

use kk::{
    Identity,
    contacts::{Permissions, Registry},
    pairing::{self, JoinOptions, PairServer, RoomJoinOptions, Scope},
};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

#[tokio::test]
async fn real_scoped_pake_and_tls_dispatch_only_room_approval_not_global_contacts() {
    let directory = tempfile::tempdir().unwrap();
    let a = Identity::create(&directory.path().join("a")).unwrap();
    let b = Identity::create(&directory.path().join("b")).unwrap();
    let ra = Registry::open(directory.path().join("profile-a")).unwrap();
    let rb = Registry::open(directory.path().join("profile-b")).unwrap();
    let room = [17; 32];
    let server = PairServer::bind_room(
        "127.0.0.1:0".parse().unwrap(),
        Duration::from_secs(10),
        room,
    )
    .await
    .unwrap();
    let options = RoomJoinOptions {
        room,
        address: server.local_addr().unwrap(),
        expected_id: Some(server.id()),
        expected_fingerprint: Some(a.peer().fingerprint()),
        code: server.code().unwrap().clone(),
        data_port: 45461,
        deadline: Duration::from_secs(10),
    };
    let callbacks = AtomicUsize::new(0);
    let (accepted, joined) = tokio::join!(
        server.run_room(
            &a,
            45460,
            |proposal| async move {
                assert_eq!(proposal.scope, Scope::Room(room));
                assert_eq!(proposal.our_grants, Permissions::default());
                assert_eq!(proposal.their_grants, Permissions::default());
                Ok(true)
            },
            |peer| {
                assert_eq!(peer.room, room);
                assert_eq!(peer.peer.fingerprint(), b.peer().fingerprint());
                assert_eq!(peer.local_identity, a.peer().fingerprint());
                callbacks.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        ),
        pairing::join_room(
            options,
            &b,
            |_| async { Ok(true) },
            |peer| {
                assert_eq!(peer.room, room);
                assert_eq!(peer.peer.fingerprint(), a.peer().fingerprint());
                assert_eq!(peer.local_identity, b.peer().fingerprint());
                callbacks.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        )
    );
    let accepted = accepted.unwrap();
    let joined = joined.unwrap();
    assert_eq!(accepted.confirmation, joined.confirmation);
    assert_eq!(joined.scope, Scope::Room(room));
    assert_eq!(callbacks.load(Ordering::SeqCst), 2);
    assert!(ra.list().unwrap().is_empty() && rb.list().unwrap().is_empty());
    // This proves scoped peer possession/approval, not MLS admission/controller rights.
}

#[tokio::test]
async fn room_a_code_cannot_create_a_room_b_or_contact_approval() {
    let directory = tempfile::tempdir().unwrap();
    let a = Identity::create(&directory.path().join("a")).unwrap();
    let b = Identity::create(&directory.path().join("b")).unwrap();
    for intended in [Scope::Room([2; 32]), Scope::Contact] {
        let server = PairServer::bind_room(
            "127.0.0.1:0".parse().unwrap(),
            Duration::from_secs(5),
            [1; 32],
        )
        .await
        .unwrap();
        let address = server.local_addr().unwrap();
        let id = server.id();
        let code = server.code().unwrap().clone();
        let stores = AtomicUsize::new(0);
        let accept = server.run_room(
            &a,
            45460,
            |_| async { Ok(true) },
            |_| {
                stores.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        );
        let join = async {
            match intended {
                Scope::Contact => {
                    pairing::join(
                        JoinOptions {
                            address,
                            expected_id: Some(id),
                            expected_fingerprint: None,
                            code,
                            grants: Permissions::default(),
                            data_port: 45461,
                            deadline: Duration::from_secs(5),
                        },
                        &b,
                        |_| async { Ok(true) },
                        |_| {
                            stores.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        },
                    )
                    .await
                }
                Scope::Room(room) => {
                    pairing::join_room(
                        RoomJoinOptions {
                            room,
                            address,
                            expected_id: Some(id),
                            expected_fingerprint: None,
                            code,
                            data_port: 45461,
                            deadline: Duration::from_secs(5),
                        },
                        &b,
                        |_| async { Ok(true) },
                        |_| {
                            stores.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        },
                    )
                    .await
                }
            }
        };
        let (a, b) = tokio::join!(accept, join);
        assert!(a.is_err() && b.is_err());
        assert_eq!(stores.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn contact_code_cannot_create_a_room_approval_and_wrong_server_api_fails_closed() {
    let directory = tempfile::tempdir().unwrap();
    let a = Identity::create(&directory.path().join("a")).unwrap();
    let b = Identity::create(&directory.path().join("b")).unwrap();
    let server = PairServer::bind("127.0.0.1:0".parse().unwrap(), Duration::from_secs(5))
        .await
        .unwrap();
    let options = RoomJoinOptions {
        room: [1; 32],
        address: server.local_addr().unwrap(),
        expected_id: Some(server.id()),
        expected_fingerprint: None,
        code: server.code().unwrap().clone(),
        data_port: 45461,
        deadline: Duration::from_secs(5),
    };
    let stores = AtomicUsize::new(0);
    let (a_result, b_result) = tokio::join!(
        server.run(
            &a,
            Permissions::default(),
            45460,
            |_| async { Ok(true) },
            |_| {
                stores.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        ),
        pairing::join_room(
            options,
            &b,
            |_| async { Ok(true) },
            |_| {
                stores.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        )
    );
    assert!(a_result.is_err() && b_result.is_err());
    assert_eq!(stores.load(Ordering::SeqCst), 0);
    let room_server = PairServer::bind_room(
        "127.0.0.1:0".parse().unwrap(),
        Duration::from_secs(5),
        [1; 32],
    )
    .await
    .unwrap();
    assert!(
        room_server
            .run(
                &a,
                Permissions::default(),
                45460,
                |_| async { Ok(true) },
                |_| {
                    stores.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            )
            .await
            .is_err()
    );
    let contact_server = PairServer::bind("127.0.0.1:0".parse().unwrap(), Duration::from_secs(5))
        .await
        .unwrap();
    assert!(
        contact_server
            .run_room(
                &a,
                45460,
                |_| async { Ok(true) },
                |_| {
                    stores.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            )
            .await
            .is_err()
    );
    assert_eq!(stores.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn room_denial_and_scoped_store_failure_never_report_success() {
    let directory = tempfile::tempdir().unwrap();
    let a = Identity::create(&directory.path().join("a")).unwrap();
    let b = Identity::create(&directory.path().join("b")).unwrap();
    for deny in [true, false] {
        let room = [17; 32];
        let server =
            PairServer::bind_room("127.0.0.1:0".parse().unwrap(), Duration::from_secs(5), room)
                .await
                .unwrap();
        let options = RoomJoinOptions {
            room,
            address: server.local_addr().unwrap(),
            expected_id: Some(server.id()),
            expected_fingerprint: None,
            code: server.code().unwrap().clone(),
            data_port: 45461,
            deadline: Duration::from_secs(5),
        };
        let stores = AtomicUsize::new(0);
        let (accepted, joined) = tokio::join!(
            server.run_room(
                &a,
                45460,
                |_| async { Ok(!deny) },
                |_| {
                    stores.fetch_add(1, Ordering::SeqCst);
                    Err(kk::Error::Storage("intentional scoped store failure"))
                }
            ),
            pairing::join_room(
                options,
                &b,
                |_| async { Ok(true) },
                |_| {
                    stores.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            )
        );
        assert!(accepted.is_err() && joined.is_err());
        assert_eq!(stores.load(Ordering::SeqCst), usize::from(!deny));
    }
}
