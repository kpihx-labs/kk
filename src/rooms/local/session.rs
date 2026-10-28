//! Same-user room commands share the node's credential-checked, bounded IPC.
use super::*;
use crate::{
    config,
    node::ipc,
    pairing::{self, Invite, PairServer, Proposal, Scope},
    rooms::{
        Rights, enrollment,
        runtime::{Actor, Event, Handle},
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};
use tokio::{
    net::UnixStream,
    sync::{Mutex, watch},
    task::JoinSet,
};
use zeroize::Zeroize;

const DEADLINE: Duration = Duration::from_secs(90);
// Reserved local control commands are tiny and sent immediately on connect.
// Stalled pre-command readers must not hold admission for the data timeout.
const CONTROL_HANDSHAKE: Duration = Duration::from_millis(250);

async fn cancelled(external: &mut Option<watch::Receiver<bool>>) -> Result<()> {
    loop {
        if let Some(stop) = external {
            if *stop.borrow() {
                return Ok(());
            }
            tokio::select! {
                signal = tokio::signal::ctrl_c() => { signal?; return Ok(()); },
                changed = stop.changed() => if changed.is_err() { return Ok(()); },
            }
        } else {
            tokio::signal::ctrl_c().await?;
            return Ok(());
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvitationOptions {
    pub listen: SocketAddr,
    pub output: PathBuf,
    pub rights: Rights,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
pub enum Command {
    MediaPublish {
        kind: crate::media::Kind,
        peers: Vec<raft::PeerId>,
    },
    MediaListen {
        kind: crate::media::Kind,
        peer: raft::PeerId,
    },
    Status,
    FileReception {
        inbox: Option<PathBuf>,
        max_bytes: u64,
        budget: u64,
    },
    Stop,
    Text {
        text: String,
        sensitive: bool,
    },
    File {
        source: PathBuf,
        target: raft::PeerId,
        sensitive: bool,
    },
    FileShare {
        source: PathBuf,
        audience: BTreeSet<raft::PeerId>,
    },
    FileUnshare {
        hash: [u8; 32],
    },
    FileLocate {
        hash: [u8; 32],
        peers: BTreeSet<raft::PeerId>,
    },
    FileFetch {
        hash: [u8; 32],
        peer: raft::PeerId,
    },
    Controllers {
        voters: BTreeSet<raft::PeerId>,
    },
    Remove {
        member: raft::PeerId,
    },
    Invite(InvitationOptions),
}
impl Command {
    fn deadline(&self) -> Duration {
        if matches!(self, Self::Invite(_)) {
            config::ROOM_INVITATION_WAIT + DEADLINE
        } else if matches!(
            self,
            Self::File { .. } | Self::FileShare { .. } | Self::FileFetch { .. }
        ) {
            crate::config::DEFAULT_FILE_TIMEOUT
        } else {
            DEADLINE
        }
    }
}
impl Drop for Command {
    fn drop(&mut self) {
        if let Self::Text { text, .. } = self {
            text.zeroize();
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub ok: bool,
    pub code: Option<String>,
    pub result: Value,
}
impl Reply {
    fn from_result(result: Result<Value>) -> Self {
        match result {
            Ok(result) => Self {
                ok: true,
                code: None,
                result,
            },
            Err(error) => Self {
                ok: false,
                code: Some(error.code().to_owned()),
                result: json!({"error": error.to_string(), "remote_effects_may_have_committed": true}),
            },
        }
    }
    pub fn check(&self) -> Result<()> {
        if self.ok {
            Ok(())
        } else {
            Err(match self.code.as_deref() {
                Some("DENIED") => Error::Denied,
                Some("REVOKED") => Error::Revoked,
                Some("TIMEOUT") => Error::Timeout,
                Some("AUTHENTICATION") => Error::Authentication,
                Some("NETWORK") => Error::Network,
                Some("PROTOCOL") => Error::Protocol,
                _ => Error::Node("room command failed; inspect the returned report"),
            })
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "event", deny_unknown_fields)]
pub enum ReplyEvent {
    MediaReady {
        peers: Vec<raft::PeerId>,
    },
    Ready {
        address: SocketAddr,
    },
    Confirm {
        fingerprint: String,
        confirmation: String,
    },
    Completed(Reply),
}

pub async fn request(profile: &Path, name: &str, command: &Command) -> Result<Reply> {
    if matches!(command, Command::Invite(_)) {
        return Err(Error::Configuration(
            "invitation requires the confirmation channel",
        ));
    }
    let directory = directory(profile, name)?;
    let deadline = command.deadline();
    let control = directory.join("ctl");
    let reserved = if is_control(command) && control.try_exists()? {
        ipc::exchange(&control, command, deadline + Duration::from_secs(5)).await?
    } else {
        None
    };
    // Older foreground owners have no reserved endpoint. Absence may fall back;
    // an unsafe or unavailable reserved endpoint must never be silently bypassed.
    let event: ReplyEvent = match reserved {
        Some(event) => event,
        None => ipc::exchange(&directory, command, deadline + Duration::from_secs(5))
            .await?
            .ok_or(Error::Node("no active room; run kk room serve"))?,
    };
    match event {
        ReplyEvent::Completed(reply) => Ok(reply),
        _ => Err(Error::Protocol),
    }
}

/// Invitation files are sensitive single-use capabilities, never stdout payloads.
pub async fn invite<F, Fut>(
    profile: &Path,
    name: &str,
    options: InvitationOptions,
    mut confirm: F,
) -> Result<Reply>
where
    F: FnMut(String, String) -> Fut,
    Fut: std::future::Future<Output = Result<bool>>,
{
    let directory = directory(profile, name)?;
    let command = Command::Invite(options);
    tokio::time::timeout(command.deadline() + Duration::from_secs(5), async {
        let mut stream = ipc::connect(&directory)
            .await?
            .ok_or(Error::Node("no active room"))?;
        ipc::write(&mut stream, &command).await?;
        loop {
            match ipc::read(&mut stream).await? {
                ReplyEvent::Ready { address } => eprintln!("ROOM INVITATION {address}"),
                ReplyEvent::Confirm {
                    fingerprint,
                    confirmation,
                } => {
                    let approved = confirm(fingerprint, confirmation).await?;
                    ipc::write(&mut stream, &approved).await?;
                }
                ReplyEvent::Completed(reply) => return Ok(reply),
                ReplyEvent::MediaReady { .. } => return Err(Error::Protocol),
            }
        }
    })
    .await
    .map_err(|_| Error::Timeout)?
}

async fn offer(stream: &Arc<Mutex<UnixStream>>, proposal: Proposal) -> Result<bool> {
    let mut stream = stream.lock().await;
    ipc::write(
        &mut stream,
        &ReplyEvent::Confirm {
            fingerprint: proposal.fingerprint,
            confirmation: proposal.confirmation,
        },
    )
    .await?;
    ipc::read(&mut stream).await
}

async fn dispatch(
    command: &Command,
    stream: &Arc<Mutex<UnixStream>>,
    handle: &Handle,
    control: &ControlNode,
    identity: &Identity,
) -> Result<Value> {
    match command {
        Command::Status => {
            let status = handle.status(DEADLINE).await?;
            // Pass one complete membership snapshot, never a filtered recipient
            // list. This schedules refresh work without awaiting peer traffic.
            let (profile_names, profile_names_error) =
                match control.cached_peer_profiles(status.epoch, &status.members) {
                    Ok(names) => (names, None),
                    Err(error) => (std::collections::BTreeMap::new(), Some(error.to_string())),
                };
            Ok(
                json!({"room": raft::PeerId(status.room), "owner": status.owner, "epoch": status.epoch,
                "active": status.active, "running": status.running, "busy": status.busy, "members": status.members,
                "freshness": "local-durable-cache", "leader_hint": control.current_leader().await, "media": control.media_status()?,
                  "files": {"accepted": control.files_enabled()?}, "profile_names": profile_names, "profile_names_error": profile_names_error,
                 "voters": control.control_membership().membership().voter_ids().collect::<Vec<_>>(), "address": control.local_addr()?}),
            )
        }
        Command::Stop => Ok(json!({"stopping": true})),
        Command::FileReception {
            inbox,
            max_bytes,
            budget,
        } => {
            let accepted = control.configure_files(inbox.clone(), *max_bytes, *budget)?;
            Ok(json!({"accepted": accepted}))
        }
        Command::FileShare { source, audience } => {
            let info = control.share_file(source, audience.clone()).await?;
            Ok(
                json!({"sha256": info.hash_hex(), "bytes": info.bytes, "audience": audience,
                "lifetime": "this-service-and-membership-epoch-until-withdrawal", "meaning": "explicit-offer-not-a-future-availability-promise"}),
            )
        }
        Command::FileUnshare { hash } => Ok(json!({"withdrawn": control.unshare_file(*hash)?})),
        Command::FileLocate { hash, peers } => {
            if peers.is_empty() || peers.len() >= config::ROOM_MAX_MEMBERS {
                return Err(Error::Configuration(
                    "choose a bounded exact set of replica peers",
                ));
            }
            let replies = futures_util::future::join_all(peers.iter().map(|peer| async move {
                (
                    *peer,
                    control
                        .file_availability(*peer, *hash, config::DEFAULT_TIMEOUT)
                        .await,
                )
            }))
            .await;
            let mut advertised = Vec::new();
            let mut unavailable = Vec::new();
            let mut failed = Vec::new();
            for (peer, reply) in replies {
                match reply {
                    Ok(Some(info)) => advertised
                        .push(json!({"peer":peer,"sha256":info.hash_hex(),"bytes":info.bytes})),
                    Ok(None) => unavailable.push(peer),
                    Err(error) => failed.push(json!({"peer":peer,"code":error.code()})),
                }
            }
            Ok(
                json!({"advertised":advertised,"unavailable":unavailable,"failed":failed,"all_responded":failed.is_empty(),
                "meaning":"explicit-audience-hints-not-verified-current-storage"}),
            )
        }
        Command::FileFetch { hash, peer } => {
            let receipt = control
                .fetch_file(*peer, *hash, config::DEFAULT_FILE_TIMEOUT)
                .await?;
            Ok(
                json!({"peer":peer,"sha256":receipt.info.hash_hex(),"bytes":receipt.info.bytes,"resumed_from":receipt.resumed_from,
                "meaning":"verified-durable-local-copy-from-selected-replica"}),
            )
        }
        Command::MediaPublish { .. } | Command::MediaListen { .. } => Err(Error::Protocol),
        Command::File {
            source,
            target,
            sensitive,
        } => {
            let receipt = control
                .send_room_file(
                    *target,
                    source,
                    *sensitive,
                    crate::config::DEFAULT_FILE_TIMEOUT,
                )
                .await?;
            Ok(
                json!({"bytes": receipt.info.bytes, "resumed_from": receipt.resumed_from,
                "sha256": if *sensitive { None } else { Some(receipt.info.hash_hex()) }, "sensitive": sensitive,
                "meaning": "verified-durable-recipient-file-not-a-replica-availability-promise"}),
            )
        }
        Command::Controllers { voters } => {
            let position = handle.approve_controllers(voters.clone(), DEADLINE).await?;
            Ok(json!({"committed": position}))
        }
        Command::Remove { member } => {
            let peer = control
                .public_view()
                .await?
                .peers()
                .into_iter()
                .find(|peer| raft::PeerId::from_peer(peer) == *member)
                .ok_or(Error::Denied)?;
            let proposal = handle.prepare_departure(peer, DEADLINE).await?;
            let applied = handle.submit_prepared(DEADLINE).await?;
            if !applied.accepted {
                return Err(Error::Denied);
            }
            Ok(json!({"proposal": proposal.id()?, "epoch": applied.epoch,
                "meaning": "data-removal-not-controller-reconfiguration"}))
        }
        Command::Text { text, sensitive } => {
            let publication = handle
                .publication(text.clone(), *sensitive, Duration::from_secs(60))
                .await?;
            let mut acknowledged = Vec::new();
            let mut failed = Vec::new();
            let deliveries = publication
                .members
                .into_iter()
                .filter(|target| *target != control.id())
                .map(|target| {
                    let wire = &publication.wire;
                    async move {
                        (
                            target,
                            control
                                .send_room_text(target, wire, crate::config::DEFAULT_TIMEOUT)
                                .await,
                        )
                    }
                });
            for (target, result) in futures_util::future::join_all(deliveries).await {
                match result {
                    Ok(()) => acknowledged.push(target),
                    Err(error) => failed.push(json!({"peer": target, "code": error.code()})),
                }
            }
            Ok(
                json!({"bytes": text.len(), "acknowledged": acknowledged, "failed": failed,
                "all_acknowledged": failed.is_empty(), "ack_meaning": "durable-replay-processing-and-event-queue-not-human-reading"}),
            )
        }
        Command::Invite(options) => {
            let server = PairServer::bind_room(
                options.listen,
                config::ROOM_INVITATION_WAIT,
                control.room_id(),
            )
            .await?;
            Invite::from_server(&server, identity)?.export(&options.output)?;
            ipc::write(
                &mut *stream.lock().await,
                &ReplyEvent::Ready {
                    address: server.local_addr()?,
                },
            )
            .await?;
            let channel = stream.clone();
            let session = server
                .run_room_session(
                    identity,
                    control.local_addr()?.port(),
                    move |proposal| {
                        let channel = channel.clone();
                        async move { offer(&channel, proposal).await }
                    },
                    |_| Ok(()),
                )
                .await?;
            let admission = enrollment::admit(session, handle, options.rights, DEADLINE).await?;
            Ok(
                json!({"admission": admission, "meaning": "committed-and-replicated-not-a-vote-or-remote-activation"}),
            )
        }
    }
}

fn is_control(command: &Command) -> bool {
    matches!(
        command,
        Command::Status
            | Command::Stop
            | Command::FileUnshare { .. }
            | Command::FileReception { inbox: None, .. }
    )
}

fn operation_lane(
    command: &Command,
    work: &Arc<tokio::sync::Semaphore>,
    invitations: &Arc<tokio::sync::Semaphore>,
) -> Result<Option<tokio::sync::OwnedSemaphorePermit>> {
    if is_control(command) {
        return Ok(None);
    }
    match command {
        Command::Invite(_) => invitations
            .clone()
            .try_acquire_owned()
            .map(Some)
            .map_err(|_| Error::Node("another room invitation is active")),
        _ => work
            .clone()
            .try_acquire_owned()
            .map(Some)
            .map_err(|_| Error::Node("room operation lane is full")),
    }
}

struct Incoming {
    stream: UnixStream,
    control_only: bool,
}

async fn connection(
    incoming: Incoming,
    handle: Handle,
    control: Arc<ControlNode>,
    identity: Arc<Identity>,
    stop: watch::Sender<bool>,
    work: Arc<tokio::sync::Semaphore>,
    invitations: Arc<tokio::sync::Semaphore>,
) -> Result<()> {
    let Incoming {
        mut stream,
        control_only,
    } = incoming;
    let handshake = if control_only {
        CONTROL_HANDSHAKE
    } else {
        crate::config::DEFAULT_TIMEOUT
    };
    let command: Command = tokio::time::timeout(handshake, ipc::read(&mut stream))
        .await
        .map_err(|_| Error::Timeout)??;
    if control_only && !is_control(&command) {
        return Err(Error::Denied);
    }
    match &command {
        Command::MediaPublish { kind, peers } => {
            return super::media::publish(stream, control, *kind, peers.clone()).await;
        }
        Command::MediaListen { kind, peer } => {
            return super::media::listen(stream, control, *kind, *peer).await;
        }
        _ => {}
    }
    let stream = Arc::new(Mutex::new(stream));
    let operation = async {
        let _lane = operation_lane(&command, &work, &invitations)?;
        dispatch(&command, &stream, &handle, &control, &identity).await
    };
    let deadline = command.deadline();
    let result = tokio::time::timeout(deadline, operation)
        .await
        .unwrap_or(Err(Error::Timeout));
    let mut reply = Reply::from_result(result);
    if reply.result.get("all_acknowledged") == Some(&Value::Bool(false)) {
        reply.ok = false;
        reply.code = Some("PARTIAL_DELIVERY".into());
    }
    if reply.result.get("all_responded") == Some(&Value::Bool(false)) {
        reply.ok = false;
        reply.code = Some("PARTIAL_AVAILABILITY".into());
    }
    let delivered = tokio::time::timeout(
        crate::config::DEFAULT_TIMEOUT,
        ipc::write(&mut *stream.lock().await, &ReplyEvent::Completed(reply)),
    )
    .await
    .map_err(|_| Error::Timeout)?;
    // A bounded status observer may leave before its read-only reply. Other
    // commands keep delivery errors visible because their effects may commit.
    if matches!(command, Command::Status)
        && matches!(&delivered, Err(Error::Io(error)) if matches!(error.kind(), std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset))
    {
        return Ok(());
    }
    delivered?;
    if matches!(command, Command::Stop) {
        let _ = stop.send(true);
    }
    Ok(())
}

async fn run(
    identity: Arc<Identity>,
    profile: &Path,
    directory: &Path,
    room: Room,
    control: Arc<ControlNode>,
    listener: ipc::Listener,
    options: Options,
) -> Result<()> {
    let control_directory = directory.join("ctl");
    crate::profile::private_directory(&control_directory, true)?;
    let control_listener = ipc::Listener::bind(&control_directory).await?;
    let (handle, actor, mut events) = Actor::new(room);
    let task = tokio::task::spawn_local(actor.run());
    let registered = control.register_room(handle.clone()).await;
    let configured = control
        .configure_profile_directory(Some(profile.to_owned()))
        .and_then(|()| match options.inbox {
            Some(inbox) => control.enable_files(inbox, options.max_bytes, options.file_budget),
            None => Ok(()),
        });
    let (event_sink, ready, mut external) = match options.observer {
        Some(observer) => (Some(observer.events), observer.ready, Some(observer.stop)),
        None => (None, None, None),
    };
    let mut renderer = tokio::task::spawn_local(async move {
        while let Some(event) = events.recv().await {
            if let Some(sink) = &event_sink {
                sink.send(event)
                    .await
                    .map_err(|_| Error::Node("room foreground event consumer closed"))?;
                continue;
            }
            let Event::Text(text) = event;
            let line = if text.content.sensitive && !options.reveal {
                format!(
                    "ROOM TEXT {} [sensitive, {} bytes]",
                    text.sender,
                    text.content.text().len()
                )
            } else {
                format!(
                    "ROOM TEXT {} {}",
                    text.sender,
                    text.content.text().escape_debug()
                )
            };
            crate::output::line(line).await?;
        }
        Ok::<(), Error>(())
    });
    let mut connections = JoinSet::new();
    let mut controls = JoinSet::new();
    let work = Arc::new(tokio::sync::Semaphore::new(4));
    let invitations = Arc::new(tokio::sync::Semaphore::new(1));
    let (stop, mut stopped) = watch::channel(false);
    let mut renderer_joined = false;
    let result = async {
        registered?;
        configured?;
        if let Some(ready) = ready { let _ = ready.send(()); }
        crate::output::line(format!("ROOM SERVING {} {}", directory.file_name().ok_or(Error::Protocol)?.to_string_lossy(), control.local_addr()?)).await?;
        loop {
            tokio::select! {
                signal = cancelled(&mut external) => { signal?; break; },
                completed = &mut renderer => {
                    renderer_joined = true;
                    return match completed {
                        Ok(Err(error)) => Err(error),
                        _ => Err(Error::Node("room event output terminated")),
                    };
                },
                changed = stopped.changed() => { changed.map_err(|_| Error::Cancelled)?; if *stopped.borrow() { break; } },
                completed = connections.join_next(), if !connections.is_empty() => {
                    match completed {
                        Some(Ok(Ok(()))) => {},
                        Some(Ok(Err(error))) => eprintln!("Room local command failed: {error}"),
                        Some(Err(error)) => return Err(Error::Io(std::io::Error::other(error))),
                        None => {},
                    }
                },
                completed = controls.join_next(), if !controls.is_empty() => {
                    match completed {
                        Some(Ok(Ok(()))) => {},
                        Some(Ok(Err(error))) => eprintln!("Room local control failed: {error}"),
                        Some(Err(error)) => return Err(Error::Io(std::io::Error::other(error))),
                        None => {},
                    }
                },
                accepted = control_listener.accept(), if controls.len() < 4 => {
                    controls.spawn_local(connection(Incoming { stream: accepted?, control_only: true }, handle.clone(), control.clone(), identity.clone(), stop.clone(), work.clone(), invitations.clone()));
                },
                accepted = listener.accept(), if connections.len() < 32 => {
                    connections.spawn_local(connection(Incoming { stream: accepted?, control_only: false }, handle.clone(), control.clone(), identity.clone(), stop.clone(), work.clone(), invitations.clone()));
                },
            }
        }
        Ok(())
    }.await;
    connections.abort_all();
    while connections.join_next().await.is_some() {}
    controls.abort_all();
    while controls.join_next().await.is_some() {}
    let unregistered = control.unregister_room();
    let stopped = handle.shutdown(DEADLINE).await;
    if stopped.is_err() {
        task.abort();
    }
    let joined = task
        .await
        .map(drop)
        .map_err(|_| Error::Node("room actor terminated unexpectedly"));
    if !renderer_joined {
        renderer.abort();
        let _ = renderer.await;
    }
    drop(handle);
    let shutdown = match Arc::try_unwrap(control) {
        Ok(node) => node.shutdown().await,
        Err(_) => Err(Error::Node("room retained a control owner")),
    };
    if let Err(error) = &shutdown {
        eprintln!("Room control cleanup failed: {error}");
    }
    result
        .and(unregistered)
        .and(stopped)
        .and(joined)
        .and(shutdown)
}

/// Caller supplies a LocalSet, keeping the SDK on its creating thread.
pub async fn serve(profile: &Path, name: &str, options: Options) -> Result<()> {
    let identity = Arc::new(profile::identity(profile, false)?);
    let directory = directory(profile, name)?;
    let _lease = lease(&directory)?;
    let definition = Definition::load(&directory, &identity)?;
    let listener = ipc::Listener::bind(&directory).await?;
    let (room, control) = definition
        .resume(identity.clone(), &directory, profile, options.bind)
        .await?;
    run(
        identity, profile, &directory, room, control, listener, options,
    )
    .await
}

pub async fn join<F, Fut>(
    profile: &Path,
    name: &str,
    invitation: &Path,
    listen: SocketAddr,
    rights: Rights,
    service: Options,
    mut confirm: F,
) -> Result<()>
where
    F: FnMut(String, String) -> Fut,
    Fut: std::future::Future<Output = Result<bool>>,
{
    let identity = Arc::new(profile::identity(profile, false)?);
    let invite = Invite::load(invitation).await?;
    let Scope::Room(room_id) = invite.scope else {
        return Err(Error::Pairing);
    };
    let (directory, _lease) = reserve(profile, name, listen, true)?;
    let listener = ipc::Listener::bind(&directory).await?;
    let options = pairing::RoomJoinOptions {
        room: room_id,
        address: invite.address,
        expected_id: Some(invite.id),
        expected_fingerprint: Some(invite.fingerprint.clone()),
        code: invite.code().clone(),
        data_port: listen.port(),
        deadline: config::ROOM_INVITATION_WAIT,
    };
    let mut external = service
        .observer
        .as_ref()
        .map(|observer| observer.stop.clone());
    let ceremony = async {
        let session = pairing::join_room_session(
            options,
            &identity,
            |proposal| confirm(proposal.fingerprint, proposal.confirmation),
            |_| Ok(()),
        )
        .await?;
        enrollment::prepare(
            session,
            identity.clone(),
            enrollment::CandidateOptions {
                private_state: directory.join("private"),
                control_state: directory.join("control"),
                profile: profile.to_owned(),
                listen,
                rights,
                deadline: DEADLINE,
            },
        )
        .await
    };
    let prepared = tokio::select! {
        signal = cancelled(&mut external) => { signal?; return Err(Error::Cancelled); },
        prepared = ceremony => prepared?,
    };
    Definition {
        version: 1,
        owner: raft::PeerId::from_peer(&identity.peer()),
        room: raft::PeerId(room_id),
        root: prepared.authority().anchor().bundle(),
        origin: prepared.authority().creator().clone(),
        listen,
        inviter: Some(prepared.inviter().certificate_der().to_vec()),
        seeds: prepared.authority().seeds().to_vec(),
    }
    .save(&directory)?;
    let joined = prepared.join_until(cancelled(&mut external)).await?;
    run(
        identity.clone(),
        profile,
        &directory,
        joined.room,
        joined.control,
        listener,
        service,
    )
    .await
}

#[cfg(test)]
mod lane_tests {
    use super::*;

    #[test]
    fn invitation_deadlines_cover_wait_and_bounded_admission_only() {
        let invitation = Command::Invite(InvitationOptions {
            listen: "127.0.0.1:12345".parse().unwrap(),
            output: PathBuf::from("invite.json"),
            rights: Rights::PARTICIPANT,
        });
        assert_eq!(config::ROOM_INVITATION_WAIT, Duration::from_secs(300));
        assert!(config::ROOM_INVITATION_WAIT <= config::MAX_TIMEOUT);
        assert_eq!(
            invitation.deadline(),
            config::ROOM_INVITATION_WAIT + DEADLINE
        );
        let client_deadline = invitation.deadline() + Duration::from_secs(5);
        assert!(client_deadline > invitation.deadline());
        assert_eq!(Command::Status.deadline(), Duration::from_secs(90));
        assert_eq!(
            Command::FileFetch {
                hash: [0; 32],
                peer: raft::PeerId([0; 32])
            }
            .deadline(),
            config::DEFAULT_FILE_TIMEOUT
        );
    }

    #[test]
    fn withdrawal_bypasses_saturated_work_and_invitation_lanes() {
        let work = Arc::new(tokio::sync::Semaphore::new(4));
        let invitations = Arc::new(tokio::sync::Semaphore::new(1));
        let _busy = work.clone().try_acquire_many_owned(4).unwrap();
        let _invitation = invitations.clone().try_acquire_owned().unwrap();
        let mut command = Command::FileReception {
            inbox: None,
            max_bytes: 1,
            budget: 1,
        };
        assert!(
            operation_lane(&command, &work, &invitations)
                .unwrap()
                .is_none()
        );
        if let Command::FileReception { inbox, .. } = &mut command {
            *inbox = Some(PathBuf::from("inbox"));
        }
        assert!(operation_lane(&command, &work, &invitations).is_err());
    }
}
