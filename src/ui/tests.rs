//! Real SDK/API workflow with independent private profiles and synthetic peers.
//! Generated test credentials never leave this process or appear in diagnostics.
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request as HttpRequest},
};
use tower::ServiceExt;

struct Device {
    app: App,
    token: Zeroizing<String>,
    driver: tokio::task::JoinHandle<Result<()>>,
}
impl Device {
    async fn new(profile: PathBuf, port: u16) -> Self {
        crate::profile::private_directory(&profile, true).unwrap();
        let origin = format!("http://127.0.0.1:{port}");
        let auth = auth::Auth::new(&profile.join("ui"), &origin).unwrap();
        // Exercise the actual one-use bootstrap representation using only
        // ephemeral synthetic test material, never operator credentials.
        let html = Zeroizing::new(std::fs::read_to_string(auth.launch_path().unwrap()).unwrap());
        let bootstrap = html.split("/#").nth(1).unwrap().split('"').next().unwrap();
        let (send, receive) = mpsc::channel(config::UI_REQUEST_QUEUE);
        let (stop, _) = watch::channel(false);
        let app = App(Arc::new(Shared {
            profile,
            auth,
            host: format!("127.0.0.1:{port}"),
            origin,
            requests: send,
            data: Mutex::new(Data::default()),
            stop,
            upload_budget: Arc::new(std::sync::atomic::AtomicU64::new(config::NODE_FILE_BUDGET)),
            upload_slots: Arc::new(tokio::sync::Semaphore::new(config::UI_MAX_TRANSFERS)),
        }));
        let body = Json(json!({"token": bootstrap}))
            .into_response()
            .into_body();
        let request = HttpRequest::builder()
            .method(Method::POST)
            .uri("/api/bootstrap")
            .header(header::HOST, &app.0.host)
            .header(header::ORIGIN, &app.0.origin)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .unwrap();
        let response = router(app.clone()).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        let token = Zeroizing::new(value["session"].as_str().unwrap().to_owned());
        let driver = tokio::task::spawn_local(driver::run(app.clone(), receive));
        Self { app, token, driver }
    }
    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        origin: Option<&str>,
        authorized: bool,
    ) -> Response {
        let mut request = HttpRequest::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, &self.app.0.host);
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        if authorized {
            request = request.header(
                header::AUTHORIZATION,
                format!("Bearer {}", self.token.as_str()),
            );
        }
        let body = if let Some(body) = body {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(&body).unwrap())
        } else {
            Body::empty()
        };
        router(self.app.clone())
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap()
    }
    async fn action(&self, action: Value) -> Value {
        let response = self
            .request(
                Method::POST,
                "/api/action",
                Some(action),
                Some(&self.app.0.origin),
                true,
            )
            .await;
        let status = response.status();
        let body = to_bytes(response.into_body(), config::UI_JSON_BYTES)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::OK, "UI action refused");
        serde_json::from_slice::<Value>(&body).unwrap()["result"].clone()
    }
    async fn state(&self) -> Value {
        let response = self
            .request(Method::GET, "/api/state", None, None, true)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_slice(
            &to_bytes(response.into_body(), config::UI_JSON_BYTES * 2)
                .await
                .unwrap(),
        )
        .unwrap()
    }
    async fn until(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        let result = tokio::time::timeout(config::UI_REQUEST_TIMEOUT, async {
            let mut timer = tokio::time::interval(std::time::Duration::from_millis(25));
            loop {
                timer.tick().await;
                let state = self.state().await;
                if predicate(&state) {
                    return state;
                }
            }
        })
        .await;
        match result {
            Ok(state) => state,
            Err(_) => {
                let state = self.state().await;
                panic!(
                    "UI workflow timed out: rooms={} phases={} operations={}",
                    state["rooms"], state["phases"], state["operations"]
                );
            }
        }
    }
    async fn close(self) {
        self.app.0.stop.send_replace(true);
        tokio::time::timeout(config::UI_REQUEST_TIMEOUT, self.driver)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
fn address() -> String {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string()
}

#[tokio::test(flavor = "current_thread")]
async fn ui_requires_private_session_exact_host_and_same_origin_without_port_leaking_cookies() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let device = Device::new(root.path().join("profile"), 12345).await;
            assert_eq!(
                device
                    .request(Method::GET, "/api/state", None, None, false)
                    .await
                    .status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                device
                    .request(
                        Method::POST,
                        "/api/action",
                        Some(json!({"action": "init"})),
                        Some("https://untrusted.invalid"),
                        true
                    )
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
            assert_eq!(
                device
                    .request(
                        Method::POST,
                        "/api/action",
                        Some(json!({"action": "init"})),
                        None,
                        true
                    )
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
            let bad = HttpRequest::builder()
                .uri("/api/state")
                .header(header::HOST, "rebound.invalid:12345")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", device.token.as_str()),
                )
                .body(Body::empty())
                .unwrap();
            assert_eq!(
                router(device.app.clone())
                    .oneshot(bad)
                    .await
                    .unwrap()
                    .status(),
                StatusCode::BAD_REQUEST
            );
            let response = device
                .request(Method::GET, "/api/state", None, None, true)
                .await;
            assert_eq!(response.status(), StatusCode::OK);
            assert!(!response.headers().contains_key(header::SET_COOKIE));
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            assert!(
                response
                    .headers()
                    .contains_key(header::CONTENT_SECURITY_POLICY)
            );
            device.close().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn ui_http_pairs_independent_devices_owns_foreground_rooms_and_redacts_sensitive_events() {
    tokio::task::LocalSet::new().run_until(async {
        let root = tempfile::tempdir().unwrap();
        let a = Device::new(root.path().join("a"), 12345).await;
        let b = Device::new(root.path().join("b"), 12346).await;
        a.action(json!({"action": "init"})).await;
        b.action(json!({"action": "init"})).await;
        assert_ne!(a.state().await["identity"], b.state().await["identity"]);
        a.action(json!({"action": "create", "name": "team", "title": "UI integration room", "bind": address()})).await;
        a.action(json!({"action": "serve", "name": "team", "accept_files": true})).await;
        a.until(|state| state["rooms"][0]["status"]["running"] == true).await;
        let invitation = a.action(json!({"action": "invite", "name": "team", "bind": address(), "role": "participant"})).await["operation"].as_u64().unwrap();
        a.until(|state| state["operations"].as_array().unwrap().iter().any(|operation| operation["id"] == invitation && operation["invitation_ready"] == true)).await;
        let response = a.request(Method::GET, &format!("/api/operations/{invitation}/invitation"), None, None, true).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bundle = Zeroizing::new(String::from_utf8(to_bytes(response.into_body(), 4096).await.unwrap().to_vec()).unwrap());
        let joined = b.action(json!({"action": "join", "name": "team", "invitation": bundle.as_str(), "bind": address(), "role": "participant", "accept_files": true})).await["operation"].as_u64().unwrap();
        let pending_a = a.until(|state| state["operations"].as_array().unwrap().iter().any(|operation| operation["proposal"].is_object())).await;
        let pending_b = b.until(|state| state["operations"].as_array().unwrap().iter().any(|operation| operation["proposal"].is_object())).await;
        assert_eq!(pending_a["operations"][0]["proposal"]["confirmation"], pending_b["operations"][0]["proposal"]["confirmation"]);
        a.action(json!({"action": "confirm", "id": invitation, "accept": true})).await;
        b.action(json!({"action": "confirm", "id": joined, "accept": true})).await;
        b.until(|state| state["rooms"].as_array().unwrap().iter().any(|room| room["status"]["running"] == true)).await;
        a.action(json!({"action": "send", "name": "team", "text": "actual UI event", "sensitive": false})).await;
        b.until(|state| state["messages"].as_array().unwrap().iter().any(|message| message["text"] == "actual UI event")).await;
        a.action(json!({"action": "send", "name": "team", "text": "synthetic sensitive UI payload", "sensitive": true})).await;
        let received = b.until(|state| state["messages"].as_array().unwrap().iter().any(|message| message["sensitive"] == true)).await;
        assert!(!received.to_string().contains("synthetic sensitive UI payload"));
        let id = received["messages"].as_array().unwrap().iter().find(|message| message["sensitive"] == true).unwrap()["id"].as_u64().unwrap();
        assert_eq!(b.action(json!({"action": "reveal", "id": id})).await["text"], "synthetic sensitive UI payload");
        let peer_b = b.state().await["identity"].as_str().unwrap().to_owned();
        let bytes = vec![0xa7; config::FILE_CHUNK_BYTES + 3];
        let upload = HttpRequest::builder().method(Method::POST).uri(format!("/api/rooms/team/files?to={peer_b}"))
            .header(header::HOST, &a.app.0.host).header(header::ORIGIN, &a.app.0.origin)
            .header(header::AUTHORIZATION, format!("Bearer {}", a.token.as_str())).header(header::CONTENT_TYPE, "application/octet-stream")
            .body(Body::from(bytes.clone())).unwrap();
        let response = router(a.app.clone()).oneshot(upload).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let reply: Value = serde_json::from_slice(&to_bytes(response.into_body(), config::UI_JSON_BYTES).await.unwrap()).unwrap();
        assert_eq!(reply["ok"], true);
        let response = b.request(Method::GET, "/api/rooms/team/files", None, None, true).await;
        let inbox: Value = serde_json::from_slice(&to_bytes(response.into_body(), config::UI_JSON_BYTES).await.unwrap()).unwrap();
        let id = inbox[0]["id"].as_str().unwrap();
        let response = b.request(Method::GET, &format!("/api/rooms/team/files/{id}"), None, None, true).await;
        assert_eq!(to_bytes(response.into_body(), config::DEFAULT_FILE_LIMIT as usize).await.unwrap().as_ref(), bytes);
        let peer_a = a.state().await["identity"].as_str().unwrap().to_owned();
        let digest = reply["result"]["sha256"].as_str().unwrap();
        let offered = b.action(json!({"action":"copies","name":"team","command":{"operation":"share","id":id,"peers":[peer_a]}})).await;
        assert_eq!(offered["ok"], true);
        assert_eq!(offered["result"]["sha256"], digest);
        let hints = a.action(json!({"action":"copies","name":"team","command":{"operation":"locate","sha256":digest,"peers":[peer_b]}})).await;
        assert_eq!(hints["result"]["advertised"][0]["peer"], peer_b);
        let fetched = a.action(json!({"action":"copies","name":"team","command":{"operation":"fetch","sha256":digest,"peer":peer_b}})).await;
        assert_eq!(fetched["ok"], true);
        assert_eq!(fetched["result"]["resumed_from"], 0);
        let local_copy = a.app.0.profile.join("rooms/team/inbox").join(format!("{peer_b}-{digest}.bin"));
        assert_eq!(std::fs::read(&local_copy).unwrap(), bytes);
        let occupied = b.app.0.upload_slots.clone().try_acquire_many_owned(config::UI_MAX_TRANSFERS as u32).unwrap();
        let withdrawn = b.action(json!({"action":"copies","name":"team","command":{"operation":"unshare","sha256":digest}})).await;
        drop(occupied);
        assert_eq!(withdrawn["result"]["withdrawn"], true);
        let hints = a.action(json!({"action":"copies","name":"team","command":{"operation":"locate","sha256":digest,"peers":[peer_b]}})).await;
        assert!(hints["result"]["advertised"].as_array().unwrap().is_empty());
        let unsafe_share = b.request(Method::POST, "/api/action", Some(json!({"action":"copies","name":"team","command":{"operation":"share","id":"../not-an-inbox-object.bin","peers":[peer_a]}})), Some(&b.app.0.origin), true).await;
        assert_ne!(unsafe_share.status(), StatusCode::OK);
        let response = b.request(Method::POST, &format!("/api/rooms/team/files/{id}/forward?to={peer_a}"), None, Some(&b.app.0.origin), true).await;
        assert_eq!(response.status(), StatusCode::OK);
        let forwarded: Value = serde_json::from_slice(&to_bytes(response.into_body(), config::UI_JSON_BYTES).await.unwrap()).unwrap();
        assert_eq!(forwarded["ok"], true);
        {
            use std::os::unix::fs::OpenOptionsExt;
            let directory = a.app.0.profile.join("rooms/team/inbox");
            for index in 0..config::NODE_FILE_COUNT_BUDGET {
                let info = crate::files::FileInfo { bytes:0, sha256:[index as u8;32] };
                std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600)
                    .open(directory.join(format!("{peer_b}-{}.bin",info.hash_hex()))).unwrap();
            }
            let limited = a.request(Method::GET, "/api/rooms/team/files", None, None, true).await;
            assert_eq!(limited.status(), StatusCode::BAD_REQUEST);
            let message = to_bytes(limited.into_body(), config::UI_JSON_BYTES).await.unwrap();
            assert!(String::from_utf8_lossy(&message).contains("stored files are intact"));
            assert_eq!(std::fs::read(&local_copy).unwrap(), bytes);
        }
        a.close().await;
        b.close().await;
        assert!(!root.path().join("a/rooms/team/node.sock").exists());
        assert!(!root.path().join("b/rooms/team/node.sock").exists());
    }).await;
}
