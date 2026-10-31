#![cfg(unix)]
//! Actual CLI forwarding over loopback, including normal foreground cold startup.
mod support;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader, Read},
    net::UdpSocket,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(110);
const OUTPUT_BYTES: usize = 256 * 1024;

struct Line {
    stdout: bool,
    text: String,
}

/// All children use generated profiles. Pipe readers have bounded output and
/// never block on a full queue; every child and reader is reaped, also on panic.
struct Process {
    child: Child,
    lines: mpsc::Receiver<Line>,
    readers: Vec<JoinHandle<()>>,
    overflow: Arc<AtomicBool>,
}

impl Process {
    fn start(profile: &Path, arguments: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_kk"))
            .arg("--profile")
            .arg(profile)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (sender, lines) = mpsc::sync_channel(128);
        let overflow = Arc::new(AtomicBool::new(false));
        let mut readers = Vec::new();
        for (stdout, input) in [
            (
                true,
                Box::new(child.stdout.take().unwrap()) as Box<dyn Read + Send>,
            ),
            (
                false,
                Box::new(child.stderr.take().unwrap()) as Box<dyn Read + Send>,
            ),
        ] {
            let sender = sender.clone();
            let overflow = overflow.clone();
            readers.push(std::thread::spawn(move || {
                let mut bytes = 0;
                for line in BufReader::new(input.take(OUTPUT_BYTES as u64 + 1)).lines() {
                    let Ok(text) = line else {
                        overflow.store(true, Ordering::SeqCst);
                        break;
                    };
                    bytes += text.len() + 1;
                    if bytes > OUTPUT_BYTES || sender.try_send(Line { stdout, text }).is_err() {
                        overflow.store(true, Ordering::SeqCst);
                        break;
                    }
                }
            }));
        }
        Self {
            child,
            lines,
            readers,
            overflow,
        }
    }

    fn until(&mut self, prefix: &str) {
        let expires = Instant::now() + WAIT;
        loop {
            assert!(
                !self.overflow.load(Ordering::SeqCst),
                "CLI fixture output exceeded its bound"
            );
            let line = self
                .lines
                .recv_timeout(expires.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|error| panic!("missing CLI phase {prefix:?}: {error}"));
            if line.text.starts_with(prefix) {
                return;
            }
        }
    }

    fn finish(&mut self, timeout: Duration) -> (ExitStatus, String, String) {
        let expires = Instant::now() + timeout;
        let mut lines = Vec::new();
        let mut captured = 0;
        let status = loop {
            while let Ok(line) = self.lines.try_recv() {
                captured += line.text.len();
                assert!(captured <= OUTPUT_BYTES, "CLI response exceeded its bound");
                lines.push(line);
            }
            assert!(
                !self.overflow.load(Ordering::SeqCst),
                "CLI fixture output exceeded its bound"
            );
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < expires,
                "CLI process exceeded its deadline"
            );
            let wait = expires
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(20));
            match self.lines.recv_timeout(wait) {
                Ok(line) => {
                    captured += line.text.len();
                    assert!(captured <= OUTPUT_BYTES, "CLI response exceeded its bound");
                    lines.push(line);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(mpsc::RecvTimeoutError::Disconnected) => std::thread::sleep(wait),
            }
        };
        for reader in self.readers.drain(..) {
            reader.join().unwrap();
        }
        for line in self.lines.try_iter() {
            captured += line.text.len();
            assert!(captured <= OUTPUT_BYTES, "CLI response exceeded its bound");
            lines.push(line);
        }
        assert!(
            !self.overflow.load(Ordering::SeqCst),
            "CLI fixture output exceeded its bound"
        );
        let stdout = lines
            .iter()
            .filter(|line| line.stdout)
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let stderr = lines
            .iter()
            .filter(|line| !line.stdout)
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        (status, stdout, stderr)
    }

    fn finish_successfully(&mut self) {
        let (status, _, stderr) = self.finish(WAIT);
        assert!(
            status.success(),
            "CLI foreground failed ({status}): {stderr}"
        );
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}

fn call(profile: &Path, arguments: &[&str], timeout: Duration) -> String {
    let (status, stdout, stderr) = Process::start(profile, arguments).finish(timeout);
    assert!(status.success(), "CLI request failed ({status}): {stderr}");
    stdout
}

fn json(profile: &Path, arguments: &[&str]) -> Value {
    let value: Value = serde_json::from_str(&call(profile, arguments, WAIT)).unwrap();
    assert_eq!(value["ok"], true, "CLI returned a non-success reply");
    value["result"].clone()
}

fn address() -> String {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string()
}

