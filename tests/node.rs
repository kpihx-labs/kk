#![cfg(target_os = "linux")]
mod support;

use kk::{
    contacts::{Contact, Permissions, Registry},
    node::{self, Node, Options},
    profile,
};
use std::{fs, path::PathBuf, time::Duration};

struct Lab {
    _root: tempfile::TempDir,
    profiles: Vec<PathBuf>,
    addresses: Vec<std::net::SocketAddr>,
    nodes: Vec<Node>,
    handles: Vec<node::Handle>,
    _events: Vec<tokio::sync::mpsc::Receiver<node::Event>>,
}
impl Lab {
    async fn new() -> Self {
        let root = support::private_root();
        let profiles: Vec<_> = (0..3)
            .map(|index| root.path().join(format!("node-{index}")))
            .collect();
        let mut identities: Vec<_> = profiles
            .iter()
            .map(|path| Some(profile::identity(path, true).unwrap()))
            .collect();
        let fingerprints: Vec<_> = identities
            .iter()
            .map(|identity| identity.as_ref().unwrap().peer().fingerprint())
            .collect();
        let certificates: Vec<_> = profiles
            .iter()
            .map(|path| fs::read(path.join("identity/certificate.der")).unwrap())
            .collect();
        for (index, path) in profiles.iter().enumerate() {
            let registry = Registry::open(path.clone()).unwrap();
            for (other, certificate) in certificates.iter().enumerate() {
                if index != other {
                    registry
                        .put(
                            &format!("peer-{other}"),
                            Contact {
                                certificate: certificate.clone(),
                                local_identity: fingerprints[index].clone(),
                                incoming: grants(),
                                outgoing: grants(),
                                endpoints: vec![],
                            },
                        )
                        .unwrap();
                }
            }
        }
        let mut nodes = Vec::new();
        let mut handles = Vec::new();
        let mut events = Vec::new();
        let mut addresses = Vec::new();
        for (index, path) in profiles.iter().enumerate() {
            let (node, handle, receiver) = Node::bind(
                identities[index].take().unwrap(),
                Options {
                    profile: path.clone(),
                    bind: "127.0.0.1:0".parse().unwrap(),
                    inbox: Some(path.join("inbox")),
                    max_file_bytes: 1_048_576,
                    file_budget: 2_097_152,
                },
            )
            .await
            .unwrap();
            addresses.push(node.local_addr().unwrap());
            nodes.push(node);
            handles.push(handle);
            events.push(receiver);
        }
        for (index, path) in profiles.iter().enumerate() {
            let registry = Registry::open(path.clone()).unwrap();
            for (other, address) in addresses.iter().enumerate() {
                if index != other {
                    let mut contact = registry.get(&format!("peer-{other}")).unwrap().unwrap();
                    contact.endpoints = vec![*address];
                    registry.put(&format!("peer-{other}"), contact).unwrap();
                }
            }
        }
        Self {
            _root: root,
            profiles,
            addresses,
            nodes,
            handles,
            _events: events,
        }
    }
    fn start(&mut self) -> Vec<tokio::task::JoinHandle<kk::Result<()>>> {
        std::mem::take(&mut self.nodes)
            .into_iter()
            .map(|node| tokio::spawn(node.run()))
            .collect()
    }
}
fn grants() -> Permissions {
    Permissions {
        text: true,
        files: true,
        ..Default::default()
    }
}

#[tokio::test]
async fn links_are_reused_and_remaining_peers_continue_after_the_first_node_stops() {
    let mut lab = Lab::new().await;
    let jobs = lab.start();
    for value in ["one", "two", "three"] {
        lab.handles[0]
            .send_text("peer-1", value, false, Duration::from_secs(3))
            .await
            .unwrap();
    }
    assert_eq!(
        lab.handles[1].status().await.unwrap().connections_accepted,
        1
    );
    assert_eq!(
        lab.handles[1].status().await.unwrap().received_operations,
        3
    );
    lab.handles[0]
        .send_text("peer-2", "A to C", false, Duration::from_secs(3))
        .await
        .unwrap();
    lab.handles[1]
        .send_text("peer-2", "B to C", false, Duration::from_secs(3))
        .await
        .unwrap();
    jobs[0].abort();
    tokio::task::yield_now().await;
    lab.handles[1]
        .send_text("peer-2", "after A leaves", false, Duration::from_secs(3))
        .await
        .unwrap();
    assert_eq!(
        lab.handles[2].status().await.unwrap().received_operations,
        3
    );
    for job in jobs {
        job.abort();
        let _ = job.await;
    }
}

