//! Real SDK/API workflow with independent private profiles and synthetic peers.
//! Generated test credentials never leave this process or appear in diagnostics.
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request as HttpRequest},
};
use tower::ServiceExt;

#[path = "history_tests.rs"]
mod history_tests;

fn private_test_root() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

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
    async fn action(&self, mut action: Value) -> Value {
        if action["action"] == "settings" && action.get("expected_revision").is_none() {
            action["expected_revision"] = self.state().await["settings"]["revision"].clone();
        }
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
    #[track_caller]
    fn until<'a>(
        &'a self,
        predicate: impl Fn(&Value) -> bool + 'a,
    ) -> impl std::future::Future<Output = Value> + 'a {
        let caller = std::panic::Location::caller();
        async move {
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
                        "UI workflow timed out at {caller}: rooms={} phases={} operations={}",
                        state["rooms"], state["phases"], state["operations"]
                    );
                }
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

#[test]
fn ui_profiles_are_isolated_and_selection_does_not_move_the_running_identity() {
    const CHILD: &str = "KK_UI_PROFILE_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let root = private_test_root();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "ui::tests::ui_profiles_are_isolated_and_selection_does_not_move_the_running_identity", "--nocapture"])
            .env(CHILD, "1")
            .env("XDG_DATA_HOME", root.path())
            .output().unwrap();
        assert!(
            result.status.success(),
            "isolated profile test failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(tokio::task::LocalSet::new().run_until(async {
            let base = crate::profile::data_directory().unwrap();
            let device = Device::new(base.clone(), 12349).await;
            device.action(json!({"action":"init"})).await;
            let original = device.state().await["identity"].clone();
            let created = device
                .action(json!({"action":"profile-create","id":"other","display_name":"Other"}))
                .await;
            let directory = created["directory"].as_str().unwrap();
            assert_ne!(created["fingerprint"], original);
            assert_eq!(
                device
                    .action(json!({"action":"profiles"}))
                    .await
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
            assert_eq!(
                device
                    .action(json!({"action":"profile-select","id":"other"}))
                    .await["restart_required"],
                true
            );
            assert_eq!(device.state().await["identity"], original);
            device
                .action(json!({"action":"profile-select","id":"default"}))
                .await;
            device
                .action(json!({"action":"profile-unregister","id":"other"}))
                .await;
            assert!(std::path::Path::new(directory).exists());
            device
                .action(json!({"action":"profile-register","id":"other","directory":directory}))
                .await;
            device
                .action(json!({"action":"profile-unregister","id":"other"}))
                .await;
            device.close().await;
        }));
}