fn digest(bytes: &[u8]) -> String {
    let sha256 = Sha256::digest(bytes).into();
    kk::files::FileInfo {
        bytes: bytes.len() as u64,
        sha256,
    }
    .hash_hex()
}

fn received(directory: &Path) -> Vec<PathBuf> {
    let mut entries = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "bin"))
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn assert_receiving_off(profile: &Path) {
    assert_eq!(
        json(profile, &["room", "status", "team"])["files"]["accepted"],
        false
    );
}

fn completed_job(profile: &Path, id: &str, forbidden: &[&str]) -> Value {
    terminal_job(profile, id, forbidden, "completed")
}

fn terminal_job(profile: &Path, id: &str, forbidden: &[&str], expected: &str) -> Value {
    let expires = Instant::now() + Duration::from_secs(30);
    for _ in 0..256 {
        let remaining = expires.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "forwarding job did not reach a terminal state"
        );
        let reply: Value =
            serde_json::from_str(&call(profile, &["room", "transfer", "team", id], remaining))
                .unwrap();
        assert_eq!(reply["ok"], true);
        let job = &reply["result"];
        assert_eq!(job["id"], id);
        assert_job_privacy(job, forbidden);
        let progress = &job["progress"];
        let local = progress["source_bytes"].as_u64().unwrap();
        let resumed = progress["resumed_from"].as_u64().unwrap();
        if let Some(total) = progress["total_bytes"].as_u64() {
            assert!(resumed <= total && local <= total - resumed);
        }
        let phase = progress["phase"].as_str().unwrap();
        if matches!(phase, "completed" | "failed" | "cancelled" | "uncertain") {
            assert_eq!(phase, expected, "unexpected forwarding outcome");
            assert_eq!(job["terminal_durable"], true);
            if phase != "completed" {
                assert!(job["receipt"].is_null());
                return job.clone();
            }
        }
        if phase == "completed" {
            assert!(
                !job["receipt"].is_null(),
                "Completed requires an actual receipt summary"
            );
            return job.clone();
        }
        assert!(
            job["receipt"].is_null(),
            "non-completed counters must not become a receipt"
        );
        std::thread::sleep(
            expires
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100)),
        );
    }
    panic!("forwarding job exceeded its bounded observation count");
}

fn assert_job_privacy(job: &Value, forbidden: &[&str]) {
    let encoded = serde_json::to_string(job).unwrap();
    for value in forbidden {
        assert!(
            !encoded.contains(value),
            "job snapshot disclosed private source metadata"
        );
    }
    for object in [job, &job["progress"], &job["receipt"]] {
        for key in ["source", "path", "sha256", "hash", "origin"] {
            assert!(object.get(key).is_none(), "job snapshot disclosed {key}");
        }
    }
}