#[tokio::test]
async fn file_streams_reuse_the_protocol_and_revocation_blocks_cached_links() {
    let mut lab = Lab::new().await;
    let jobs = lab.start();
    let source = lab.profiles[0].join("source");
    fs::write(&source, vec![42; 100_000]).unwrap();
    let receipt = lab.handles[0]
        .send_file("peer-1", &source, false, Duration::from_secs(3))
        .await
        .unwrap();
    assert_eq!(receipt.info.bytes, 100_000);
    let repeated = lab.handles[0]
        .send_file("peer-1", &source, false, Duration::from_secs(3))
        .await
        .unwrap();
    assert_eq!(repeated.resumed_from, 100_000);
    assert_eq!(
        lab.handles[1].status().await.unwrap().connections_accepted,
        1
    );
    assert_eq!(
        lab.handles[1].status().await.unwrap().file_budget_remaining,
        2_097_152 - 100_000
    );
    let registry = Registry::open(lab.profiles[1].clone()).unwrap();
    registry.revoke("peer-0").unwrap();
    assert!(
        lab.handles[0]
            .send_file("peer-1", &source, false, Duration::from_secs(3))
            .await
            .is_err()
    );
    assert_eq!(
        lab.handles[1].status().await.unwrap().received_operations,
        2
    );
    for job in jobs {
        job.abort();
        let _ = job.await;
    }
}

#[tokio::test]
async fn ipc_is_private_and_socket_lifetime_is_owned_by_the_node() {
    use std::os::unix::fs::PermissionsExt;
    let mut lab = Lab::new().await;
    let jobs = lab.start();
    assert_eq!(
        fs::metadata(lab.profiles[0].join("node.sock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let reply = node::ipc::request(
        &lab.profiles[0],
        &node::ipc::Request::Text {
            target: "peer-1".to_owned(),
            text: "synthetic ipc fixture".to_owned(),
            sensitive: true,
            seconds: 3,
        },
        Duration::from_secs(4),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(reply.ok && reply.sensitive && reply.digest.is_none());
    let reply = node::ipc::request(
        &lab.profiles[0],
        &node::ipc::Request::Status,
        Duration::from_secs(3),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(reply.status.unwrap().outgoing_links, 1);
    for handle in &lab.handles {
        handle.shutdown().unwrap();
    }
    for job in jobs {
        job.await.unwrap().unwrap();
    }
    assert!(!lab.profiles[0].join("node.sock").exists());
    assert!(lab.handles[0].status().await.is_err());
    assert!(
        node::ipc::request(
            &lab.profiles[0],
            &node::ipc::Request::Status,
            Duration::from_secs(1)
        )
        .await
        .unwrap()
        .is_none()
    );
    // Quinn releases its socket on the runtime driver's next scheduling turn.
    tokio::task::yield_now().await;
    let _socket = std::net::UdpSocket::bind(lab.addresses[0]).unwrap();
}

#[tokio::test]
async fn a_stalled_cli_output_pipe_does_not_freeze_status_or_cancellation() {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
    };
    let mut lab = Lab::new().await;
    drop(lab.nodes.remove(0));
    tokio::task::yield_now().await;
    let mut child = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&lab.profiles[0])
        .args(["serve", "--bind", &lab.addresses[0].to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut diagnostics = BufReader::new(child.stderr.take().unwrap());
    let mut ready = String::new();
    diagnostics.read_line(&mut ready).unwrap();
    assert!(ready.starts_with("NODE LISTENING "));
    let jobs = lab.start();
    let text = "x".repeat(kk::config::MAX_MESSAGE_BYTES);
    for _ in 0..5 {
        lab.handles[1]
            .send_text("peer-0", &text, false, Duration::from_secs(3))
            .await
            .unwrap();
    }
    // Deliberately do not drain child.stdout. Control must remain usable.
    let status = node::ipc::request(
        &lab.profiles[0],
        &node::ipc::Request::Status,
        Duration::from_secs(2),
    )
    .await
    .unwrap()
    .unwrap()
    .status
    .unwrap();
    assert_eq!(status.received_operations, 5);
    assert!(
        Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let stopped = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    if stopped.is_err() {
        child.kill().unwrap();
        child.wait().unwrap();
    }
    assert_eq!(
        stopped.expect("output worker must cancel").code(),
        Some(130)
    );
    for job in jobs {
        job.abort();
        let _ = job.await;
    }
}

#[tokio::test]
async fn ipc_handles_maximum_escaped_text_and_refuses_oversize_without_delivery() {
    let mut lab = Lab::new().await;
    let jobs = lab.start();
    for (bytes, accepted) in [
        (kk::config::MAX_MESSAGE_BYTES, true),
        (kk::config::MAX_MESSAGE_BYTES + 1, false),
    ] {
        let reply = node::ipc::request(
            &lab.profiles[0],
            &node::ipc::Request::Text {
                target: "peer-1".to_owned(),
                text: "\0".repeat(bytes),
                sensitive: true,
                seconds: 3,
            },
            Duration::from_secs(4),
        )
        .await;
        if accepted {
            let reply = reply.unwrap().unwrap();
            assert!(reply.ok && reply.sensitive && reply.digest.is_none());
        } else {
            assert!(matches!(reply, Err(kk::Error::MessageTooLarge)));
        }
    }
    assert_eq!(
        lab.handles[1].status().await.unwrap().received_operations,
        1
    );
    assert_eq!(lab.handles[0].status().await.unwrap().sent_operations, 1);
    for job in jobs {
        job.abort();
        let _ = job.await;
    }
}
