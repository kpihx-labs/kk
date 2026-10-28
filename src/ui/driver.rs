//! Foreground LocalSet owner. HTTP adapters never move private MLS state into
//! Send handlers or reimplement membership, pairing or authorization decisions.
use super::*;
use crate::{
    Peer,
    rooms::{
        Rights,
        control::raft::PeerId,
        local::{self, Command, Observer},
        runtime::Event,
    },
};
use futures_util::FutureExt;
use std::{future::Future, pin::Pin};
use tokio::task::JoinSet;

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Role {
    Reader,
    #[default]
    Participant,
    Inviter,
    Moderator,
}
impl Role {
    fn rights(self) -> Rights {
        match self {
            Self::Reader => Rights::READER,
            Self::Participant => Rights::PARTICIPANT,
            Self::Inviter => Rights::INVITER,
            Self::Moderator => Rights::MODERATOR,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Action {
    #[serde(skip)]
    Transfer(super::files::Transfer),
    Copies {
        name: String,
        command: super::files::CopyAction,
    },
    Init,
    Settings {
        expected_revision: String,
        #[serde(default)]
        display_name: Option<String>,
        #[serde(default)]
        appearance: Option<crate::profile::Appearance>,
        #[serde(default)]
        share_display_name: Option<bool>,
        #[serde(default)]
        sidebar_mode: Option<crate::profile::SidebarMode>,
    },
    Profiles,
    ProfileCreate {
        id: String,
        display_name: String,
    },
    ProfileRegister {
        id: String,
        directory: PathBuf,
    },
    ProfileSelect {
        id: String,
    },
    ProfileUnregister {
        id: String,
    },
    MemberLabel {
        name: String,
        member: PeerId,
        display_name: Option<String>,
    },
    FileConsent {
        name: String,
        accepted: bool,
    },
    Create {
        name: String,
        title: String,
        bind: SocketAddr,
    },
    Serve {
        name: String,
        #[serde(default)]
        bind: Option<SocketAddr>,
        #[serde(default)]
        accept_files: bool,
    },
    Stop {
        name: String,
    },
    Send {
        name: String,
        text: String,
        #[serde(default)]
        sensitive: bool,
    },
    Invite {
        name: String,
        bind: SocketAddr,
        #[serde(default)]
        role: Role,
    },
    Join {
        name: String,
        invitation: String,
        bind: SocketAddr,
        #[serde(default)]
        role: Role,
        #[serde(default)]
        accept_files: bool,
    },
    Confirm {
        id: u64,
        accept: bool,
    },
    Controllers {
        name: String,
        peers: Vec<PeerId>,
    },
    Remove {
        name: String,
        peer: PeerId,
    },
    Reveal {
        id: u64,
    },
    Shutdown,
}
pub(super) struct Request {
    pub action: Action,
    pub reply: oneshot::Sender<Result<Value>>,
}

fn next(data: &mut Data) -> Result<u64> {
    data.next = data
        .next
        .checked_add(1)
        .ok_or(Error::Node("UI event sequence exhausted"))?;
    Ok(data.next)
}

#[derive(Deserialize)]
struct Membership {
    room: PeerId,
    owner: PeerId,
    members: Vec<PeerId>,
}

fn membership(value: &Value, room: PeerId, own: PeerId) -> Result<Membership> {
    let status: Membership = serde_json::from_value(value.clone())
        .map_err(|_| Error::Configuration("invalid room membership status"))?;
    if status.room != room || status.owner != own || !status.members.contains(&own) {
        return Err(Error::Configuration(
            "room membership does not match this profile",
        ));
    }
    Ok(status)
}
fn phase(app: &App, room: &str, state: &str, error: Option<String>) {
    if let Ok(mut data) = app.0.data.lock() {
        data.phases
            .insert(room.into(), json!({"state": state, "error": error}));
    }
}
fn message(
    app: &App,
    room: &str,
    sender: String,
    text: &str,
    sensitive: bool,
    outgoing: bool,
) -> Result<u64> {
    let mut data = app
        .0
        .data
        .lock()
        .map_err(|_| Error::Node("UI event store is unavailable"))?;
    let id = next(&mut data)?;
    data.messages.push_back(Message {
        id,
        room: room.into(),
        sender,
        text: Zeroizing::new(text.to_owned()),
        sensitive,
        outgoing,
    });
    while data.messages.len() > config::UI_HISTORY_LIMIT {
        data.messages.pop_front();
    }
    Ok(id)
}
struct RunOptions {
    options: local::Options,
    stop: watch::Sender<bool>,
    ready: oneshot::Receiver<()>,
    events: mpsc::Receiver<Event>,
}
fn options(app: &App, name: &str, bind: Option<SocketAddr>, files: bool) -> Result<RunOptions> {
    let (stop, stopped) = watch::channel(false);
    let (ready, initialized) = oneshot::channel();
    let (events, receive) = mpsc::channel(config::ROOM_EVENT_QUEUE);
    let inbox = if files {
        Some(local::directory(&app.0.profile, name)?.join("inbox"))
    } else {
        None
    };
    Ok(RunOptions {
        options: local::Options {
            bind,
            reveal: false,
            inbox,
            max_bytes: config::DEFAULT_FILE_LIMIT,
            file_budget: config::NODE_FILE_BUDGET,
            observer: Some(Observer {
                ready: Some(ready),
                events,
                stop: stopped,
            }),
        },
        stop,
        ready: initialized,
        events: receive,
    })
}
async fn monitor(
    app: App,
    name: String,
    operation: Option<u64>,
    mut ready: oneshot::Receiver<()>,
    mut events: mpsc::Receiver<Event>,
    mut service: Pin<Box<dyn Future<Output = Result<()>>>>,
) -> String {
    let mut observed = false;
    let mut open = true;
    let result = loop {
        tokio::select! {
            result = &mut service => break result,
            result = &mut ready, if !observed => {
                observed = true;
                if result.is_ok() {
                    phase(&app, &name, "online", None);
                    if let Some(id) = operation && let Ok(mut data) = app.0.data.lock() && let Some(operation) = data.operations.get_mut(&id) { operation.state = "joined"; operation.proposal = None; }
                }
            },
            event = events.recv(), if open => match event {
                Some(Event::Text(text)) => { if let Err(error) = message(&app, &name, text.sender, text.content.text(), text.content.sensitive, false) { break Err(error); } },
                None => open = false,
            },
        }
    };
    phase(
        &app,
        &name,
        "stopped",
        result.as_ref().err().map(ToString::to_string),
    );
    if let Some(id) = operation
        && let Ok(mut data) = app.0.data.lock()
        && let Some(operation) = data.operations.get_mut(&id)
    {
        operation.state = if result.is_ok() { "stopped" } else { "failed" };
        operation.error = result.err().map(|error| error.to_string());
        operation.confirm.take();
    }
    name
}
fn operation(app: &App, room: &str, issuing: bool) -> Result<(u64, PathBuf)> {
    let mut data = app
        .0
        .data
        .lock()
        .map_err(|_| Error::Node("UI operation store is unavailable"))?;
    if data.operations.len() >= config::UI_OPERATION_LIMIT {
        data.operations.retain(|_, operation| {
            !matches!(
                operation.state,
                "joined" | "stopped" | "failed" | "complete"
            )
        });
    }
    if data.operations.len() >= config::UI_OPERATION_LIMIT {
        return Err(Error::Node("too many pending UI ceremonies"));
    }
    let id = next(&mut data)?;
    let directory = tempfile::Builder::new()
        .prefix("ceremony-")
        .tempdir_in(app.0.profile.join("ui"))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let bundle = directory.path().join("invitation.json");
    data.operations.insert(
        id,
        Operation {
            room: room.into(),
            issuing,
            state: "waiting",
            proposal: None,
            confirm: None,
            _directory: directory,
            bundle: bundle.clone(),
            error: None,
        },
    );
    Ok((id, bundle))
}
async fn confirm(app: App, id: u64, fingerprint: String, confirmation: String) -> Result<bool> {
    let (send, receive) = oneshot::channel();
    {
        let mut data = app
            .0
            .data
            .lock()
            .map_err(|_| Error::Node("UI confirmation store is unavailable"))?;
        let operation = data.operations.get_mut(&id).ok_or(Error::Cancelled)?;
        operation.state = "confirmation";
        operation.proposal =
            Some(json!({"fingerprint": fingerprint, "confirmation": confirmation}));
        operation.confirm = Some(send);
    }
    tokio::time::timeout(config::MAX_TIMEOUT, receive)
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|_| Error::Cancelled)
}

pub(super) async fn run(app: App, mut requests: mpsc::Receiver<Request>) -> Result<()> {
    let mut stopped = app.0.stop.subscribe();
    let mut rooms = JoinSet::new();
    let mut invitations = JoinSet::new();
    let mut transfers = JoinSet::new();
    let mut managed: BTreeMap<String, watch::Sender<bool>> = BTreeMap::new();
    let mut failure = None;
    loop {
        if *stopped.borrow() {
            break;
        }
        let request = tokio::select! {
            changed = stopped.changed() => { let _ = changed; break; },
            room = rooms.join_next(), if !rooms.is_empty() => {
                match room { Some(Ok(name)) => { managed.remove(&name); }, Some(Err(error)) => { eprintln!("UI room owner failed: {error}"); failure = Some(Error::Node("UI room owner failed")); break; }, None => {} }
                continue;
            },
            operation = invitations.join_next(), if !invitations.is_empty() => {
                if let Some(Err(error)) = operation { eprintln!("UI ceremony owner failed: {error}"); failure = Some(Error::Node("UI ceremony owner failed")); break; }
                continue;
            },
            transfer = transfers.join_next(), if !transfers.is_empty() => {
                if let Some(Err(error)) = transfer { eprintln!("UI file owner failed: {error}"); failure = Some(Error::Node("UI file owner failed")); break; }
                continue;
            },
            request = requests.recv() => match request { Some(request) => request, None => break },
        };
        let action = match request.action {
            Action::Copies { name, command } => {
                let current = app.clone();
                transfers.spawn_local(async move {
                    let _ = request.reply.send(command.run(current, &name).await);
                });
                continue;
            }
            Action::Transfer(transfer) => {
                let current = app.clone();
                transfers.spawn_local(async move {
                    let _ = request.reply.send(transfer.run(current).await);
                });
                continue;
            }
            action => action,
        };
        let result: Result<Value> = async {
            match action {
                Action::Transfer(_) => unreachable!("owned transfers run in their bounded task lane"),
                Action::Copies { .. } => unreachable!("owned copy operations run in their bounded task lane"),
                Action::Init => Ok(json!({"identity": crate::profile::identity(&app.0.profile, true)?.peer().fingerprint()})),
                Action::Settings { expected_revision, display_name, appearance, share_display_name, sidebar_mode } => Ok(json!(crate::profile::Settings::update_with_revision(&app.0.profile, crate::profile::Settings::parse_revision(&expected_revision)?, |settings| {
                    if let Some(name) = display_name { settings.display_name = name; }
                    if let Some(appearance) = appearance { settings.appearance = appearance; }
                    if let Some(share) = share_display_name { settings.share_display_name = share; }
                    if let Some(mode) = sidebar_mode { settings.sidebar_mode = mode; }
                    Ok(())
                })?)),
                Action::Profiles => Ok(json!(crate::profile::catalog::list()?)),
                Action::ProfileCreate { id, display_name } => Ok(json!(crate::profile::catalog::create(&id, display_name, None)?)),
                Action::ProfileRegister { id, directory } => Ok(json!(crate::profile::catalog::register(&id, directory)?)),
                Action::ProfileSelect { id } => { crate::profile::catalog::select(&id)?; Ok(json!({"restart_required": true})) },
                Action::ProfileUnregister { id } => {
                    if crate::profile::catalog::resolve(&id)? == app.0.profile { return Err(Error::Configuration("cannot unregister the profile running this UI")); }
                    crate::profile::catalog::remove(&id)?;
                    Ok(json!({"unregistered": true}))
                },
                Action::MemberLabel { name, member, display_name } => {
                    let summary = local::list(&app.0.profile)?.into_iter().find(|room| room.name == name)
                        .ok_or(Error::Configuration("unknown local room"))?;
                    let room = summary.room.filter(|_| summary.problem.is_none()).ok_or(Error::Configuration("room needs recovery"))?;
                    let own = PeerId::from_peer(&Peer::load(&app.0.profile.join("identity/certificate.der"))?);
                    let reply = local::request(&app.0.profile, &name, &Command::Status).await?;
                    reply.check()?;
                    let status = membership(&reply.result, room, own)?;
                    if member == own || !status.members.contains(&member) { return Err(Error::Configuration("choose another current room member")); }
                    crate::profile::labels::set(&app.0.profile, room.0, member, display_name)?;
                    Ok(json!({"updated": true}))
                },
                Action::FileConsent { name, accepted } => {
                    let inbox = if accepted { Some(local::directory(&app.0.profile, &name)?.join("inbox")) } else { None };
                    let reply = local::request(&app.0.profile, &name, &Command::FileReception {
                        inbox, max_bytes: config::DEFAULT_FILE_LIMIT, budget: config::NODE_FILE_BUDGET,
                    }).await?;
                    reply.check()?;
                    Ok(reply.result)
                },
                Action::Create { name, title, bind } => Ok(json!({"room": local::create(&app.0.profile, &name, &title, bind).await?})),
                Action::Serve { name, bind, accept_files } => {
                    if managed.contains_key(&name) { return Err(Error::Node("this room already has a foreground UI owner")); }
                    let RunOptions { options, stop, ready, events } = options(&app, &name, bind, accept_files)?;
                    phase(&app, &name, "starting", None);
                    managed.insert(name.clone(), stop);
                    let profile = app.0.profile.clone();
                    let target = name.clone();
                    let service = async move { local::serve(&profile, &target, options).await }.boxed_local();
                    rooms.spawn_local(monitor(app.clone(), name, None, ready, events, service));
                    Ok(json!({"starting": true}))
                },
                Action::Stop { name } => {
                    if let Some(stop) = managed.get(&name) { let _ = stop.send(true); phase(&app, &name, "stopping", None); }
                    else { local::request(&app.0.profile, &name, &Command::Stop).await?.check()?; }
                    Ok(json!({"stopping": true}))
                },
                Action::Send { name, text, sensitive } => {
                    let text = Zeroizing::new(text);
                    let reply = local::request(&app.0.profile, &name, &Command::Text { text: text.to_string(), sensitive }).await?;
                    let peer = Peer::load(&app.0.profile.join("identity/certificate.der"))?;
                    let id = message(&app, &name, peer.fingerprint(), &text, sensitive, true)?;
                    Ok(json!({"message": id, "delivery": reply}))
                },
                Action::Controllers { name, peers } => {
                    let peers = peers.into_iter().collect();
                    let reply = local::request(&app.0.profile, &name, &Command::Controllers { voters: peers }).await?;
                    reply.check()?;
                    Ok(reply.result)
                },
                Action::Remove { name, peer } => {
                    let reply = local::request(&app.0.profile, &name, &Command::Remove { member: peer }).await?;
                    reply.check()?;
                    Ok(reply.result)
                },
                Action::Invite { name, bind, role } => {
                    let (id, bundle) = operation(&app, &name, true)?;
                    let current = app.clone();
                    invitations.spawn_local(async move {
                        let expected = current.clone();
                        let result = local::invite(&current.0.profile, &name, local::InvitationOptions { listen: bind, output: bundle, rights: role.rights() }, move |fingerprint, confirmation| confirm(expected.clone(), id, fingerprint, confirmation)).await;
                        if let Ok(mut data) = current.0.data.lock() && let Some(operation) = data.operations.get_mut(&id) {
                            operation.confirm.take();
                            operation.proposal = None;
                            match result { Ok(reply) if reply.ok => operation.state = "complete", Ok(reply) => { operation.state = "failed"; operation.error = Some(reply.result.to_string()); }, Err(error) => { operation.state = "failed"; operation.error = Some(error.to_string()); } }
                        }
                        id
                    });
                    Ok(json!({"operation": id}))
                },
                Action::Join { name, invitation, bind, role, accept_files } => {
                    if managed.contains_key(&name) { return Err(Error::Node("this room already has a foreground UI owner")); }
                    let invitation = Zeroizing::new(invitation);
                    if invitation.len() > 4096 { return Err(Error::Pairing); }
                    let (id, bundle) = operation(&app, &name, false)?;
                    let setup = async {
                        let mut file = std::fs::OpenOptions::new();
                        use std::os::unix::fs::OpenOptionsExt;
                        let mut file = file.write(true).create_new(true).mode(0o600).open(&bundle)?;
                        file.write_all(invitation.as_bytes())?;
                        file.sync_all()?;
                        let invite = crate::pairing::Invite::load(&bundle).await?;
                        if !matches!(invite.scope, crate::pairing::Scope::Room(_)) { return Err(Error::Pairing); }
                        options(&app, &name, None, accept_files)
                    }.await;
                    let RunOptions { options, stop, ready, events } = match setup {
                        Ok(options) => options,
                        Err(error) => {
                            if let Ok(mut data) = app.0.data.lock() && let Some(operation) = data.operations.get_mut(&id) {
                                operation.state = "failed";
                                operation.error = Some(error.to_string());
                            }
                            return Err(error);
                        }
                    };
                    phase(&app, &name, "joining", None);
                    managed.insert(name.clone(), stop);
                    let current = app.clone();
                    let target = name.clone();
                    let profile = app.0.profile.clone();
                    let service = async move { local::join(&profile, &target, &bundle, bind, role.rights(), options, move |fingerprint, confirmation| confirm(current.clone(), id, fingerprint, confirmation)).await }.boxed_local();
                    rooms.spawn_local(monitor(app.clone(), name, Some(id), ready, events, service));
                    Ok(json!({"operation": id}))
                },
                Action::Confirm { id, accept } => {
                    let mut data = app.0.data.lock().map_err(|_| Error::Node("UI confirmation store is unavailable"))?;
                    let operation = data.operations.get_mut(&id).ok_or(Error::Cancelled)?;
                    operation.confirm.take().ok_or(Error::Configuration("there is no pending confirmation"))?.send(accept).map_err(|_| Error::Cancelled)?;
                    operation.state = if accept { "working" } else { "declined" };
                    operation.proposal = None;
                    Ok(json!({"confirmed": accept}))
                },
                Action::Reveal { id } => {
                    let data = app.0.data.lock().map_err(|_| Error::Node("UI event store is unavailable"))?;
                    let message = data.messages.iter().find(|message| message.id == id).ok_or(Error::Configuration("message no longer retained in memory"))?;
                    Ok(json!({"id": id, "text": message.text.as_str()}))
                },
                Action::Shutdown => { app.0.stop.send_replace(true); Ok(json!({"stopping": true})) },
            }
        }.await;
        let _ = request.reply.send(result);
    }
    for stop in managed.values() {
        let _ = stop.send(true);
    }
    if let Ok(mut data) = app.0.data.lock() {
        for operation in data.operations.values_mut() {
            if let Some(confirm) = operation.confirm.take() {
                let _ = confirm.send(false);
            }
        }
    }
    while rooms.join_next().await.is_some() {}
    while transfers.join_next().await.is_some() {}
    invitations.abort_all();
    while invitations.join_next().await.is_some() {}
    failure.map_or(Ok(()), Err)
}

pub(super) async fn snapshot(app: &App) -> Result<Value> {
    let peer = Peer::load(&app.0.profile.join("identity/certificate.der")).ok();
    let own = peer.as_ref().map(PeerId::from_peer);
    let identity = peer.map(|peer| peer.fingerprint());
    let rooms = if identity.is_some() {
        local::list(&app.0.profile)?
    } else {
        Vec::new()
    };
    let phases = app
        .0
        .data
        .lock()
        .map_err(|_| Error::Node("UI event store is unavailable"))?
        .phases
        .clone();
    let rooms = futures_util::future::join_all(rooms.into_iter().map(|room| {
        let query = room.problem.is_none() && !phases.get(&room.name).is_some_and(|phase| matches!(phase["state"].as_str(), Some("starting" | "joining" | "stopping")));
        async move {
        let status = if query {
            match tokio::time::timeout(std::time::Duration::from_millis(500), local::request(&app.0.profile, &room.name, &Command::Status)).await {
                Ok(Ok(reply)) if reply.ok => Some(reply.result), _ => None,
            }
        } else { None };
        let presentation = match (&status, room.room, own) {
            (Some(status), Some(id), Some(own)) => Some(membership(status, id, own).and_then(|membership| {
                let announcements = status.get("profile_names").cloned().map(serde_json::from_value).transpose().map_err(|_| Error::Protocol)?.unwrap_or_default();
                crate::profile::labels::present_with_announcements(&app.0.profile, id.0, own, &membership.members, &announcements)
            })),
            _ => None,
        };
        let (presentation, presentation_error) = match presentation { Some(Ok(items)) => (Some(items), None), Some(Err(error)) => (None, Some(error.to_string())), None => (None, None) };
        json!({"name": room.name, "title": room.title, "id": room.room, "problem": room.problem, "status": status, "presentation": presentation, "presentation_error": presentation_error})
    }})).await;
    let candidates = {
        let data = app
            .0
            .data
            .lock()
            .map_err(|_| Error::Node("UI event store is unavailable"))?;
        data.operations
            .iter()
            .filter(|(_, op)| op.issuing && op.state == "waiting")
            .map(|(id, op)| (*id, op.bundle.clone()))
            .collect::<Vec<_>>()
    };
    let mut valid = std::collections::BTreeSet::new();
    for (id, path) in candidates {
        if crate::pairing::Invite::load(&path).await.is_ok() {
            valid.insert(id);
        }
    }
    let data = app
        .0
        .data
        .lock()
        .map_err(|_| Error::Node("UI event store is unavailable"))?;
    let operations: Vec<_> = data.operations.iter().map(|(id, operation)| json!({"id": id, "room": operation.room, "kind": if operation.issuing { "invite" } else { "join" }, "state": operation.state, "proposal": operation.proposal,
        "invitation_ready": operation.issuing && operation.state == "waiting" && valid.contains(id), "error": operation.error})).collect();
    Ok(
        json!({"version": crate::VERSION, "identity": identity, "settings": crate::profile::Settings::load(&app.0.profile)?, "rooms": rooms, "messages": data.messages.iter().map(Message::view).collect::<Vec<_>>(),
        "operations": operations, "phases": data.phases, "history": "bounded-memory-only", "files": {"max_bytes": config::DEFAULT_FILE_LIMIT}, "media": {"width": config::MEDIA_VIDEO_WIDTH, "height": config::MEDIA_VIDEO_HEIGHT, "fps": config::MEDIA_VIDEO_FPS,
        "packet_bytes": config::MEDIA_PACKET_BYTES, "frame_bytes": config::MEDIA_ENCODED_FRAME_BYTES, "datagram_buffer_bytes": config::MEDIA_DATAGRAM_BUFFER_BYTES, "packet_age_ms": config::MEDIA_PACKET_MAX_AGE.as_millis(), "audio_bitrate": config::MEDIA_AUDIO_BITRATE,
        "max_tracks": config::MEDIA_MAX_STREAMS, "max_subscriptions": config::MEDIA_MAX_STREAMS, "max_publications": config::MEDIA_MAX_PUBLICATIONS, "source_kinds": config::MEDIA_SOURCE_KINDS, "max_users": config::MEDIA_MAX_USERS}}),
    )
}
