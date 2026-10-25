use super::*;

#[tokio::test]
async fn tampered_truncated_and_extra_payloads_never_publish_a_file() {
    for mode in ["digest", "truncated", "extra", "race"] {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let a = Identity::create(&root.path().join("a")).unwrap();
        let b = Identity::create(&root.path().join("b")).unwrap();
        let output = root.path().join("output");
        let receiver = FileReceiver::bind(
            "127.0.0.1:0".parse().unwrap(),
            &b,
            &a.peer(),
            ReceivePolicy::accept(output.clone(), 100).unwrap(),
        )
        .unwrap();
        let address = receiver.local_addr().unwrap();
        let endpoint = transport::client(address, &a, &b.peer(), ALPN).unwrap();
        let attack = async {
            let connection = endpoint
                .connect(address, config::TLS_NAME)
                .unwrap()
                .await
                .unwrap();
            let (mut request, mut response) = connection.open_bi().await.unwrap();
            request.write_all(&[0]).await.unwrap();
            request.write_all(&3u64.to_be_bytes()).await.unwrap();
            let digest = if mode == "digest" {
                [0; 32]
            } else {
                Sha256::digest(b"abc").into()
            };
            request.write_all(&digest).await.unwrap();
            response.read_exact(&mut [0; 40]).await.unwrap();
            let data: &[u8] = match mode {
                "truncated" => b"ab",
                "extra" => b"abcd",
                _ => b"abc",
            };
            request.write_all(data).await.unwrap();
            if mode == "race" {
                std::fs::write(&output, b"keep").unwrap();
            }
            request.finish().unwrap();
            let reply = response.read_to_end(32).await;
            assert!(reply.is_err() || reply.unwrap().is_empty());
        };
        let ((), received) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(attack, receiver.receive(Duration::from_secs(2)))
        })
        .await
        .unwrap();
        assert!(received.is_err());
        if mode == "race" {
            assert_eq!(std::fs::read(&output).unwrap(), b"keep");
        } else {
            assert!(!output.exists());
        }
    }
}
