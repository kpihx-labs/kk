//! Real authenticated HTTP -> driver -> IPC -> actor history, disposable keys only.
use super::*;

async fn history(device: &Device, command: Value) -> Value {
    device
        .action(json!({"action":"history", "name":"team", "command":command}))
        .await
}
async fn refused(device: &Device, action: Value) {
    let operation = action["command"]["operation"].clone();
    let response = device
        .request(
            Method::POST,
            "/api/action",
            Some(action),
            Some(&device.app.0.origin),
            true,
        )
        .await;
    assert!(
        response.status().is_client_error(),
        "must refuse {operation}"
    );
    let bytes = to_bytes(response.into_body(), config::UI_JSON_BYTES)
        .await
        .unwrap();
    if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
        assert_eq!(value["ok"], false);
        assert!(value.get("rows").is_none());
    }
}
fn query(text: &str, after: u64, limit: usize) -> Value {
    json!({"operation":"search", "text":text, "after_id":after, "limit":limit})
}
async fn start(device: &Device) {
    device
        .action(json!({"action":"serve", "name":"team"}))
        .await;
    device
        .until(|state| state["rooms"][0]["status"]["running"] == true)
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn ui_history_orphan_recovery_requires_confirmed_exact_public_recipient() {
    tokio::task::LocalSet::new().run_until(async {
        let root = private_test_root();
        let profile = root.path().join("synthetic-orphan");
        let device = Device::new(profile.clone(), 12443).await;
        device.action(json!({"action":"init"})).await;
        device.action(json!({"action":"create", "name":"team", "title":"Orphan fixture", "bind":address()})).await;
        start(&device).await;
        let state = device.state().await;
        let room: crate::rooms::control::raft::PeerId = serde_json::from_value(state["rooms"][0]["id"].clone()).unwrap();
        let owner = crate::Peer::load(&profile.join("identity/certificate.der")).unwrap().fingerprint_bytes();
        device.close().await;
        // Model publication before enrollment via the existing key owner. This
        // synthetic fixture never reads private bytes or opens SQL in HTTP.
        let directory = crate::rooms::local::directory(&profile, "team").unwrap().join("private");
        let anchor = crate::rooms::store::history_key::HistoryKeyAnchor::open(&directory, owner, room.0).unwrap();
        let key = anchor.initialize().unwrap();
        let recipient = key.recipient().to_string();
        drop(key);
        drop(anchor);
        let device = Device::new(profile, 12443).await;
        start(&device).await;
        assert_eq!(history(&device, json!({"operation":"status"})).await, "Off");
        refused(&device, json!({"action":"history", "name":"team", "command":{"operation":"enable", "confirm":true}})).await;
        refused(&device, json!({"action":"history", "name":"team", "command":{"operation":"recover", "confirm":false, "expected_recipient":recipient}})).await;
        let wrong = age::x25519::Identity::generate().to_public().to_string();
        refused(&device, json!({"action":"history", "name":"team", "command":{"operation":"recover", "confirm":true, "expected_recipient":wrong}})).await;
        let recovered = history(&device, json!({"operation":"recover", "confirm":true, "expected_recipient":recipient})).await;
        assert_eq!(recovered["Enrolled"]["recipient"], recipient);
        assert_eq!(recovered["Enrolled"]["unlocked"], true);
        history(&device, json!({"operation":"open"})).await;
        device.close().await;
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn ui_history_actual_two_profiles_restart_paging_literal_search_clear_and_missing_key() {
    tokio::task::LocalSet::new().run_until(async {
        let root = private_test_root();
        let pa = root.path().join("alice");
        let pb = root.path().join("bob");
        let a = Device::new(pa.clone(), 12441).await;
        let b = Device::new(pb.clone(), 12442).await;
        for device in [&a, &b] { device.action(json!({"action":"init"})).await; }
        a.action(json!({"action":"create", "name":"team", "title":"History integration", "bind":address()})).await;
        start(&a).await;
        assert_eq!(history(&a, json!({"operation":"status"})).await, "Off");
        let directory = crate::rooms::local::directory(&pa, "team").unwrap().join("private");
        assert!(!directory.join("history.agekey").exists());
        let invitation = a.action(json!({"action":"invite", "name":"team", "bind":address()})).await["operation"].as_u64().unwrap();
        a.until(|state| state["operations"].as_array().unwrap().iter().any(|op| op["id"] == invitation && op["invitation_ready"] == true)).await;
        let response = a.request(Method::GET, &format!("/api/operations/{invitation}/invitation"), None, None, true).await;
        let bundle = Zeroizing::new(String::from_utf8(to_bytes(response.into_body(), 4096).await.unwrap().to_vec()).unwrap());
        let joined = b.action(json!({"action":"join", "name":"team", "invitation":bundle.as_str(), "bind":address()})).await["operation"].as_u64().unwrap();
        let pending = |state: &Value| state["operations"].as_array().unwrap().iter().any(|op| op["proposal"].is_object());
        let sa = a.until(pending).await;
        let sb = b.until(pending).await;
        assert_eq!(sa["operations"][0]["proposal"]["confirmation"], sb["operations"][0]["proposal"]["confirmation"]);
        a.action(json!({"action":"confirm", "id":invitation, "accept":true})).await;
        b.action(json!({"action":"confirm", "id":joined, "accept":true})).await;
        b.until(|state| state["rooms"][0]["status"]["running"] == true).await;
        assert_eq!(history(&b, json!({"operation":"status"})).await, "Off");
        a.action(json!({"action":"send", "name":"team", "text":"before enrollment"})).await;
        for device in [&a, &b] {
            refused(device, json!({"action":"history", "name":"team", "command":{"operation":"enable", "confirm":false}})).await;
            let summary = history(device, json!({"operation":"enable", "confirm":true})).await;
            assert_eq!(summary["Enrolled"]["enabled"], true);
            assert_eq!(summary["Enrolled"]["unlocked"], true);
        }
        let body = "literal 雪 %_ same body";
        for _ in 0..2 { a.action(json!({"action":"send", "name":"team", "text":body})).await; }
        a.action(json!({"action":"send", "name":"team", "text":"synthetic sensitive exclusion", "sensitive":true})).await;
        let live = b.until(|state| state["messages"].as_array().unwrap().iter().filter(|m| m["text"] == body).count() == 2).await;
        let messages: Vec<_> = live["messages"].as_array().unwrap().iter().filter(|m| m["text"] == body).collect();
        assert!(messages[0]["reference"].is_object());
        assert_ne!(messages[0]["reference"], messages[1]["reference"]);
        assert!(!live.to_string().contains("synthetic sensitive exclusion"));
        for device in [&a, &b] {
            let first = history(device, query("雪 %_", 0, 1)).await;
            assert_eq!(first["rows"].as_array().unwrap().len(), 1);
            assert_eq!(first["has_more"], true);
            let second = history(device, query("雪 %_", first["next_after"].as_u64().unwrap(), 1)).await;
            assert_eq!(second["rows"].as_array().unwrap().len(), 1);
            assert_ne!(first["rows"][0]["reference"], second["rows"][0]["reference"]);
            let all = history(device, query("", 0, 20)).await;
            assert_eq!(all["rows"].as_array().unwrap().len(), 2);
            assert!(!all.to_string().contains("before enrollment"));
            assert!(!all.to_string().contains("synthetic sensitive exclusion"));
            assert!(history(device, query("雪 %__", 0, 20)).await["rows"].as_array().unwrap().is_empty());
        }
        let stored_b = history(&b, query("", 0, 20)).await;
        for message in &messages { assert!(stored_b["rows"].as_array().unwrap().iter().any(|row| row["reference"] == message["reference"])); }
        history(&a, json!({"operation":"disable"})).await;
        a.action(json!({"action":"send", "name":"team", "text":"not captured by Alice"})).await;
        assert!(history(&a, query("not captured", 0, 20)).await["rows"].as_array().unwrap().is_empty());
        a.close().await;
        b.close().await;
        let a = Device::new(pa.clone(), 12441).await;
        let b = Device::new(pb.clone(), 12442).await;
        a.action(json!({"action":"serve", "name":"team"})).await;
        start(&b).await;
        a.until(|state| state["rooms"][0]["status"]["running"] == true).await;
        assert_eq!(history(&a, query("雪 %_", 0, 20)).await["rows"].as_array().unwrap().len(), 2);
        refused(&a, json!({"action":"history", "name":"team", "command":{"operation":"clear", "confirm":false}})).await;
        refused(&a, json!({"action":"history", "name":"team", "command":{"operation":"recover", "expected_recipient":"not a public recipient", "confirm":true}})).await;
        refused(&a, json!({"action":"history", "name":"team", "command":{"operation":"open", "path":"/not-accepted"}})).await;
        let unauthorized = a.request(Method::POST, "/api/action", Some(json!({"action":"history", "name":"team", "command":{"operation":"status"}})), Some(&a.app.0.origin), false).await;
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        let cross_origin = a.request(Method::POST, "/api/action", Some(json!({"action":"history", "name":"team", "command":{"operation":"status"}})), Some("https://other.invalid"), true).await;
        assert_eq!(cross_origin.status(), StatusCode::FORBIDDEN);
        history(&a, json!({"operation":"clear", "confirm":true})).await;
        assert!(history(&a, query("", 0, 20)).await["rows"].as_array().unwrap().is_empty());
        assert!(directory.join("history.agekey").exists());
        a.close().await;
        // Preserve the synthetic original, but deny reads rather than replacing
        // a missing enrolled key. No key contents are read by this test.
        std::fs::rename(directory.join("history.agekey"), root.path().join("held-test-key")).unwrap();
        let a = Device::new(pa, 12441).await;
        start(&a).await;
        assert_eq!(history(&a, json!({"operation":"status"})).await["Enrolled"]["unlocked"], false);
        refused(&a, json!({"action":"history", "name":"team", "command":query("", 0, 20)})).await;
        refused(&a, json!({"action":"history", "name":"team", "command":{"operation":"open"}})).await;
        assert!(!directory.join("history.agekey").exists());
        b.close().await;
        a.close().await;
    }).await;
}
