//! HTTP qualification of owned-copy reads, not peer delivery or browser decoding.
use super::*;
use sha2::{Digest, Sha256};
use std::os::unix::fs::OpenOptionsExt;

async fn reply_json(response: Response) -> Value {
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(
        &to_bytes(response.into_body(), config::UI_JSON_BYTES)
            .await
            .unwrap(),
    )
    .unwrap()
}

async fn completed_job(device: &Device, path: &str) -> Value {
    tokio::time::timeout(config::UI_REQUEST_TIMEOUT, async {
        loop {
            let reply = reply_json(device.request(Method::GET, path, None, None, true).await).await;
            assert_eq!(reply["ok"], true);
            let job = &reply["result"];
            match job["progress"]["phase"].as_str().unwrap() {
                "completed" => {
                    assert!(job["receipt"]["bytes"].is_u64());
                    return job.clone();
                }
                "failed" | "uncertain" | "cancelled" => panic!(
                    "synthetic transfer did not complete: {}",
                    job["progress"]["phase"]
                ),
                _ => tokio::time::sleep(std::time::Duration::from_millis(10)).await,
            }
        }
    })
    .await
    .unwrap()
}

pub(super) async fn qualify_transfer_jobs(
    a: &Device,
    b: &Device,
    stored_id: &str,
    peer_a: &str,
    peer_b: &str,
) {
    // The original B received copy remains forwardable without granting B reception.
    let forward = format!(
        "/api/rooms/team/files/{stored_id}/forward?to={peer_a}&background=true&sensitive=true"
    );
    let started = reply_json(
        b.request(Method::POST, &forward, None, Some(&b.app.0.origin), true)
            .await,
    )
    .await;
    assert_eq!(started["ok"], true);
    let job_id = started["result"]["id"].as_str().unwrap();
    let path = format!("/api/rooms/team/transfers/{job_id}");
    assert_eq!(
        b.request(Method::GET, &path, None, None, false)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        b.request(
            Method::POST,
            &format!("{path}/cancel"),
            None,
            Some("http://untrusted.invalid"),
            true
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let foreign = reply_json(a.request(Method::GET, &path, None, None, true).await).await;
    assert_eq!(
        foreign["ok"], false,
        "another foreground owner must not resolve this job ID"
    );
    let done = completed_job(b, &path).await;
    assert!(
        done["receipt"].get("sha256").is_none(),
        "job snapshots must not disclose sensitive hashes"
    );
    assert_eq!(
        b.state().await["rooms"][0]["status"]["files"]["accepted"],
        false
    );
    let cancelled = reply_json(
        b.request(
            Method::POST,
            &format!("{path}/cancel"),
            None,
            Some(&b.app.0.origin),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(cancelled["result"]["requested"], false);
    assert_eq!(cancelled["result"]["acknowledged"], true);
    assert_eq!(
        cancelled["result"]["job"]["progress"]["phase"], "completed",
        "late cancellation cannot undo a receipt"
    );
    let listed = reply_json(
        b.request(Method::GET, "/api/rooms/team/transfers", None, None, true)
            .await,
    )
    .await;
    assert!(
        listed["result"]
            .as_array()
            .unwrap()
            .iter()
            .any(|job| job["id"] == job_id)
    );

    // Actual asynchronous upload admission owns the FD before its HTTP temporary
    // guard is dropped. A can receive; no reception is silently granted to B.
    let bytes = vec![42_u8; 128 * 1024];
    let request = HttpRequest::builder()
        .method(Method::POST)
        .uri(format!(
            "/api/rooms/team/files?to={peer_a}&background=true&sensitive=true"
        ))
        .header(header::HOST, &b.app.0.host)
        .header(header::ORIGIN, &b.app.0.origin)
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", b.token.as_str()),
        )
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(bytes.clone()))
        .unwrap();
    let started = reply_json(router(b.app.clone()).oneshot(request).await.unwrap()).await;
    assert_eq!(started["ok"], true);
    let upload_id = started["result"]["id"].as_str().unwrap();
    let done = completed_job(b, &format!("/api/rooms/team/transfers/{upload_id}")).await;
    assert_eq!(done["receipt"]["bytes"], bytes.len() as u64);
    assert!(
        std::fs::read_dir(b.app.0.profile.join("ui/uploads"))
            .unwrap()
            .next()
            .is_none()
    );
    let hash: [u8; 32] = Sha256::digest(&bytes).into();
    let info = crate::files::FileInfo {
        bytes: bytes.len() as u64,
        sha256: hash,
    };
    assert_eq!(
        std::fs::read(
            a.app
                .0
                .profile
                .join("rooms/team/inbox")
                .join(format!("{peer_b}-{}.bin", info.hash_hex()))
        )
        .unwrap(),
        bytes
    );
}

fn stored_copy(inbox: &std::path::Path, peer: &str, bytes: &[u8]) -> String {
    use std::io::Write;
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    let info = crate::files::FileInfo {
        bytes: bytes.len() as u64,
        sha256: digest,
    };
    let id = format!("{peer}-{}.bin", info.hash_hex());
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(inbox.join(&id))
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    id
}

#[tokio::test(flavor = "current_thread")]
async fn owned_copy_http_preview_is_explicit_authenticated_and_keeps_verified_inode() {
    tokio::task::LocalSet::new().run_until(async {
        let root = private_test_root();
        let device = Device::new(root.path().join("profile"), 12347).await;
        device.action(json!({"action":"init"})).await;
        device.action(json!({"action":"create","name":"copies","title":"Owned copies","bind":address()})).await;
        let peer = device.state().await["identity"].as_str().unwrap().to_owned();
        let inbox = device.app.0.profile.join("rooms/copies/inbox");
        crate::profile::private_directory(&inbox, true).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let bytes = png.into_inner();
        let id = stored_copy(&inbox, &peer, &bytes);
        let path = format!("/api/rooms/copies/files/{id}");
        let preview = format!("{path}?preview=true");
        assert_eq!(device.request(Method::GET, &preview, None, None, false).await.status(), StatusCode::UNAUTHORIZED);
        let response = device.request(Method::GET, &preview, None, None, true).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
        assert_eq!(response.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(response.headers()[header::CONTENT_DISPOSITION], "attachment");
        assert_eq!(to_bytes(response.into_body(), config::UI_FILE_PREVIEW_BYTES as usize).await.unwrap().as_ref(), bytes);

        // Both response paths retain their already-verified object. Replacing a
        // pathname after authorization must not substitute unverified bytes.
        let download = device.request(Method::GET, &path, None, None, true).await;
        let preview = device.request(Method::GET, &preview, None, None, true).await;
        assert_eq!(download.status(), StatusCode::OK);
        assert_eq!(preview.status(), StatusCode::OK);
        assert_eq!(download.headers()[header::CONTENT_TYPE], "application/octet-stream");
        std::fs::rename(inbox.join(&id), inbox.join("retained-original.bin")).unwrap();
        std::fs::write(inbox.join(&id), b"not the verified image").unwrap();
        assert_eq!(to_bytes(download.into_body(), config::DEFAULT_FILE_LIMIT as usize).await.unwrap().as_ref(), bytes);
        assert_eq!(to_bytes(preview.into_body(), config::UI_FILE_PREVIEW_BYTES as usize).await.unwrap().as_ref(), bytes);
        assert_ne!(device.request(Method::GET, &path, None, None, true).await.status(), StatusCode::OK);

        // A valid content-addressed active document is still downloadable as an
        // attachment, but cannot become an inline raster preview.
        let svg = b"<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>";
        let svg_id = stored_copy(&inbox, &peer, svg);
        let svg_path = format!("/api/rooms/copies/files/{svg_id}");
        assert_ne!(device.request(Method::GET, &format!("{svg_path}?preview=true"), None, None, true).await.status(), StatusCode::OK);
        let response = device.request(Method::GET, &svg_path, None, None, true).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/octet-stream");
        assert_eq!(to_bytes(response.into_body(), 4096).await.unwrap().as_ref(), svg);
        assert!(!device.state().await["rooms"][0]["status"].is_object(), "local reads must not start a foreground room or grant reception");
        device.close().await;
    }).await;
}
