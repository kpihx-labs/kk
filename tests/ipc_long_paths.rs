#![cfg(unix)]
//! Real Unix IPC regressions. Native macOS execution is required separately;
//! these tests do not turn Linux evidence into a macOS qualification claim.

use kk::{
    node::{self, Node, Options},
    profile,
    rooms::local,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

fn long_profile(root: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = root
        .join("absolute profile with spaces")
        .join("étude-房间".repeat(12))
        .join("reboot-alex");
    assert!(path.is_absolute());
    assert!(path.as_os_str().len() > 104);
    path
}

fn options(profile: &Path) -> Options {
    Options {
        profile: profile.to_owned(),
        bind: "127.0.0.1:0".parse().unwrap(),
        inbox: None,
        max_file_bytes: 1024,
        file_budget: 2048,
    }
}

#[tokio::test]
async fn long_absolute_unicode_profile_real_status_and_single_owner_preserve_identity() {
    let root = tempfile::tempdir().unwrap();
    let path = long_profile(root.path());
    let identity = profile::identity(&path, true).unwrap();
    let fingerprint = identity.peer().fingerprint();
    let peer = profile::identity(&root.path().join("synthetic-peer"), true).unwrap();
    kk::contacts::Registry::open(path.clone())
        .unwrap()
        .put(
            "synthetic-peer",
            kk::contacts::Contact {
                certificate: peer.peer().certificate_der().to_vec(),
                local_identity: fingerprint.clone(),
                incoming: Default::default(),
                outgoing: Default::default(),
                endpoints: vec![],
            },
        )
        .unwrap();
    let (node, _handle, _events) = Node::bind(identity, options(&path)).await.unwrap();
    assert!(
        Node::bind(profile::identity(&path, false).unwrap(), options(&path))
            .await
            .is_err()
    );
    let task = tokio::spawn(node.run());
    let reply = node::ipc::request(&path, &node::ipc::Request::Status, Duration::from_secs(5))
        .await
        .unwrap()
        .unwrap();
    assert!(reply.ok && reply.status.is_some());
    task.abort();
    let _ = task.await;
    assert!(
        node::ipc::request(&path, &node::ipc::Request::Status, Duration::from_secs(5))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        profile::identity(&path, false)
            .unwrap()
            .peer()
            .fingerprint(),
        fingerprint
    );
    let (node, _, _) = Node::bind(profile::identity(&path, false).unwrap(), options(&path))
        .await
        .unwrap();
    drop(node);
}

#[tokio::test]
async fn long_room_creation_and_both_session_endpoints_use_shared_transport() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let path = long_profile(root.path());
            let identity = profile::identity(&path, true).unwrap().peer().fingerprint();
            let reservation = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            let address = reservation.local_addr().unwrap();
            drop(reservation);
            local::create(&path, "reboot-study", "Reboot study", address)
                .await
                .unwrap();
            let (ready, started) = tokio::sync::oneshot::channel();
            let (stop, stopped) = tokio::sync::watch::channel(false);
            let (events, _received) = tokio::sync::mpsc::channel(32);
            let session_path = path.clone();
            let session = tokio::task::spawn_local(async move {
                local::serve(
                    &session_path,
                    "reboot-study",
                    local::Options {
                        bind: None,
                        reveal: false,
                        inbox: None,
                        max_bytes: 1024,
                        file_budget: 2048,
                        observer: Some(local::Observer {
                            ready: Some(ready),
                            events,
                            stop: stopped,
                        }),
                    },
                )
                .await
            });
            tokio::time::timeout(Duration::from_secs(15), started)
                .await
                .unwrap()
                .unwrap();
            local::request(&path, "reboot-study", &local::Command::Status)
                .await
                .unwrap();
            stop.send_replace(true);
            tokio::time::timeout(Duration::from_secs(15), session)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(
                profile::identity(&path, false)
                    .unwrap()
                    .peer()
                    .fingerprint(),
                identity
            );
        })
        .await;
}

// A real TCP HTTP client, not an in-process router mock. Credentials below are
// generated exclusively inside this test's temporary synthetic profile.
async fn http(
    address: std::net::SocketAddr,
    route: &str,
    session: Option<&str>,
    body: Option<serde_json::Value>,
) -> serde_json::Value {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let body = body.map(|body| body.to_string()).unwrap_or_default();
    let authorization = session
        .map(|value| format!("Authorization: Bearer {value}\r\n"))
        .unwrap_or_default();
    let request = zeroize::Zeroizing::new(format!(
        "{} {route} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\n{authorization}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        if body.is_empty() { "GET" } else { "POST" },
        body.len()
    ));
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = zeroize::Zeroizing::new(Vec::new());
        stream
            .take(1024 * 1024)
            .read_to_end(&mut response)
            .await
            .unwrap();
        let split = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap();
        assert!(
            response.starts_with(b"HTTP/1.1 200 "),
            "HTTP action refused"
        );
        serde_json::from_slice(&response[split + 4..]).unwrap()
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn real_http_create_room_in_persisted_long_profile() {
    use serde_json::json;
    tokio::task::LocalSet::new().run_until(async {
        let root = tempfile::tempdir().unwrap();
        let path = long_profile(root.path());
        let identity = profile::identity(&path, true).unwrap().peer().fingerprint();
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let ui_path = path.clone();
        let ui = tokio::task::spawn_local(kk::ui::run(ui_path, kk::ui::Options {
            bind: address, open_browser: false, opener: None, opener_args: vec![],
        }));
        let html = tokio::time::timeout(Duration::from_secs(15), async {
            let mut interval = tokio::time::interval(Duration::from_millis(10));
            loop {
                interval.tick().await;
                if let Ok(entries) = std::fs::read_dir(path.join("ui")) {
                    for entry in entries {
                        let entry = entry.unwrap();
                        if entry.file_name().to_string_lossy().starts_with("ui-launch-") {
                            let html = std::fs::read_to_string(entry.path()).unwrap();
                            if html.contains("/#") { return zeroize::Zeroizing::new(html); }
                        }
                    }
                }
            }
        }).await.unwrap();
        let bootstrap = html.split("/#").nth(1).unwrap().split('"').next().unwrap();
        let authorized = http(address, "/api/bootstrap", None, Some(json!({"token":bootstrap}))).await;
        let session = zeroize::Zeroizing::new(authorized["session"].as_str().unwrap().to_owned());
        let reservation = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let room_address = reservation.local_addr().unwrap();
        drop(reservation);
        http(address, "/api/action", Some(&session), Some(json!({
            "action":"create", "name":"reboot-study", "title":"Reboot study", "bind":room_address.to_string(),
        }))).await;
        assert!(path.join("rooms/reboot-study/manifest.json").exists());
        assert_eq!(profile::identity(&path, false).unwrap().peer().fingerprint(), identity);
        http(address, "/api/action", Some(&session), Some(json!({"action":"shutdown"}))).await;
        tokio::time::timeout(Duration::from_secs(15), ui).await.unwrap().unwrap().unwrap();
    }).await;
}