#[test]
fn forward_copy_sync_and_background_after_real_receive_and_restart_keep_receiving_off() {
    let root = support::private_root();
    let alice = root.path().join("alice");
    let bob = root.path().join("bob");
    for profile in [&alice, &bob] {
        call(profile, &["init"], WAIT);
    }
    call(
        &alice,
        &["room", "create", "team", "--bind", &address()],
        WAIT,
    );
    let alice_inbox = alice.join("rooms/team/inbox");
    let bob_inbox = bob.join("rooms/team/inbox");
    let mut sender = Process::start(
        &alice,
        &[
            "room",
            "serve",
            "team",
            "--accept-files",
            "--inbox",
            alice_inbox.to_str().unwrap(),
            "--max-bytes",
            "1048576",
            "--file-budget",
            "2097152",
        ],
    );
    sender.until("ROOM SERVING");
    let invitation = alice.join("invitation.json");
    let mut invite = Process::start(
        &alice,
        &[
            "room",
            "invite",
            "team",
            "--bind",
            "127.0.0.1:0",
            "--invite-out",
            invitation.to_str().unwrap(),
            "--role",
            "participant",
            "--confirm",
        ],
    );
    invite.until("ROOM INVITATION");
    let mut receiver = Process::start(
        &bob,
        &[
            "room",
            "join",
            "team",
            "--invitation",
            invitation.to_str().unwrap(),
            "--bind",
            &address(),
            "--role",
            "participant",
            "--confirm",
            "--accept-files",
            "--inbox",
            bob_inbox.to_str().unwrap(),
            "--max-bytes",
            "1048576",
            "--file-budget",
            "2097152",
        ],
    );
    receiver.until("ROOM SERVING");
    invite.finish_successfully();
    let alice_id = json(&alice, &["room", "status", "team"])["owner"]
        .as_str()
        .unwrap()
        .to_owned();
    let bob_id = json(&bob, &["room", "status", "team"])["owner"]
        .as_str()
        .unwrap()
        .to_owned();
    let payloads = [vec![0xa7; 196_608], vec![0x5b; 131_072]];
    let hashes = payloads
        .iter()
        .map(|bytes| digest(bytes))
        .collect::<Vec<_>>();
    for (index, bytes) in payloads.iter().enumerate() {
        let selected = alice.join(format!("selected-{index}.bin"));
        std::fs::write(&selected, bytes).unwrap();
        let receipt = json(
            &alice,
            &[
                "room",
                "send",
                "team",
                selected.to_str().unwrap(),
                "-f",
                "--to",
                &bob_id,
            ],
        );
        assert_eq!(receipt["bytes"], bytes.len());
        assert_eq!(receipt["sha256"], hashes[index]);
    }
    // These are actual recipient publications, not fixture-written inbox objects.
    let stored = received(&bob_inbox);
    assert_eq!(stored.len(), 2);
    let inventory = json(&bob, &["room", "files", "team", "received"]);
    assert_eq!(inventory["content_verified"], false);
    assert_eq!(inventory["truncated"], false);
    assert_eq!(inventory["unavailable"], 0);
    assert_eq!(inventory["files"].as_array().unwrap().len(), 2);
    let ids = payloads
        .iter()
        .map(|bytes| {
            inventory["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["bytes"] == bytes.len())
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let original_inodes = ids
        .iter()
        .map(|id| {
            let metadata = std::fs::metadata(bob_inbox.join(id)).unwrap();
            (metadata.dev(), metadata.ino())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        json(&bob, &["room", "receiving", "team", "--disable"])["accepted"],
        false
    );
    assert_receiving_off(&bob);
    json(&bob, &["room", "stop", "team"]);
    receiver.finish_successfully();
    // Normal serving startup alone must register the saved-copy source root.
    // No direct Factory binding, receive toggle, caller path or manual grant.
    let mut resumed = Process::start(&bob, &["room", "serve", "team"]);
    resumed.until("ROOM SERVING");
    assert_receiving_off(&bob);

    let sync = json(
        &bob,
        &[
            "room",
            "files",
            "team",
            "forward-copy",
            &ids[0],
            "--to",
            &alice_id,
        ],
    );
    assert_eq!(sync["bytes"], payloads[0].len());
    assert_eq!(sync["sha256"], hashes[0]);
    assert_eq!(sync["resumed_from"], 0);
    assert!(
        sync.get("id").is_none(),
        "default forwarding must return a receipt, not a job ID"
    );
    assert_receiving_off(&bob);

    let started = json(
        &bob,
        &[
            "room",
            "files",
            "team",
            "forward-copy",
            &ids[1],
            "--to",
            &alice_id,
            "--sensitive",
            "--background",
        ],
    );
    let forbidden = [
        ids[1].as_str(),
        hashes[1].as_str(),
        bob_inbox.to_str().unwrap(),
    ];
    assert_job_privacy(&started, &forbidden);
    assert_eq!(
        started["progress"]["phase"] == "completed",
        !started["receipt"].is_null(),
        "initial job counters cannot stand in for a verified receipt"
    );
    let id = started["id"]
        .as_str()
        .expect("background mode must return an opaque owner job ID");
    let job = completed_job(&bob, id, &forbidden);
    assert_eq!(job["receipt"]["bytes"], payloads[1].len());
    assert_eq!(job["receipt"]["resumed_from"], 0);
    assert_eq!(job["progress"]["total_bytes"], payloads[1].len());
    assert_eq!(job["progress"]["source_bytes"], payloads[1].len());
    assert_eq!(job["retry_allowed"], false);
    assert_receiving_off(&bob);

    // A denied target fails before handoff. This is a certain, durable failure,
    // suitable for an explicit corrected attempt with a freshly selected source.
    let absent_peer = "da".repeat(32);
    assert_ne!(absent_peer, alice_id);
    assert_ne!(absent_peer, bob_id);
    let rejected = json(
        &bob,
        &[
            "room",
            "files",
            "team",
            "forward-copy",
            &ids[1],
            "--to",
            &absent_peer,
            "--background",
            "--sensitive",
        ],
    );
    let rejected_id = rejected["id"].as_str().unwrap();
    let failed = terminal_job(&bob, rejected_id, &forbidden, "failed");
    assert_eq!(failed["progress"]["error"], "DENIED");
    assert_eq!(failed["progress"]["handed_off"], false);
    assert_eq!(failed["retry_allowed"], true);

    json(&bob, &["room", "stop", "team"]);
    resumed.finish_successfully();
    resumed = Process::start(&bob, &["room", "serve", "team"]);
    resumed.until("ROOM SERVING");
    assert_receiving_off(&bob);
    let previous = json(&bob, &["room", "transfer", "team", id]);
    assert_eq!(previous["progress"]["phase"], "completed");
    assert_eq!(previous["historical"], true);
    assert_eq!(previous["terminal_durable"], true);
    assert_eq!(previous["retry_allowed"], false);
    assert_job_privacy(&previous, &forbidden);
    let old_failure = json(&bob, &["room", "transfer", "team", rejected_id]);
    assert_eq!(old_failure["historical"], true);
    assert_eq!(old_failure["retry_allowed"], true);
    let history = json(&bob, &["room", "transfers", "team"]);
    assert_eq!(
        history.as_array().unwrap().len(),
        2,
        "restart must not replay jobs"
    );
    assert!(
        history
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["historical"] == true)
    );

    let (status, output, _) = Process::start(
        &bob,
        &[
            "room",
            "files",
            "team",
            "forward-copy",
            &ids[1],
            "--to",
            &alice_id,
            "--background",
            "--retry-of",
            id,
        ],
    )
    .finish(WAIT);
    assert!(!status.success());
    let refusal: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(refusal["code"], "DENIED", "a completed job must not replay");
    assert_eq!(
        json(&bob, &["room", "transfers", "team"])
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let target_copy = alice_inbox.join(format!("{bob_id}-{}.bin", hashes[1]));
    let before = std::fs::metadata(&target_copy).unwrap();
    let retried = json(
        &bob,
        &[
            "room",
            "files",
            "team",
            "forward-copy",
            &ids[1],
            "--to",
            &alice_id,
            "--background",
            "--sensitive",
            "--retry-of",
            rejected_id,
        ],
    );
    let retried_id = retried["id"].as_str().unwrap();
    assert_ne!(retried_id, rejected_id);
    let retry = completed_job(&bob, retried_id, &forbidden);
    assert_eq!(retry["receipt"]["resumed_from"], payloads[1].len());
    assert_eq!(retry["progress"]["source_bytes"], 0);
    let after = std::fs::metadata(&target_copy).unwrap();
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    assert_receiving_off(&bob);

    let forwarded = received(&alice_inbox);
    assert_eq!(forwarded.len(), 2);
    for (index, bytes) in payloads.iter().enumerate() {
        let destination = forwarded
            .iter()
            .find(|path| std::fs::read(path).unwrap() == *bytes)
            .unwrap();
        // Actual TLS sender is Bob, not the original source Alice. No fabricated
        // origin metadata or source filename is used as forwarding authority.
        assert_eq!(
            destination.file_name().unwrap().to_str().unwrap(),
            format!("{bob_id}-{}.bin", hashes[index])
        );
        assert_eq!(digest(&std::fs::read(destination).unwrap()), hashes[index]);
        assert_eq!(std::fs::read(bob_inbox.join(&ids[index])).unwrap(), *bytes);
        let metadata = std::fs::metadata(bob_inbox.join(&ids[index])).unwrap();
        assert_eq!((metadata.dev(), metadata.ino()), original_inodes[index]);
        assert!(ids[index].starts_with(&format!("{alice_id}-")));
    }
    assert_eq!(received(&bob_inbox), stored);
    let (status, _, error) =
        Process::start(&bob, &["room", "files", "team", "delete-copy", &ids[0]]).finish(WAIT);
    assert_eq!(status.code(), Some(2));
    assert!(error.contains("--confirm"));
    assert_eq!(received(&bob_inbox), stored);
    let deleted = json(
        &bob,
        &["room", "files", "team", "delete-copy", &ids[0], "--confirm"],
    );
    assert_eq!(deleted["deleted"], true);
    assert!(!bob_inbox.join(&ids[0]).exists());
    assert_eq!(
        json(
            &bob,
            &["room", "files", "team", "delete-copy", &ids[0], "--confirm"]
        )["deleted"],
        false
    );
    let inventory = json(&bob, &["room", "files", "team", "received"]);
    assert_eq!(inventory["files"].as_array().unwrap().len(), 1);
    assert_eq!(inventory["files"][0]["id"], ids[1]);
    assert_eq!(std::fs::read(bob_inbox.join(&ids[1])).unwrap(), payloads[1]);
    assert_eq!(
        received(&alice_inbox),
        forwarded,
        "local delete must not erase a peer copy"
    );
    assert_receiving_off(&bob);
    json(&bob, &["room", "stop", "team"]);
    resumed.finish_successfully();
    json(&alice, &["room", "stop", "team"]);
    sender.finish_successfully();
}
