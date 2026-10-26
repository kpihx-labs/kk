//! Explicit preapproved-bootstrap VM fixture, NOT a room invitation/CLI product.
//! Real data uses QUIC. Virtio commands select synthetic payload indices; results
//! expose hashes/lengths/flags only. No private key or MLS DB leaves its guest.
use super::{Genesis, NodeConfig, anchor, json, output_json, publish_json, read, start};
use kk::{
    Error, Identity, Peer, Result, config,
    contacts::{Contact, Permissions, Registry},
    profile,
    rooms::{
        RootBundle,
        group::Group,
        mls::CryptoMember,
        runtime::{Actor, Event, Handle, Room},
    },
};
use ring::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::mpsc, task::JoinHandle};

const DEADLINE: Duration = Duration::from_secs(8);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentInvite {
    listen: std::net::SocketAddr,
    expected_certificate: PathBuf,
    invite: PathBuf,
    status: PathBuf,
    result: PathBuf,
    role: String,
}

pub fn genesis(identity: &Path, state: &Path, out: &Path) -> Result<()> {
    let identity = Identity::load(identity)?;
    let root = RootBundle::create(&identity, "Private cross-guest room fixture")?;
    let actor =
        CryptoMember::create_private(&identity, &root.verify(&identity.peer())?, vec![], state)?;
    let creator = actor.public_identity()?;
    drop(Group::create(actor)?);
    output_json(out, &Genesis { root, creator })
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
enum Action {
    Controllers {
        voters: std::collections::BTreeSet<kk::rooms::control::raft::PeerId>,
    },
    Status,
    Sync,
    Admit {
        package: PathBuf,
    },
    SubmitPending,
    Join {
        inviter_certificate: PathBuf,
    },
    Send {
        target_certificate: PathBuf,
        fixture: u32,
        bytes: usize,
        sensitive: bool,
        #[serde(default)]
        capture: bool,
    },
    ReplayCaptured {
        target_certificate: PathBuf,
    },
    Remove {
        subject_certificate: PathBuf,
    },
    Revoke {
        subject_certificate: PathBuf,
    },
    Stop,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    instance: [u8; 16],
    command: Action,
}
impl Request {
    fn for_instance(self, expected: [u8; 16]) -> Result<Action> {
        if self.instance != expected {
            return Err(Error::Cancelled);
        }
        Ok(self.command)
    }
}

struct Running {
    handle: Handle,
    task: JoinHandle<Room>,
    events: mpsc::Receiver<Event>,
}
impl Running {
    async fn start(
        room: Room,
        control: &kk::rooms::control::raft::service::ControlNode,
    ) -> Result<Self> {
        let (handle, actor, events) = Actor::new(room);
        let task = tokio::task::spawn_local(actor.run());
        if let Err(error) = control.register_room(handle.clone()).await {
            task.abort();
            let _ = task.await;
            return Err(error);
        }
        Ok(Self {
            handle,
            task,
            events,
        })
    }
    async fn stop(self) -> Result<()> {
        let stopped = self.handle.shutdown(DEADLINE).await;
        if stopped.is_err() {
            self.task.abort();
        }
        let joined = self.task.await;
        match (stopped, joined) {
            (Ok(()), Ok(room)) => {
                drop(room);
                Ok(())
            }
            (Err(error), _) => Err(error),
            (_, Err(_)) => Err(Error::Node("private fixture actor terminated unexpectedly")),
        }
    }
}

struct Fixture {
    instance: [u8; 16],
    config: NodeConfig,
    identity: Arc<Identity>,
    control: Arc<kk::rooms::control::raft::service::ControlNode>,
    private_state: PathBuf,
    profile_dir: PathBuf,
    running: Option<Running>,
    received: u64,
    latest: Value,
    captured: Option<Vec<u8>>,
    phase: &'static str,
    enrolling: Option<JoinHandle<()>>,
}
impl Fixture {
    async fn invite(&mut self, config: EnrollmentInvite) -> Result<()> {
        let rights = super::role_rights(&config.role)?;
        let handle = self.handle()?;
        let room = self.control.room_id();
        let data_port = self.control.local_addr()?.port();
        let identity = self.identity.clone();
        let expected = Peer::load(&config.expected_certificate)?.fingerprint();
        self.enrolling = Some(tokio::task::spawn_local(async move {
            let result = async {
                let server = kk::pairing::PairServer::bind_room(
                    config.listen,
                    Duration::from_secs(120),
                    room,
                )
                .await?;
                kk::pairing::Invite::from_server(&server, &identity)?.export(&config.invite)?;
                publish_json(
                    &config.status,
                    &json!({"pid": std::process::id(), "ready": true, "room": room}),
                    true,
                )?;
                let session = server
                    .run_room_session(
                        &identity,
                        data_port,
                        |offer| async move {
                            Ok(offer.fingerprint == expected
                                && offer.scope == kk::pairing::Scope::Room(room))
                        },
                        |_| Ok(()),
                    )
                    .await?;
                kk::rooms::enrollment::admit(session, &handle, rights, Duration::from_secs(90))
                    .await
            }
            .await;
            let value = match result {
                Ok(admission) => json!({"ok": true, "admission": admission}),
                Err(error) => json!({"ok": false, "error": error.to_string()}),
            };
            if let Err(error) = publish_json(&config.result, &value, false) {
                eprintln!("Enrollment result publication failed: {error}");
            }
        }));
        Ok(())
    }
    fn handle(&self) -> Result<Handle> {
        self.running
            .as_ref()
            .map(|running| running.handle.clone())
            .ok_or(Error::Node("candidate is not privately activated"))
    }
    async fn status(&self) -> Result<Value> {
        let public = self.control.public_view().await?;
        let contacts = Registry::open(self.profile_dir.clone())?.list()?.len();
        let private = match &self.running {
            Some(running) => {
                let status = running.handle.status(DEADLINE).await?;
                json!({"epoch": status.epoch, "active": status.active, "applied": status.applied, "members": status.members})
            }
            None => Value::Null,
        };
        Ok(
            json!({"pid": std::process::id(), "instance": self.instance, "profile_contacts": contacts, "id": self.control.id(), "leader_hint": self.control.current_leader().await,
            "voters": self.control.control_membership().membership().voter_ids().collect::<Vec<_>>(), "joint": self.control.control_membership().membership().get_joint_config().len(),
            "epoch": public.epoch(), "head": public.head(), "private": private, "received": self.received, "latest": self.latest}),
        )
    }
    async fn act(&mut self, action: Action) -> Result<Value> {
        self.phase = "dispatch";
        match action {
            Action::Controllers { voters } => {
                let applied = self.handle()?.approve_controllers(voters, DEADLINE).await?;
                Ok(
                    json!({"membership": applied, "voters": self.control.control_membership().membership().voter_ids().collect::<Vec<_>>()}),
                )
            }
            Action::Status => self.status().await,
            Action::Sync => {
                self.handle()?.synchronize(DEADLINE).await?;
                self.status().await
            }
            Action::Admit { package } => {
                let handle = self.handle()?;
                let proposal = handle.prepare_admission(read(&package)?, DEADLINE).await?;
                let receipt = handle.submit_prepared(DEADLINE).await?;
                if !receipt.accepted {
                    return Err(Error::Denied);
                }
                Ok(json!({"proposal": proposal.id()?, "receipt": receipt}))
            }
            Action::SubmitPending => {
                Ok(json!({"receipt": self.handle()?.submit_prepared(DEADLINE).await?}))
            }
            Action::Join {
                inviter_certificate,
            } => {
                if self.running.is_some() {
                    return Err(Error::Node("private member already active"));
                }
                let (anchor, _) = anchor(&self.config.context)?;
                let actor =
                    CryptoMember::open_private(&self.identity, &anchor, &self.private_state)?;
                let room = Room::join(
                    self.identity.clone(),
                    self.control.clone(),
                    actor,
                    &Peer::load(&inviter_certificate)?,
                    Registry::open(self.profile_dir.clone())?,
                    DEADLINE,
                )
                .await?;
                self.running = Some(Running::start(room, &self.control).await?);
                self.status().await
            }
            Action::Send {
                target_certificate,
                fixture,
                bytes,
                sensitive,
                capture,
            } => {
                if bytes > config::MAX_MESSAGE_BYTES {
                    return Err(Error::MessageTooLarge);
                }
                let mut text = format!("kk-room-fixture-{fixture}:");
                if text.len() > bytes {
                    text.truncate(bytes);
                } else {
                    text.push_str(&"x".repeat(bytes - text.len()));
                }
                let hash = digest(text.as_bytes());
                self.phase = "send-encrypt";
                let wire = self
                    .handle()?
                    .encrypt_text(text, sensitive, DEADLINE)
                    .await?;
                let peer = Peer::load(&target_certificate)?;
                if capture {
                    self.captured = Some(wire.clone());
                }
                self.phase = "send-quic";
                self.control
                    .send_room_text(
                        kk::rooms::control::raft::PeerId::from_peer(&peer),
                        &wire,
                        DEADLINE,
                    )
                    .await?;
                Ok(
                    json!({"sha256": hash, "bytes": bytes, "sensitive": sensitive, "ciphertext_bytes": wire.len()}),
                )
            }
            Action::ReplayCaptured { target_certificate } => {
                // Test-only explicit adversarial replay. Exactly one bounded
                // ciphertext is held in RAM; no plaintext or key is copied.
                let wire = self
                    .captured
                    .as_ref()
                    .ok_or(Error::Node("no captured fixture ciphertext"))?;
                let peer = Peer::load(&target_certificate)?;
                self.control
                    .send_room_text(
                        kk::rooms::control::raft::PeerId::from_peer(&peer),
                        wire,
                        DEADLINE,
                    )
                    .await?;
                Ok(json!({"replayed": true, "ciphertext_bytes": wire.len()}))
            }
            Action::Remove {
                subject_certificate,
            } => {
                let handle = self.handle()?;
                handle
                    .prepare_departure(Peer::load(&subject_certificate)?, DEADLINE)
                    .await?;
                let receipt = handle.submit_prepared(DEADLINE).await?;
                if !receipt.accepted {
                    return Err(Error::Denied);
                }
                Ok(json!({"receipt": receipt}))
            }
            Action::Revoke {
                subject_certificate,
            } => {
                let certificate = read(&subject_certificate)?;
                let permissions = Permissions {
                    text: true,
                    files: false,
                    media: false,
                    rooms: true,
                };
                let policy = Registry::open(self.profile_dir.clone())?;
                let contact = Contact {
                    certificate,
                    local_identity: self.identity.peer().fingerprint(),
                    incoming: permissions,
                    outgoing: permissions,
                    endpoints: vec![],
                };
                let peer = contact.peer()?;
                policy.put("revoked-lab-peer", contact)?;
                policy.revoke("revoked-lab-peer")?;
                Ok(json!({"revoked": peer.fingerprint()}))
            }
            Action::Stop => Ok(json!({"stopping": true})),
        }
    }
    async fn cleanup(mut self) -> Result<()> {
        if let Some(enrolling) = self.enrolling.take() {
            enrolling.abort();
            let _ = enrolling.await;
        }
        let detached = self.control.unregister_room();
        let stopped = match self.running.take() {
            Some(running) => running.stop().await,
            None => Ok(()),
        };
        let node = Arc::try_unwrap(self.control)
            .map_err(|_| Error::Node("private fixture leaked control owner"))?;
        let closed = node.shutdown().await;
        detached.and(stopped).and(closed)
    }
}

pub async fn serve(
    config: NodeConfig,
    private_state: PathBuf,
    profile_dir: PathBuf,
    commands: PathBuf,
    member: bool,
    enrollment: Option<EnrollmentInvite>,
) -> Result<()> {
    profile::private_directory(&commands, true)?;
    let identity = Arc::new(Identity::load(&config.context.identity)?);
    Registry::open(profile_dir.clone())?;
    let mut instance = [0u8; 16];
    SystemRandom::new()
        .fill(&mut instance)
        .map_err(|_| Error::Authentication)?;
    let control = Arc::new(start(&config).await?);
    tokio::task::LocalSet::new().run_until(async move {
        let mut fixture = Fixture { instance, config, identity, control, private_state, profile_dir, running: None, received: 0, latest: Value::Null, captured: None, phase: "startup", enrolling: None };
        let result = async {
            if member {
                let (anchor, _) = anchor(&fixture.config.context)?;
                let actor = CryptoMember::open_private(&fixture.identity, &anchor, &fixture.private_state)?;
                let group = Group::restore(actor)?;
                let room = Room::new(fixture.identity.clone(), fixture.control.clone(), group, Registry::open(fixture.profile_dir.clone())?)?;
                fixture.running = Some(Running::start(room, &fixture.control).await?);
            }
            if let Some(enrollment) = enrollment { fixture.invite(enrollment).await?; }
            let mut tick = tokio::time::interval(Duration::from_millis(100));
            loop {
                tokio::select! {
                    signal = tokio::signal::ctrl_c() => { signal?; break; },
                    event = async { match fixture.running.as_mut() {
                        Some(running) => running.events.recv().await,
                        None => std::future::pending().await,
                    }} => {
                        let Some(Event::Text(text)) = event else { return Err(Error::Cancelled); };
                        fixture.received = fixture.received.checked_add(1).ok_or(Error::Protocol)?;
                        fixture.latest = json!({"sender": text.sender, "sha256": digest(text.content.text().as_bytes()),
                            "bytes": text.content.text().len(), "sensitive": text.content.sensitive});
                    },
                    _ = tick.tick() => {
                        let mut stop = false;
                        let mut files = fs::read_dir(&commands)?.take(2 * config::ROOM_COMMAND_QUEUE + 1).collect::<std::io::Result<Vec<_>>>()?;
                        if files.len() > 2 * config::ROOM_COMMAND_QUEUE { return Err(Error::Limit { resource: "lab command directory", max: 2 * config::ROOM_COMMAND_QUEUE }); }
                        files.sort_by_key(|entry| entry.file_name());
                        for entry in files {
                            let name = entry.file_name();
                            let Some(id) = name.to_str().and_then(|name| name.strip_prefix("request-")).and_then(|name| name.strip_suffix(".json")) else { continue; };
                            if id.is_empty() || id.len() > 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) { return Err(Error::Protocol); }
                            let response = commands.join(format!("result-{id}.json"));
                            if response.try_exists()? { continue; }
                            // A restarted instance never replays old pending sends.
                            fixture.phase = "parse-command";
                            let outcome = match json::<Request>(&entry.path()).and_then(|request| request.for_instance(fixture.instance)) {
                                Ok(action) => { stop = matches!(action, Action::Stop); fixture.act(action).await },
                                Err(error) => Err(error),
                            };
                            let value = match outcome { Ok(value) => json!({"ok": true, "value": value}), Err(error) => json!({"ok": false, "phase": fixture.phase, "error": error.to_string()}) };
                            // File existence is the completion signal. A direct
                            // create+write lets the controller see partial JSON.
                            publish_json(&response, &value, false)?;
                            if stop { break; }
                        }
                        publish_json(&fixture.config.status, &fixture.status().await?, true)?;
                        if stop { break; }
                    }
                }
            }
            Ok(())
        }.await;
        let cleanup = fixture.cleanup().await;
        if let Err(error) = &cleanup { eprintln!("Private fixture cleanup failed: {error}"); }
        result.and(cleanup)
    }).await
}
fn digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| {
            [
                HEX[(byte >> 4) as usize] as char,
                HEX[(byte & 15) as usize] as char,
            ]
        })
        .collect()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn old_instance_commands_are_rejected_before_any_action_can_run() {
        let old = Request {
            instance: [0; 16],
            command: Action::Stop,
        };
        assert!(matches!(old.for_instance([1; 16]), Err(Error::Cancelled)));
        let current = Request {
            instance: [1; 16],
            command: Action::Stop,
        };
        assert!(matches!(current.for_instance([1; 16]), Ok(Action::Stop)));
        let instance = [1u8; 16];
        let wire = json!({"instance": instance, "command": {"action": "stop"}});
        let parsed: Request = serde_json::from_value(wire).unwrap();
        assert!(matches!(parsed.for_instance([1; 16]), Ok(Action::Stop)));
    }
    #[test]
    fn complete_results_are_published_atomically_without_overwriting_prior_results() {
        let directory = tempfile::tempdir().unwrap();
        let private = directory.path().join("private");
        profile::private_directory(&private, true).unwrap();
        let out = private.join("result.json");
        publish_json(&out, &json!({"ok": true, "value": 1}), false).unwrap();
        assert!(publish_json(&out, &json!({"ok": false}), false).is_err());
        let value: Value = json(&out).unwrap();
        assert_eq!(value, json!({"ok": true, "value": 1}));
        publish_json(&out, &json!({"status": 2}), true).unwrap();
        let value: Value = json(&out).unwrap();
        assert_eq!(value, json!({"status": 2}));
    }

    #[test]
    fn private_genesis_restores_the_exact_public_creator_and_never_overwrites_state() {
        let directory = tempfile::tempdir().unwrap();
        let identity_path = directory.path().join("identity");
        let identity = Identity::create(&identity_path).unwrap();
        let state = directory.path().join("crypto");
        let public = directory.path().join("public");
        profile::private_directory(&public, true).unwrap();
        let out = public.join("genesis.json");
        genesis(&identity_path, &state, &out).unwrap();
        let first = read(&out).unwrap();
        let document: Genesis = json(&out).unwrap();
        let anchor = document.root.verify(&identity.peer()).unwrap();
        let actor = CryptoMember::open_private(&identity, &anchor, &state).unwrap();
        assert_eq!(
            serde_json::to_vec(&actor.public_identity().unwrap()).unwrap(),
            serde_json::to_vec(&document.creator).unwrap()
        );
        let group = Group::restore(actor).unwrap();
        assert_eq!(group.room_id(), anchor.id());
        assert_eq!(group.epoch(), 0);
        assert!(genesis(&identity_path, &state, &out).is_err());
        assert_eq!(read(&out).unwrap(), first);
        assert_eq!(
            digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
