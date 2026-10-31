#![cfg(target_os = "linux")]
mod support;

use age::secrecy::SecretString;
use kk::{
    Error, Identity,
    contacts::{Action, Permissions, Registry},
    pairing::{self, JoinOptions, PairServer},
};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

#[tokio::test]
async fn real_tcp_pake_and_pinned_tls_proof_precede_contact_storage() {
    let root = support::private_root();
    let a = Identity::create(&root.path().join("a")).unwrap();
    let b = Identity::create(&root.path().join("b")).unwrap();
    let ra = Registry::open(root.path().join("pa")).unwrap();
    let rb = Registry::open(root.path().join("pb")).unwrap();
    let ga = Permissions {
        text: true,
        ..Default::default()
    };
    let gb = Permissions {
        text: true,
        files: true,
        ..Default::default()
    };
    let server = PairServer::bind("127.0.0.1:0".parse().unwrap(), Duration::from_secs(15))
        .await
        .unwrap();
    let options = JoinOptions {
        address: server.local_addr().unwrap(),
        expected_id: Some(server.id()),
        expected_fingerprint: None,
        code: server.code().unwrap().clone(),
        grants: gb,
        data_port: 45454,
        deadline: Duration::from_secs(15),
    };
    let (accepted, joined) = tokio::join!(
        server.run(
            &a,
            ga,
            45454,
            |_| async { Ok(true) },
            |contact| ra.put("bob", contact)
        ),
        pairing::join(
            options,
            &b,
            |_| async { Ok(true) },
            |contact| rb.put("alice", contact)
        )
    );
    assert_eq!(accepted.unwrap().fingerprint, b.peer().fingerprint());
    assert_eq!(joined.unwrap().fingerprint, a.peer().fingerprint());
    ra.resolve("bob", &a, Action::Text, true).unwrap();
    assert!(matches!(
        ra.resolve("bob", &a, Action::Files, true),
        Err(Error::Denied)
    ));
    ra.resolve("bob", &a, Action::Files, false).unwrap();
    rb.resolve("alice", &b, Action::Files, true).unwrap();
}

#[tokio::test]
async fn wrong_code_wrong_invitation_and_denial_never_call_commit() {
    let root = support::private_root();
    let a = Identity::create(&root.path().join("a")).unwrap();
    let b = Identity::create(&root.path().join("b")).unwrap();
    for mode in ["code", "id", "deny"] {
        let calls = AtomicUsize::new(0);
        let server = PairServer::bind("127.0.0.1:0".parse().unwrap(), Duration::from_secs(10))
            .await
            .unwrap();
        let mut id = server.id();
        if mode == "id" {
            id[0] ^= 1;
        }
        let code = if mode == "code" {
            SecretString::from("wrong synthetic code".to_owned())
        } else {
            server.code().unwrap().clone()
        };
        let options = JoinOptions {
            address: server.local_addr().unwrap(),
            expected_id: Some(id),
            expected_fingerprint: None,
            code,
            grants: Permissions::default(),
            data_port: 45454,
            deadline: Duration::from_secs(10),
        };
        let (accepted, joined) = tokio::join!(
            server.run(
                &a,
                Permissions::default(),
                45454,
                |_| async { Ok(true) },
                |_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            ),
            pairing::join(
                options,
                &b,
                |_| async { Ok(mode != "deny") },
                |_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            )
        );
        assert!(accepted.is_err() && joined.is_err(), "{mode}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn store_failure_is_not_reported_as_atomic_success() {
    let root = support::private_root();
    let a = Identity::create(&root.path().join("a")).unwrap();
    let b = Identity::create(&root.path().join("b")).unwrap();
    for failing_server in [true, false] {
        let stored = AtomicUsize::new(0);
        let server = PairServer::bind("127.0.0.1:0".parse().unwrap(), Duration::from_secs(10))
            .await
            .unwrap();
        let options = JoinOptions {
            address: server.local_addr().unwrap(),
            expected_id: Some(server.id()),
            expected_fingerprint: None,
            code: server.code().unwrap().clone(),
            grants: Permissions::default(),
            data_port: 45454,
            deadline: Duration::from_secs(10),
        };
        let (accepted, joined) = tokio::join!(
            server.run(
                &a,
                Permissions::default(),
                45454,
                |_| async { Ok(true) },
                |_| {
                    if failing_server {
                        Err(Error::Destination("synthetic disk failure"))
                    } else {
                        stored.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                }
            ),
            pairing::join(
                options,
                &b,
                |_| async { Ok(true) },
                |_| Err(Error::Destination("synthetic disk failure"))
            )
        );
        assert!(accepted.is_err() && joined.is_err());
        assert_eq!(stored.load(Ordering::SeqCst), usize::from(!failing_server));
    }
}
