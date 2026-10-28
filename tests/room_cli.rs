#![cfg(unix)]
//! Actual CLI processes over loopback. This is not a cross-VM evidence claim.
mod support;
use std::{
    io::{BufRead, BufReader},
    net::{SocketAddr, UdpSocket},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

fn address() -> String {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string()
}

#[test]
fn cancelled_status_observations_are_not_failed_room_operations() {
    use std::io::Write;
    let root = support::private_root();
    let profile = root.path().join("observer");
    call(&profile, &["init"]);
    call(&profile, &["room", "create", "team", "--bind", &address()]);
    let mut server = Process::start(&profile, &["room", "serve", "team"]);
    server.until("ROOM SERVING");
    let body = serde_json::to_vec(&kk::rooms::local::Command::Status).unwrap();
    for _ in 0..16 {
        let mut stream =
            std::os::unix::net::UnixStream::connect(profile.join("rooms/team/node.sock")).unwrap();
        stream
            .write_all(&(body.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&body).unwrap();
        drop(stream);
    }
    let status: serde_json::Value =
        serde_json::from_str(&call(&profile, &["room", "status", "team"])).unwrap();
    assert_eq!(status["result"]["running"], true);
    call(&profile, &["room", "stop", "team"]);
    server.finish();
    let output = server.lines.iter().collect::<Vec<_>>().join("\n");
    assert!(
        !output.contains("Room local command failed"),
        "read-only caller cancellation was misreported: {output}"
    );
}

#[test]
fn consent_withdrawal_and_status_bypass_saturated_ordinary_ipc_connections() {
    let root = support::private_root();
    let profile = root.path().join("saturation");
    let inbox = root.path().join("inbox");
    call(&profile, &["init"]);
    call(&profile, &["room", "create", "team", "--bind", &address()]);
    let mut server = Process::start(
        &profile,
        &[
            "room",
            "serve",
            "team",
            "--accept-files",
            "--inbox",
            inbox.to_str().unwrap(),
        ],
    );
    server.until("ROOM SERVING");
    // Queue more than the entire ordinary connection budget, withholding every
    // command frame. FIFO acceptance cannot reach a later ordinary control RPC
    // before the stalled-handshake timeout. The reserved endpoint stays free.
    let stalled: Vec<_> = (0..40)
        .map(|_| {
            std::os::unix::net::UnixStream::connect(profile.join("rooms/team/node.sock")).unwrap()
        })
        .collect();
    // Also occupy every reserved handshake slot. These readers have a short
    // control-specific expiry, not the ordinary ten-second data timeout.
    let stalled_control: Vec<_> = (0..4)
        .map(|_| {
            std::os::unix::net::UnixStream::connect(profile.join("rooms/team/ctl/node.sock"))
                .unwrap()
        })
        .collect();
    let started = std::time::Instant::now();
    let reply: serde_json::Value =
        serde_json::from_str(&call(&profile, &["room", "receiving", "team", "--disable"])).unwrap();
    assert_eq!(reply["result"]["accepted"], false);
    let status: serde_json::Value =
        serde_json::from_str(&call(&profile, &["room", "status", "team"])).unwrap();
    assert_eq!(status["result"]["running"], true);
    assert!(
        started.elapsed() < kk::config::DEFAULT_TIMEOUT / 2,
        "control calls waited for stalled ordinary connections"
    );
    let global: serde_json::Value =
        serde_json::from_str(&call(&profile, &["status", "--json"])).unwrap();
    assert_eq!(global["node"]["state"], "offline");
    assert_eq!(global["rooms"][0]["observation"]["state"], "online");
    call(&profile, &["room", "stop", "team"]);
    server.finish();
    drop(stalled);
    drop(stalled_control);
    assert!(!profile.join("rooms/team/ctl/node.sock").exists());
    assert!(!profile.join("rooms/team/node.sock").exists());
}

struct Process {
    child: Child,
    lines: mpsc::Receiver<String>,
}
impl Process {
    fn start(profile: &Path, args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_kk"))
            .arg("--profile")
            .arg(profile)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (sender, lines) = mpsc::channel();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        for input in [
            Box::new(stdout) as Box<dyn std::io::Read + Send>,
            Box::new(stderr),
        ] {
            let sender = sender.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(input).lines() {
                    let Ok(line) = line else {
                        break;
                    };
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        Self { child, lines }
    }
    fn until(&self, prefix: &str) -> String {
        let until = std::time::Instant::now() + Duration::from_secs(110);
        let mut seen = Vec::new();
        loop {
            match self
                .lines
                .recv_timeout(until.saturating_duration_since(std::time::Instant::now()))
            {
                Ok(line) if line.starts_with(prefix) => return line,
                Ok(line) => seen.push(line),
                Err(error) => panic!("missing {prefix:?}: {error}; output={seen:?}"),
            }
        }
    }
    fn finish(&mut self) {
        assert!(self.child.wait().unwrap().success());
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn call(profile: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(profile)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn rooms_have_an_actual_cli_enrollment_broadcast_and_cold_resume() {
    let root = support::private_root();
    let alice = root.path().join("alice");
    let bob = root.path().join("bob");
    let carol = root.path().join("carol");
    for profile in [&alice, &bob, &carol] {
        call(profile, &["init"]);
    }
    let alice_address = address();
    call(
        &alice,
        &[
            "room",
            "create",
            "team",
            "--title",
            "Actual CLI room",
            "--bind",
            &alice_address,
        ],
    );
    let manifest = std::fs::read(alice.join("rooms/team/manifest.json")).unwrap();
    let mut source = Process::start(&alice, &["room", "serve", "team"]);
    source.until("ROOM SERVING");
    // Export a real invitation, then shut down its owner. Connection refusal
    // must retain only runtime state, and a fresh invite must reuse that name.
    let expired = alice.join("unavailable-invitation.json");
    let mut unavailable = Process::start(
        &alice,
        &[
            "room",
            "invite",
            "team",
            "--bind",
            &address(),
            "--invite-out",
            expired.to_str().unwrap(),
            "--confirm",
        ],
    );
    unavailable.until("ROOM INVITATION");
    call(&alice, &["room", "stop", "team"]);
    source.finish();
    assert!(!unavailable.child.wait().unwrap().success());
    let refused = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&bob)
        .args([
            "room",
            "join",
            "team",
            "--invitation",
            expired.to_str().unwrap(),
            "--bind",
            &address(),
            "--confirm",
        ])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("Connection refused"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    let reservation = bob.join("rooms/team");
    assert!(reservation.join("node.lock").is_file());
    assert!(!reservation.join("manifest.json").exists());
    assert!(!reservation.join("private").exists());
    source = Process::start(&alice, &["room", "serve", "team"]);
    source.until("ROOM SERVING");
    let mut guests = Vec::new();
    for (index, profile) in [&bob, &carol].into_iter().enumerate() {
        let invitation = alice.join(format!("invitation-{index}.json"));
        let mut invite = Process::start(
            &alice,
            &[
                "room",
                "invite",
                "team",
                "--bind",
                &address(),
                "--invite-out",
                invitation.to_str().unwrap(),
                "--role",
                "moderator",
                "--confirm",
            ],
        );
        invite.until("ROOM INVITATION");
        let guest = Process::start(
            profile,
            &[
                "room",
                "join",
                "team",
                "--invitation",
                invitation.to_str().unwrap(),
                "--bind",
                &address(),
                "--role",
                "moderator",
                "--confirm",
            ],
        );
        guest.until("ROOM SERVING");
        invite.until("{\"ok\":true");
        invite.finish();
        guests.push(guest);
    }
    let reply = call(
        &alice,
        &["room", "send", "team", "first-public-room-message"],
    );
    assert!(reply.contains("\"all_acknowledged\":true"));
    for guest in &guests {
        assert!(
            guest
                .until("ROOM TEXT")
                .contains("first-public-room-message")
        );
    }
    let status: serde_json::Value =
        serde_json::from_str(&call(&bob, &["room", "status", "team"])).unwrap();
    assert_eq!(status["result"]["members"].as_array().unwrap().len(), 3);
    assert_eq!(status["result"]["freshness"], "local-durable-cache");
    let bob_id = status["result"]["owner"].as_str().unwrap();
    let file = alice.join("payload.bin");
    let bytes = vec![0xa7u8; 196_608];
    std::fs::write(&file, &bytes).unwrap();
    let refused = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&alice)
        .args([
            "room",
            "send",
            "team",
            file.to_str().unwrap(),
            "-f",
            "--to",
            bob_id,
        ])
        .output()
        .unwrap();
    assert!(
        !refused.status.success(),
        "files need explicit recipient consent"
    );
    let voters: Vec<_> = status["result"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap())
        .collect();
    call(
        &alice,
        &[
            "room",
            "controllers",
            "team",
            voters[0],
            voters[1],
            voters[2],
        ],
    );
    call(&bob, &["room", "stop", "team"]);
    guests[0].finish();
    let inbox = bob.join("inbox");
    let mut resumed = Process::start(
        &bob,
        &[
            "room",
            "serve",
            "team",
            "--accept-files",
            "--inbox",
            inbox.to_str().unwrap(),
            "--max-bytes",
            "1048576",
            "--file-budget",
            "2097152",
        ],
    );
    resumed.until("ROOM SERVING");
    let reply = call(&bob, &["room", "send", "team", "after-public-cold-resume"]);
    assert!(reply.contains("\"all_acknowledged\":true"));
    assert!(
        source
            .until("ROOM TEXT")
            .contains("after-public-cold-resume")
    );
    assert!(
        guests[1]
            .until("ROOM TEXT")
            .contains("after-public-cold-resume")
    );
    let sent: serde_json::Value = serde_json::from_str(&call(
        &alice,
        &[
            "room",
            "send",
            "team",
            file.to_str().unwrap(),
            "-f",
            "--to",
            bob_id,
        ],
    ))
    .unwrap();
    assert_eq!(sent["result"]["bytes"], bytes.len());
    assert_eq!(sent["result"]["resumed_from"], 0);
    let stored: Vec<_> = std::fs::read_dir(&inbox)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "bin"))
        .collect();
    assert_eq!(stored.len(), 1);
    assert_eq!(std::fs::read(&stored[0]).unwrap(), bytes);
    let resumed_file: serde_json::Value = serde_json::from_str(&call(
        &alice,
        &[
            "room",
            "send",
            "team",
            file.to_str().unwrap(),
            "-f",
            "--to",
            bob_id,
        ],
    ))
    .unwrap();
    assert_eq!(resumed_file["result"]["resumed_from"], bytes.len());
    // A fully committed file is reverified without rewriting it. Sensitivity
    // controls this receipt, not retroactive erasure of a prior ordinary copy.
    let changed_offer = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&alice)
        .args([
            "room",
            "send",
            "team",
            file.to_str().unwrap(),
            "-f",
            "-s",
            "--to",
            bob_id,
        ])
        .output()
        .unwrap();
    assert!(changed_offer.status.success());
    let changed_receipt: serde_json::Value = serde_json::from_slice(&changed_offer.stdout).unwrap();
    assert!(changed_receipt["result"]["sha256"].is_null());
    assert_eq!(std::fs::read(&stored[0]).unwrap(), bytes);
    let sensitive_bytes = vec![0x5bu8; 131_072];
    std::fs::write(&file, &sensitive_bytes).unwrap();
    let sensitive: serde_json::Value = serde_json::from_str(&call(
        &alice,
        &[
            "room",
            "send",
            "team",
            file.to_str().unwrap(),
            "-f",
            "-s",
            "--to",
            bob_id,
        ],
    ))
    .unwrap();
    assert!(sensitive["result"]["sha256"].is_null());
    let carol_status: serde_json::Value =
        serde_json::from_str(&call(&carol, &["room", "status", "team"])).unwrap();
    let carol_id = carol_status["result"]["owner"].as_str().unwrap();
    let digest = sent["result"]["sha256"].as_str().unwrap();
    let absent: serde_json::Value = serde_json::from_str(&call(
        &carol,
        &["room", "files", "team", "locate", digest, "--from", bob_id],
    ))
    .unwrap();
    assert_eq!(absent["result"]["advertised"].as_array().unwrap().len(), 0);
    call(
        &bob,
        &[
            "room",
            "files",
            "team",
            "share",
            stored[0].to_str().unwrap(),
            "--to",
            carol_id,
        ],
    );
    let hidden: serde_json::Value = serde_json::from_str(&call(
        &alice,
        &["room", "files", "team", "locate", digest, "--from", bob_id],
    ))
    .unwrap();
    assert_eq!(
        hidden["result"]["advertised"].as_array().unwrap().len(),
        0,
        "the original sender is not implicitly a publication audience"
    );
    let available: serde_json::Value = serde_json::from_str(&call(
        &carol,
        &["room", "files", "team", "locate", digest, "--from", bob_id],
    ))
    .unwrap();
    assert_eq!(available["result"]["advertised"][0]["sha256"], digest);
    let no_consent = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&carol)
        .args(["room", "files", "team", "fetch", digest, "--from", bob_id])
        .output()
        .unwrap();
    assert!(!no_consent.status.success());
    call(&carol, &["room", "stop", "team"]);
    guests[1].finish();
    let carol_inbox = carol.join("inbox");
    guests[1] = Process::start(
        &carol,
        &[
            "room",
            "serve",
            "team",
            "--accept-files",
            "--inbox",
            carol_inbox.to_str().unwrap(),
        ],
    );
    guests[1].until("ROOM SERVING");
    let fetched: serde_json::Value = serde_json::from_str(&call(
        &carol,
        &["room", "files", "team", "fetch", digest, "--from", bob_id],
    ))
    .unwrap();
    assert_eq!(fetched["result"]["sha256"], digest);
    assert_eq!(fetched["result"]["resumed_from"], 0);
    let replica = carol_inbox.join(format!("{bob_id}-{digest}.bin"));
    assert_eq!(std::fs::read(&replica).unwrap(), bytes);
    let repeated_replica: serde_json::Value = serde_json::from_str(&call(
        &carol,
        &["room", "files", "team", "fetch", digest, "--from", bob_id],
    ))
    .unwrap();
    assert_eq!(repeated_replica["result"]["resumed_from"], bytes.len());
    call(&bob, &["room", "files", "team", "unshare", digest]);
    let withdrawn: serde_json::Value = serde_json::from_str(&call(
        &carol,
        &["room", "files", "team", "locate", digest, "--from", bob_id],
    ))
    .unwrap();
    assert_eq!(
        withdrawn["result"]["advertised"].as_array().unwrap().len(),
        0
    );
    let denied = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&carol)
        .args(["room", "files", "team", "fetch", digest, "--from", bob_id])
        .output()
        .unwrap();
    assert!(!denied.status.success());
    assert_eq!(std::fs::read(&replica).unwrap(), bytes);
    call(
        &bob,
        &[
            "room",
            "files",
            "team",
            "share",
            stored[0].to_str().unwrap(),
            "--to",
            carol_id,
        ],
    );
    std::fs::write(&stored[0], b"changed after the explicit offer").unwrap();
    let changed_replica = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&carol)
        .args(["room", "files", "team", "fetch", digest, "--from", bob_id])
        .output()
        .unwrap();
    assert!(
        !changed_replica.status.success(),
        "an advertised digest is not proof of unchanged storage"
    );
    assert_eq!(std::fs::read(&replica).unwrap(), bytes);
    std::fs::write(&stored[0], &bytes).unwrap();
    // Transport inspection is deliberately not a native decoder/capture claim.
    let alice_status: serde_json::Value =
        serde_json::from_str(&call(&alice, &["room", "status", "team"])).unwrap();
    let alice_id = alice_status["result"]["owner"].as_str().unwrap();
    let rtp = alice.join("synthetic-transport.rtp");
    let mut recording = Vec::new();
    for sequence in 0u16..4 {
        let mut packet = vec![0x80, 97];
        packet.extend(sequence.to_be_bytes());
        packet.extend((sequence as u32 * 6000).to_be_bytes());
        packet.extend(1u32.to_be_bytes());
        packet.push(0);
        recording.extend((packet.len() as u16).to_be_bytes());
        recording.extend(packet);
    }
    std::fs::write(&rtp, recording).unwrap();
    let unconsented = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&alice)
        .args([
            "room",
            "media",
            "team",
            "publish",
            "screen",
            "--rtp-file",
            rtp.to_str().unwrap(),
            "--to",
            bob_id,
            "--consent",
        ])
        .output()
        .unwrap();
    assert!(!unconsented.status.success());
    let mut media = Process::start(
        &bob,
        &[
            "room",
            "media",
            "team",
            "listen",
            "screen",
            "--from",
            alice_id,
            "--inspect",
            "--consent",
        ],
    );
    media.until("MEDIA LISTENING");
    let sent: serde_json::Value = serde_json::from_str(&call(
        &alice,
        &[
            "room",
            "media",
            "team",
            "publish",
            "screen",
            "--rtp-file",
            rtp.to_str().unwrap(),
            "--to",
            bob_id,
            "--consent",
        ],
    ))
    .unwrap();
    assert_eq!(sent["result"]["statistics"]["sent"], 4);
    let received: serde_json::Value = serde_json::from_str(&media.until("{")).unwrap();
    assert_eq!(received["packets"], 4);
    assert_eq!(received["bytes"], 52);
    assert_eq!(received["recorded"], false);
    media.finish();
    let previous: serde_json::Value =
        serde_json::from_str(&call(&bob, &["room", "status", "team"])).unwrap();
    call(&bob, &["room", "stop", "team"]);
    resumed.finish();
    let mut moved = address();
    if previous["result"]["address"] == moved {
        moved = address();
    }
    resumed = Process::start(&bob, &["room", "serve", "team", "--bind", &moved]);
    resumed.until("ROOM SERVING");
    let changed: serde_json::Value =
        serde_json::from_str(&call(&bob, &["room", "status", "team"])).unwrap();
    assert_eq!(changed["result"]["address"], moved);
    assert_eq!(changed["result"]["owner"], previous["result"]["owner"]);
    assert_eq!(changed["result"]["voters"], previous["result"]["voters"]);
    call(
        &alice,
        &["room", "send", "team", "after-authenticated-address-change"],
    );
    assert!(
        resumed
            .until("ROOM TEXT")
            .contains("after-authenticated-address-change")
    );
    assert!(
        guests[1]
            .until("ROOM TEXT")
            .contains("after-authenticated-address-change")
    );
    call(&bob, &["room", "stop", "team"]);
    resumed.finish();
    resumed = Process::start(&bob, &["room", "serve", "team"]);
    resumed.until("ROOM SERVING");
    let persisted: serde_json::Value =
        serde_json::from_str(&call(&bob, &["room", "status", "team"])).unwrap();
    assert_eq!(persisted["result"]["address"], moved);
    let not_restored: serde_json::Value = serde_json::from_str(&call(
        &carol,
        &["room", "files", "team", "locate", digest, "--from", bob_id],
    ))
    .unwrap();
    assert_eq!(
        not_restored["result"]["advertised"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "a service restart cannot silently republish a copy"
    );
    assert_eq!(
        std::fs::read_dir(bob.join("rooms/team/address-history"))
            .unwrap()
            .count(),
        1
    );
    call(&alice, &["room", "stop", "team"]);
    source.finish();
    call(
        &bob,
        &[
            "room",
            "files",
            "team",
            "share",
            stored[0].to_str().unwrap(),
            "--to",
            carol_id,
        ],
    );
    let without_original: serde_json::Value = serde_json::from_str(&call(
        &carol,
        &["room", "files", "team", "fetch", digest, "--from", bob_id],
    ))
    .unwrap();
    assert_eq!(without_original["result"]["resumed_from"], bytes.len());
    assert_eq!(std::fs::read(&replica).unwrap(), bytes);
    call(&bob, &["room", "controllers", "team", bob_id, carol_id]);
    call(&bob, &["room", "remove", "team", alice_id]);
    let retired_epoch: serde_json::Value = serde_json::from_str(&call(
        &carol,
        &["room", "files", "team", "locate", digest, "--from", bob_id],
    ))
    .unwrap();
    assert_eq!(
        retired_epoch["result"]["advertised"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "availability approval belongs to one membership epoch"
    );
    call(
        &bob,
        &[
            "room",
            "files",
            "team",
            "share",
            stored[0].to_str().unwrap(),
            "--to",
            carol_id,
        ],
    );
    call(
        &carol,
        &["room", "files", "team", "fetch", digest, "--from", bob_id],
    );
    assert_eq!(std::fs::read(&replica).unwrap(), bytes);
    call(&bob, &["room", "stop", "team"]);
    resumed.finish();
    call(&carol, &["room", "stop", "team"]);
    guests[1].finish();
    assert_eq!(
        std::fs::read(alice.join("rooms/team/manifest.json")).unwrap(),
        manifest
    );
    for profile in [&alice, &bob, &carol] {
        assert!(!profile.join("rooms/team/node.sock").exists());
        assert_eq!(
            std::fs::read_dir(profile.join("identity")).unwrap().count(),
            2
        );
    }
    let _: SocketAddr = alice_address.parse().unwrap();
}

#[test]
fn room_names_missing_identity_and_existing_state_fail_without_replacement() {
    let root = support::private_root();
    let profile = root.path().join("profile");
    call(&profile, &["init"]);
    for name in ["../outside", "", "Team", "a/b"] {
        let output = Command::new(env!("CARGO_BIN_EXE_kk"))
            .arg("--profile")
            .arg(&profile)
            .args(["room", "create", name, "--bind", &address()])
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
    call(
        &profile,
        &["room", "create", "retained", "--bind", &address()],
    );
    let manifest = std::fs::read(profile.join("rooms/retained/manifest.json")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&profile)
        .args(["room", "create", "retained", "--bind", &address()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        std::fs::read(profile.join("rooms/retained/manifest.json")).unwrap(),
        manifest
    );
    std::fs::rename(
        profile.join("identity"),
        root.path().join("retained-identity"),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_kk"))
        .arg("--profile")
        .arg(&profile)
        .arg("init")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!profile.join("identity").exists());
}