#[tokio::test(flavor = "current_thread")]
async fn ui_requires_private_session_exact_host_and_same_origin_without_port_leaking_cookies() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let root = private_test_root();
            let device = Device::new(root.path().join("profile"), 12345).await;
            let state = device.state().await;
            assert_eq!(state["media"]["max_tracks"], config::MEDIA_MAX_STREAMS);
            assert_eq!(state["media"]["max_subscriptions"], config::MEDIA_MAX_STREAMS);
            assert_eq!(state["media"]["max_publications"], config::MEDIA_MAX_PUBLICATIONS);
            assert_eq!(state["media"]["source_kinds"], config::MEDIA_SOURCE_KINDS);
            assert_eq!(state["media"]["max_users"], config::MEDIA_MAX_USERS);
            assert_eq!(config::MEDIA_MAX_STREAMS, (config::ROOM_MAX_MEMBERS - 1) * config::MEDIA_SOURCE_KINDS);
            assert_eq!(config::MEDIA_MAX_USERS, config::MEDIA_MAX_STREAMS + config::MEDIA_MAX_PUBLICATIONS);
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
            assert_eq!(device.state().await["settings"]["display_name"], "My profile");
            assert_eq!(device.state().await["settings"]["share_display_name"], false);
            assert_eq!(device.state().await["settings"]["sidebar_mode"], "expanded");
            assert!(!crate::profile::Settings::load(&device.app.0.profile).unwrap().message_notifications);
            device.action(json!({"action":"settings", "message_notifications":true})).await;
            assert!(crate::profile::Settings::load(&device.app.0.profile).unwrap().message_notifications);
            for mode in ["compact", "expanded"] {
                device.action(json!({"action":"settings", "sidebar_mode":mode})).await;
                assert_eq!(device.state().await["settings"]["sidebar_mode"], mode);
                assert_eq!(serde_json::to_value(crate::profile::Settings::load(&device.app.0.profile).unwrap()).unwrap()["sidebar_mode"], mode);
            }
            device.action(json!({"action":"settings", "sidebar_mode":"auto"})).await;
            assert_eq!(device.state().await["settings"]["message_notifications"], true);
            let notification_revision = device.state().await["settings"]["revision"].clone();
            device.action(json!({"action":"settings", "message_notifications":false})).await;
            let notification_bytes = std::fs::read(device.app.0.profile.join("settings.json")).unwrap();
            let stale_notifications = device.request(Method::POST, "/api/action", Some(json!({"action":"settings", "expected_revision":notification_revision, "message_notifications":true})), Some(&device.app.0.origin), true).await;
            assert_eq!(stale_notifications.status(), StatusCode::BAD_REQUEST);
            assert_eq!(std::fs::read(device.app.0.profile.join("settings.json")).unwrap(), notification_bytes);
            device.action(json!({"action":"settings", "share_display_name":true})).await;
            device.action(json!({"action":"settings", "display_name":"Alice"})).await;
            device.action(json!({"action":"settings", "appearance":{"hue":120,"clarity":60,"saturation":30}})).await;
            let settings = device.state().await["settings"].clone();
            assert_eq!(settings["display_name"], "Alice");
            assert_eq!(settings["appearance"]["hue"], 120);
            assert_eq!(settings["share_display_name"], true);
            assert_eq!(settings["sidebar_mode"], "auto");
            let before = std::fs::read(device.app.0.profile.join("settings.json")).unwrap();
            let refused = device.request(Method::POST, "/api/action", Some(json!({"action":"settings", "expected_revision":settings["revision"], "display_name":"Discarded", "sidebar_mode":"invalid"})), Some(&device.app.0.origin), true).await;
            assert!(refused.status().is_client_error());
            assert_eq!(device.state().await["settings"], settings);
            assert_eq!(std::fs::read(device.app.0.profile.join("settings.json")).unwrap(), before);
            device.action(json!({"action":"settings", "share_display_name":false})).await;
            let settings = device.state().await["settings"].clone();
            assert_eq!(settings["display_name"], "Alice");
            assert_eq!(settings["appearance"]["hue"], 120);
            let refused = device.request(Method::POST, "/api/action", Some(json!({"action":"settings", "expected_revision":settings["revision"], "appearance":{"hue":400,"clarity":60,"saturation":30}})), Some(&device.app.0.origin), true).await;
            assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
            assert_eq!(device.state().await["settings"], settings);
            let stale_revision = settings["revision"].clone();
            device.action(json!({"action":"settings", "share_display_name":true})).await;
            device.action(json!({"action":"settings", "share_display_name":false})).await;
            let before = std::fs::read(device.app.0.profile.join("settings.json")).unwrap();
            let refused = device.request(Method::POST, "/api/action", Some(json!({"action":"settings", "expected_revision":stale_revision, "share_display_name":true, "display_name":"Stale dialog"})), Some(&device.app.0.origin), true).await;
            assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
            let body = to_bytes(refused.into_body(), config::UI_JSON_BYTES).await.unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["code"], "SETTINGS_CONFLICT");
            assert_eq!(std::fs::read(device.app.0.profile.join("settings.json")).unwrap(), before);
            assert_eq!(device.state().await["settings"]["share_display_name"], false);
            let refused = device.request(Method::POST, "/api/action", Some(json!({"action":"join", "name":"../bad", "invitation":"{}", "bind":address(), "accept_files":true})), Some(&device.app.0.origin), true).await;
            assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
            let operations = device.state().await["operations"].clone();
            assert_eq!(operations[0]["state"], "failed");
            assert_eq!(operations[0]["invitation_ready"], false);
            let refused = device.request(Method::POST, "/api/action", Some(json!({"action":"join", "name":"valid", "invitation":"not json", "bind":address(), "accept_files":false})), Some(&device.app.0.origin), true).await;
            assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
            let operations = device.state().await["operations"].clone();
            assert_eq!(operations[1]["state"], "failed");
            assert_eq!(operations[1]["kind"], "join");
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
        let root = private_test_root();
        let a = Device::new(root.path().join("a"), 12345).await;
        let b = Device::new(root.path().join("b"), 12346).await;
        a.action(json!({"action": "init"})).await;
        b.action(json!({"action": "init"})).await;
        // Even a configured human name is local-only until separately enabled.
        a.action(json!({"action":"settings","display_name":"Sam"})).await;
        b.action(json!({"action":"settings","display_name":"Sam"})).await;
        assert_ne!(a.state().await["identity"], b.state().await["identity"]);
        a.action(json!({"action": "create", "name": "team", "title": "UI integration room", "bind": address()})).await;
        a.action(json!({"action": "serve", "name": "team", "accept_files": true})).await;
        a.until(|state| state["rooms"][0]["status"]["running"] == true).await;
        let invitation = a.action(json!({"action": "invite", "name": "team", "bind": address(), "role": "participant"})).await["operation"].as_u64().unwrap();
        a.until(|state| state["operations"].as_array().unwrap().iter().any(|operation| operation["id"] == invitation && operation["invitation_ready"] == true)).await;
        let response = a.request(Method::GET, &format!("/api/operations/{invitation}/invitation"), None, None, true).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bundle = Zeroizing::new(String::from_utf8(to_bytes(response.into_body(), 4096).await.unwrap().to_vec()).unwrap());
        let failed = b.action(json!({"action": "join", "name": "invalid/name", "invitation": bundle.as_str(), "bind": address(), "role": "participant", "accept_files": false})).await["operation"].as_u64().unwrap();
        let failure = b.until(|state| state["operations"].as_array().unwrap().iter().any(|op| op["id"] == failed && op["state"] == "failed")).await;
        assert!(failure["operations"].as_array().unwrap().iter().any(|op| op["id"] == failed && op["kind"] == "join"));
        let joined = b.action(json!({"action": "join", "name": "team", "invitation": bundle.as_str(), "bind": address(), "role": "participant", "accept_files": false})).await["operation"].as_u64().unwrap();
        assert_eq!(b.request(Method::GET, &format!("/api/operations/{joined}/invitation"), None, None, true).await.status(), StatusCode::BAD_REQUEST);
        assert!(b.state().await["operations"].as_array().unwrap().iter().filter(|op| op["kind"] == "join").all(|op| op["invitation_ready"] == false));
        let pending_a = a.until(|state| state["operations"].as_array().unwrap().iter().any(|operation| operation["proposal"].is_object())).await;
        let pending_b = b.until(|state| state["operations"].as_array().unwrap().iter().any(|operation| operation["proposal"].is_object())).await;
        assert_eq!(pending_a["operations"].as_array().unwrap().iter().find(|op| op["id"] == invitation).unwrap()["proposal"]["confirmation"], pending_b["operations"].as_array().unwrap().iter().find(|op| op["id"] == joined).unwrap()["proposal"]["confirmation"]);
        a.action(json!({"action": "confirm", "id": invitation, "accept": true})).await;
        b.action(json!({"action": "confirm", "id": joined, "accept": true})).await;
        assert_eq!(a.request(Method::GET, &format!("/api/operations/{invitation}/invitation"), None, None, true).await.status(), StatusCode::BAD_REQUEST);
        b.until(|state| state["rooms"].as_array().unwrap().iter().any(|room| room["status"]["running"] == true)).await;
        let peer_b = b.state().await["identity"].as_str().unwrap().to_owned();
        let before_label = a.state().await;
        let member = before_label["rooms"][0]["presentation"].as_array().unwrap().iter().find(|entry| entry["id"] == peer_b).unwrap();
        assert_eq!(member["source"], "unknown");
        assert_eq!(before_label["rooms"][0]["status"]["profile_names"], json!({}));
        a.action(json!({"action":"member-label","name":"team","member":peer_b,"display_name":"Local friend"})).await;
        let labeled = a.state().await;
        let member = labeled["rooms"][0]["presentation"].as_array().unwrap().iter().find(|entry| entry["id"] == peer_b).unwrap();
        assert_eq!(member["display_name"], "Local friend");
        assert_eq!(member["source"], "local_override");
        let remote_view = b.state().await;
        let peer_a = a.state().await["identity"].as_str().unwrap().to_owned();
        assert_eq!(remote_view["rooms"][0]["presentation"].as_array().unwrap().iter().find(|entry| entry["id"] == peer_a).unwrap()["source"], "unknown");
        assert_eq!(before_label["rooms"][0]["status"]["members"], labeled["rooms"][0]["status"]["members"]);
        let outsider = "ab".repeat(32);
        let refused = a.request(Method::POST, "/api/action", Some(json!({"action":"member-label","name":"team","member":outsider,"display_name":"Not a member"})), Some(&a.app.0.origin), true).await;
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
        let refused = a.request(Method::POST, "/api/action", Some(json!({"action":"member-label","name":"team","member":peer_a,"display_name":"Self alias"})), Some(&a.app.0.origin), true).await;
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
        let refused = a.request(Method::POST, "/api/action", Some(json!({"action":"member-label","name":"team","member":peer_b,"display_name":"bad\nname"})), Some(&a.app.0.origin), true).await;
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
        a.action(json!({"action":"member-label","name":"team","member":peer_b,"display_name":null})).await;
        let cleared = a.state().await;
        assert_eq!(cleared["rooms"][0]["presentation"].as_array().unwrap().iter().find(|entry| entry["id"] == peer_b).unwrap()["source"], "unknown");
        a.action(json!({"action":"settings","share_display_name":true})).await;
        b.action(json!({"action":"settings","share_display_name":true})).await;
        let announced = a.until(|state| state["rooms"][0]["presentation"].as_array().unwrap().iter().any(|entry| entry["id"] == peer_b && entry["source"] == "self_announcement")).await;
        let remote_announced = b.until(|state| state["rooms"][0]["presentation"].as_array().unwrap().iter().any(|entry| entry["id"] == peer_a && entry["source"] == "self_announcement")).await;
        assert_eq!(announced["rooms"][0]["status"]["profile_names"][&peer_b], "Sam");
        for state in [&announced, &remote_announced] {
            let members = state["rooms"][0]["presentation"].as_array().unwrap();
            assert_eq!(members.len(), 2);
            assert!(members.iter().all(|entry| entry["display_name"] == "Sam"));
            assert_ne!(members[0]["id"], members[1]["id"]);
        }
        assert_eq!(before_label["rooms"][0]["status"]["members"], announced["rooms"][0]["status"]["members"]);
        a.action(json!({"action":"member-label","name":"team","member":peer_b,"display_name":"Known locally"})).await;
        b.action(json!({"action":"settings","share_display_name":false})).await;
        let disabled = a.state().await;
        let member = disabled["rooms"][0]["presentation"].as_array().unwrap().iter().find(|entry| entry["id"] == peer_b).unwrap();
        assert_eq!(member["display_name"], "Known locally");
        assert_eq!(member["source"], "local_override");
        assert_eq!(b.state().await["settings"]["share_display_name"], false);
        a.action(json!({"action": "send", "name": "team", "text": "actual UI event", "sensitive": false})).await;
         let original = b.until(|state| state["messages"].as_array().unwrap().iter().any(|message| message["text"] == "actual UI event")).await;
         let reference:crate::rooms::group::StableReference = serde_json::from_value(original["messages"].as_array().unwrap().iter().find(|message|message["text"]=="actual UI event").unwrap()["reference"].clone()).unwrap();
         let target=crate::rooms::conversation::Target::from(&reference);
         b.action(json!({"action":"reply","name":"team","target":target,"text":"actual typed reply","sensitive":false})).await;
         let answered=a.until(|state|state["messages"].as_array().unwrap().iter().any(|message|message["kind"]=="reply")).await;
         let answer=answered["messages"].as_array().unwrap().iter().find(|message|message["kind"]=="reply").unwrap();
         assert_eq!(answer["text"],"actual typed reply");assert_eq!(answer["relation"]["kind"],"reply");assert_eq!(answer["sender"],peer_b);
         b.action(json!({"action":"react","name":"team","target":target,"value":"thanks","added":true,"sensitive":false})).await;
         let reacted=a.until(|state|!state["reactions"].as_array().unwrap().is_empty()).await;
         assert_eq!(reacted["reactions"][0]["members"].as_array().unwrap().len(),1);
         a.action(json!({"action":"react","name":"team","target":target,"value":"thanks","added":false,"sensitive":false})).await;
         assert_eq!(a.state().await["reactions"][0]["members"].as_array().unwrap().len(),1,"another author cannot remove the peer's reaction");
         b.action(json!({"action":"react","name":"team","target":target,"value":"thanks","added":false,"sensitive":false})).await;
         a.until(|state|state["reactions"].as_array().unwrap().is_empty()).await;
         let spoofed=a.request(Method::POST,"/api/action",Some(json!({"action":"react","name":"team","target":target,"value":"thanks","added":true,"author":peer_b})),Some(&a.app.0.origin),true).await;
         assert_eq!(spoofed.status(),StatusCode::UNPROCESSABLE_ENTITY);
        a.action(json!({"action": "send", "name": "team", "text": "synthetic sensitive UI payload", "sensitive": true})).await;
        let received = b.until(|state| state["messages"].as_array().unwrap().iter().any(|message| message["sensitive"] == true)).await;
        assert!(!received.to_string().contains("synthetic sensitive UI payload"));
        let id = received["messages"].as_array().unwrap().iter().find(|message| message["sensitive"] == true).unwrap()["id"].as_u64().unwrap();
         assert_eq!(b.action(json!({"action": "reveal", "id": id})).await["text"], "synthetic sensitive UI payload");
         b.action(json!({"action":"reply","name":"team","target":target,"text":"hidden typed reply body","sensitive":true})).await;
         let hidden=a.until(|state|state["messages"].as_array().unwrap().iter().any(|message|message["kind"]=="reply"&&message["sensitive"]==true)).await;
         assert!(!hidden.to_string().contains("hidden typed reply body"));
         let hidden_message=hidden["messages"].as_array().unwrap().iter().find(|message|message["kind"]=="reply"&&message["sensitive"]==true).unwrap();
         assert!(hidden_message["relation"].is_null());
         let revealed=a.action(json!({"action":"reveal","id":hidden_message["id"]})).await;
         assert_eq!(revealed["text"],"hidden typed reply body");assert_eq!(revealed["relation"]["kind"],"reply");
        let bytes = vec![0xa7; config::FILE_CHUNK_BYTES + 3];
        let before = b.state().await["rooms"][0]["status"].clone();
        assert_eq!(before["files"]["accepted"], false);
        let refused_upload = HttpRequest::builder().method(Method::POST).uri(format!("/api/rooms/team/files?to={peer_b}"))
            .header(header::HOST, &a.app.0.host).header(header::ORIGIN, &a.app.0.origin)
            .header(header::AUTHORIZATION, format!("Bearer {}", a.token.as_str())).header(header::CONTENT_TYPE, "application/octet-stream")
            .body(Body::from(bytes.clone())).unwrap();
        let refused = router(a.app.clone()).oneshot(refused_upload).await.unwrap();
        let refused: Value = serde_json::from_slice(&to_bytes(refused.into_body(), config::UI_JSON_BYTES).await.unwrap()).unwrap();
        assert_eq!(refused["ok"], false);
        assert_eq!(b.action(json!({"action":"file-consent","name":"team","accepted":true})).await["accepted"], true);
        let after = b.state().await["rooms"][0]["status"].clone();
        assert_eq!(after["files"]["accepted"], true);
        for key in ["owner", "epoch", "address", "voters"] { assert_eq!(before[key], after[key]); }
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
        assert_eq!(b.action(json!({"action":"file-consent","name":"team","accepted":false})).await["accepted"], false);
        assert_eq!(b.state().await["rooms"][0]["status"]["files"]["accepted"], false);
        b.action(json!({"action":"file-consent","name":"team","accepted":true})).await;
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
        std::fs::write(a.app.0.profile.join("labels.json"), b"invalid labels").unwrap();
        let corrupted = a.state().await;
        assert!(corrupted["rooms"][0]["presentation_error"].is_string());
        assert!(corrupted["rooms"][0]["presentation"].is_null());
        assert_eq!(corrupted["rooms"][0]["status"]["running"], true);
        let refused = a.request(Method::POST, "/api/action", Some(json!({"action":"member-label","name":"team","member":peer_b,"display_name":"Again"})), Some(&a.app.0.origin), true).await;
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
        assert_eq!(std::fs::read(a.app.0.profile.join("labels.json")).unwrap(), b"invalid labels");
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
