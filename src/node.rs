//! Foreground persistent inbox, admitted peers and reusable authenticated links.

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex, Notify, Semaphore, mpsc},
    task::JoinSet,
};

use crate::{
    Error, Identity, Peer, ReceivedText, Result, config,
    contacts::{Action, Registry},
    files, session, transport,
};

pub mod ipc;

#[derive(Clone)]
pub struct Options {
    pub profile: PathBuf,
    pub bind: SocketAddr,
    pub inbox: Option<PathBuf>,
    pub max_file_bytes: u64,
    pub file_budget: u64,
}

#[derive(Debug)]
pub enum Event {
    Connected {
        alias: String,
    },
    Disconnected {
        alias: String,
    },
    Text {
        alias: String,
        content: ReceivedText,
    },
    File {
        alias: String,
        path: PathBuf,
        receipt: files::Receipt,
    },
    Refused {
        alias: Option<String>,
        code: &'static str,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Status {
    pub connections_accepted: u64,
    pub active_connections: usize,
    pub outgoing_links: usize,
    pub received_operations: u64,
    pub sent_operations: u64,
    pub file_budget_remaining: u64,
    pub file_attempts_remaining: u64,
    pub local_text_operations: usize,
    pub local_file_operations: usize,
}

type Slot = Arc<Mutex<Option<quinn::Connection>>>;
type Connections = BTreeMap<u64, (String, quinn::Connection)>;
struct Shared {
    stopping: AtomicBool,
    stop: Notify,
    identity: Identity,
    registry: Registry,
    options: Options,
    endpoint: quinn::Endpoint,
    events: mpsc::Sender<Event>,
    outgoing: Mutex<BTreeMap<(String, u8), Slot>>,
    connections: StdMutex<Connections>,
    next_connection: AtomicU64,
    received: AtomicU64,
    sent: AtomicU64,
    budget: AtomicU64,
    file_attempts: AtomicU64,
    streams: Arc<Semaphore>,
    files: Arc<Semaphore>,
    local_text: Arc<Semaphore>,
    local_files: Arc<Semaphore>,
}

pub struct Node {
    shared: Arc<Shared>,
    local: ipc::Listener,
}
#[derive(Clone)]
pub struct Handle {
    shared: std::sync::Weak<Shared>,
}

struct LinkAttempt {
    connection: quinn::Connection,
    complete: bool,
}
impl Drop for LinkAttempt {
    fn drop(&mut self) {
        // Never retry uncertain data automatically. Retire its link so the next
        // explicit operation can authenticate a fresh connection instead of reusing
        // a dead peer's cached connection indefinitely.
        if !self.complete {
            self.connection.close(1u32.into(), b"");
        }
    }
}

struct IncomingGuard {
    shared: Arc<Shared>,
    id: u64,
    alias: String,
}
impl Drop for IncomingGuard {
    fn drop(&mut self) {
        if let Ok(mut connections) = self.shared.connections.lock() {
            connections.remove(&self.id);
        }
        let _ = self.shared.events.try_send(Event::Disconnected {
            alias: self.alias.clone(),
        });
    }
}

impl Node {
    pub async fn bind(
        identity: Identity,
        options: Options,
    ) -> Result<(Self, Handle, mpsc::Receiver<Event>)> {
        if options.max_file_bytes > config::MAX_FILE_BYTES
            || options.file_budget > config::MAX_FILE_BYTES
        {
            return Err(Error::FileTooLarge);
        }
        crate::profile::private_directory(&options.profile, true)?;
        if let Some(inbox) = &options.inbox {
            crate::profile::private_directory(inbox, true)?;
        }
        let registry = Registry::open(options.profile.clone())?;
        let peers = admitted(&registry, &identity)?;
        let server = transport::server_config(
            &identity,
            &peers,
            &[config::ALPN, files::ALPN],
            transport::Policy::Interactive,
        )?;
        let endpoint = quinn::Endpoint::server(server, options.bind)?;
        let local = ipc::Listener::bind(&options.profile).await?;
        let (events, receiver) = mpsc::channel(config::NODE_EVENT_QUEUE);
        let budget = options.file_budget;
        let shared = Arc::new(Shared {
            stopping: AtomicBool::new(false),
            stop: Notify::new(),
            identity,
            registry,
            options,
            endpoint,
            events,
            outgoing: Mutex::new(BTreeMap::new()),
            connections: StdMutex::new(BTreeMap::new()),
            next_connection: AtomicU64::new(0),
            received: AtomicU64::new(0),
            sent: AtomicU64::new(0),
            budget: AtomicU64::new(budget),
            file_attempts: AtomicU64::new(config::NODE_FILE_COUNT_BUDGET),
            streams: Arc::new(Semaphore::new(config::NODE_MAX_STREAMS)),
            files: Arc::new(Semaphore::new(config::NODE_MAX_FILES)),
            local_text: Arc::new(Semaphore::new(config::NODE_MAX_STREAMS)),
            local_files: Arc::new(Semaphore::new(config::NODE_MAX_FILES)),
        });
        Ok((
            Self {
                shared: shared.clone(),
                local,
            },
            Handle {
                shared: Arc::downgrade(&shared),
            },
            receiver,
        ))
    }
    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.shared.endpoint.local_addr()?)
    }

    pub async fn run(self) -> Result<()> {
        let mut tasks = JoinSet::new();
        let connections = Arc::new(Semaphore::new(config::NODE_MAX_CONNECTIONS));
        let commands = Arc::new(Semaphore::new(config::NODE_MAX_STREAMS));
        let text_commands = self.shared.local_text.clone();
        let file_commands = self.shared.local_files.clone();
        let mut revision = self.shared.registry.revision()?;
        let mut timer = tokio::time::interval(Duration::from_secs(1));
        loop {
            while let Some(result) = tasks.try_join_next() {
                if result.is_err() {
                    return Err(Error::Node("session worker failed"));
                }
            }
            tokio::select! {
                _ = self.shared.stop.notified() => break,
                incoming = self.shared.endpoint.accept() => {
                    let Some(incoming) = incoming else { break; };
                    if let Ok(permit) = connections.clone().try_acquire_owned() {
                        let shared = self.shared.clone();
                        tasks.spawn(async move {
                            let _permit = permit;
                            if let Err(error) = receive_connection(shared.clone(), incoming).await {
                                let _ = shared.events.try_send(Event::Refused { alias: None, code: error.code() });
                            }
                        });
                    } else { incoming.refuse(); }
                },
                stream = self.local.accept() => {
                    let stream = stream?;
                    if let Ok(permit) = commands.clone().try_acquire_owned() {
                        let handle = Handle { shared: Arc::downgrade(&self.shared) };
                        let text_slots = text_commands.clone();
                        let file_slots = file_commands.clone();
                        tasks.spawn(ipc::serve(stream, handle, permit, text_slots, file_slots));
                    }
                },
                _ = timer.tick() => {
                    let current = self.shared.registry.revision()?;
                    if current != revision {
                        revision = current;
                        let peers = admitted(&self.shared.registry, &self.shared.identity)?;
                        let server = if peers.is_empty() { None } else { Some(transport::server_config(&self.shared.identity, &peers,
                            &[config::ALPN, files::ALPN], transport::Policy::Interactive)?) };
                        self.shared.endpoint.set_server_config(server);
                        {
                            let active = self.shared.connections.lock().map_err(|_| Error::Node("connection state unavailable"))?;
                            for (_, connection) in active.values() {
                                if transport::peer_identity(connection).and_then(|peer| self.shared.registry.find_peer(&peer, &self.shared.identity)).is_err() {
                                    connection.close(1u32.into(), b"");
                                }
                            }
                        }
                        let outgoing = self.shared.outgoing.lock().await;
                        for slot in outgoing.values() {
                            if let Ok(connection) = slot.try_lock()
                                && let Some(connection) = connection.as_ref()
                                && transport::peer_identity(connection).and_then(|peer| self.shared.registry.find_peer(&peer, &self.shared.identity)).is_err() {
                                connection.close(1u32.into(), b"");
                            }
                        }
                    }
                },
                finished = tasks.join_next(), if !tasks.is_empty() => {
                    if finished.is_some_and(|result| result.is_err()) { return Err(Error::Node("session worker failed")); }
                },
            }
        }
        self.shared.endpoint.close(0u32.into(), b"");
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        self.shared.endpoint.wait_idle().await;
        Ok(())
    }
}
impl Drop for Node {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        self.shared.endpoint.close(0u32.into(), b"");
    }
}

fn admitted(registry: &Registry, identity: &Identity) -> Result<Vec<Peer>> {
    registry
        .active(identity)?
        .into_iter()
        .map(|(_, contact)| contact.peer())
        .collect()
}

async fn receive_connection(shared: Arc<Shared>, incoming: quinn::Incoming) -> Result<()> {
    if shared.stopping.load(Ordering::SeqCst) {
        incoming.refuse();
        return Err(Error::Node("session is stopping"));
    }
    let connection = tokio::time::timeout(config::DEFAULT_TIMEOUT, incoming)
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|_| Error::Authentication)?;
    let peer = transport::peer_identity(&connection)?;
    let (alias, _) = shared.registry.find_peer(&peer, &shared.identity)?;
    let protocol = transport::protocol(&connection)?;
    if protocol != config::ALPN && protocol != files::ALPN {
        return Err(Error::Protocol);
    }
    let fingerprint = peer.fingerprint();
    let id;
    {
        let mut active = shared
            .connections
            .lock()
            .map_err(|_| Error::Node("connection state unavailable"))?;
        let count = active
            .values()
            .filter(|(key, _)| key == &fingerprint)
            .count();
        let distinct: BTreeSet<_> = active.values().map(|(key, _)| key).collect();
        if count >= 3
            || (!distinct.contains(&fingerprint) && distinct.len() >= config::NODE_MAX_PEERS)
        {
            connection.close(1u32.into(), b"");
            return Err(Error::Node("peer connection limit reached"));
        }
        id = shared.next_connection.fetch_add(1, Ordering::Relaxed);
        active.insert(id, (fingerprint.clone(), connection.clone()));
    }
    let _guard = IncomingGuard {
        shared: shared.clone(),
        id,
        alias: alias.clone(),
    };
    shared
        .events
        .send(Event::Connected {
            alias: alias.clone(),
        })
        .await
        .map_err(|_| Error::Node("event consumer closed"))?;
    let mut operations = JoinSet::new();
    loop {
        while let Some(result) = operations.try_join_next() {
            if result.is_err() {
                return Err(Error::Node("stream worker failed"));
            }
        }
        tokio::select! {
            stream = connection.accept_bi(), if operations.len() < config::NODE_MAX_STREAMS => {
                let (response, request) = match stream { Ok(value) => value, Err(_) => break };
                let permit = shared.streams.clone().acquire_owned().await.map_err(|_| Error::Node("stream capacity closed"))?;
                let shared = shared.clone(); let peer = peer.clone(); let alias = alias.clone(); let protocol = protocol.clone();
                operations.spawn(async move {
                    let _permit = permit;
                    let outcome = tokio::time::timeout(config::DEFAULT_FILE_TIMEOUT,
                        receive_operation(shared.clone(), &peer, alias.clone(), protocol, request, response)).await;
                    let error = match outcome { Ok(Ok(())) => None, Ok(Err(error)) => Some(error), Err(_) => Some(Error::Timeout) };
                    if let Some(error) = error {
                        let _ = shared.events.try_send(Event::Refused { alias: Some(alias), code: error.code() });
                    }
                });
            },
            _ = operations.join_next(), if !operations.is_empty() => {},
        }
    }
    while let Some(result) = operations.join_next().await {
        if result.is_err() {
            return Err(Error::Node("stream worker failed"));
        }
    }
    Ok(())
}

async fn receive_operation(
    shared: Arc<Shared>,
    peer: &Peer,
    alias: String,
    protocol: Vec<u8>,
    request: quinn::RecvStream,
    response: quinn::SendStream,
) -> Result<()> {
    let event = shared
        .events
        .clone()
        .reserve_owned()
        .await
        .map_err(|_| Error::Node("event consumer closed"))?;
    if protocol == config::ALPN {
        let content = session::receive_stream(request, response, || {
            shared
                .registry
                .authorize_peer(peer, &shared.identity, Action::Text, true)
        })
        .await?;
        shared.received.fetch_add(1, Ordering::Relaxed);
        event.send(Event::Text { alias, content });
    } else {
        let _file = shared
            .files
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Node("file capacity exhausted"))?;
        let inbox = shared.options.inbox.as_ref().ok_or(Error::Denied)?;
        let receipt = files::stream::receive_with(
            peer,
            request,
            response,
            || {
                shared
                    .registry
                    .authorize_peer(peer, &shared.identity, Action::Files, true)
            },
            |info| {
                if info.bytes > shared.options.max_file_bytes {
                    return Err(Error::FileTooLarge);
                }
                shared
                    .file_attempts
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                        left.checked_sub(1)
                    })
                    .map_err(|_| Error::Node("file count budget exhausted"))?;
                shared
                    .budget
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                        left.checked_sub(info.bytes)
                    })
                    .map_err(|_| Error::Node("file byte budget exhausted"))?;
                files::ReceivePolicy::accept(
                    inbox.join(format!("{}-{}.bin", peer.fingerprint(), info.hash_hex())),
                    shared.options.max_file_bytes,
                )
            },
        )
        .await?;
        // Successful resume used no storage for the retained prefix; failed offers
        // retain their reservation conservatively rather than bypassing the quota.
        shared
            .budget
            .fetch_add(receipt.resumed_from, Ordering::SeqCst);
        let path = inbox.join(format!(
            "{}-{}.bin",
            peer.fingerprint(),
            receipt.info.hash_hex()
        ));
        shared.received.fetch_add(1, Ordering::Relaxed);
        event.send(Event::File {
            alias,
            path,
            receipt,
        });
    }
    Ok(())
}

impl Handle {
    pub fn shutdown(&self) -> Result<()> {
        let shared = self
            .shared
            .upgrade()
            .ok_or(Error::Node("session is stopped"))?;
        shared.stopping.store(true, Ordering::SeqCst);
        shared.endpoint.close(0u32.into(), b"");
        shared.stop.notify_one();
        Ok(())
    }
    async fn link(
        &self,
        alias: &str,
        action: Action,
        kind: u8,
    ) -> Result<(Arc<Shared>, Peer, quinn::Connection)> {
        let shared = self
            .shared
            .upgrade()
            .ok_or(Error::Node("session is stopped"))?;
        if shared.stopping.load(Ordering::SeqCst) {
            return Err(Error::Node("session is stopping"));
        }
        let contact = shared
            .registry
            .resolve(alias, &shared.identity, action, false)?;
        let peer = contact.peer()?;
        let fingerprint = peer.fingerprint();
        let slot = {
            let mut slots = shared.outgoing.lock().await;
            slots.retain(|_, slot| {
                Arc::strong_count(slot) > 1
                    || slot.try_lock().map_or(true, |connection| {
                        connection
                            .as_ref()
                            .is_some_and(|connection| connection.close_reason().is_none())
                    })
            });
            let peers: BTreeSet<_> = slots.keys().map(|(peer, _)| peer).collect();
            if !peers.contains(&fingerprint) && peers.len() >= config::NODE_MAX_PEERS {
                return Err(Error::Node("outgoing peer limit reached"));
            }
            slots
                .entry((fingerprint, kind))
                .or_insert_with(|| Arc::new(Mutex::new(None)))
                .clone()
        };
        let mut cached = slot.lock().await;
        if let Some(connection) = cached
            .as_ref()
            .filter(|connection| connection.close_reason().is_none())
        {
            return Ok((shared.clone(), peer, connection.clone()));
        }
        let destination = *contact
            .endpoints
            .first()
            .ok_or(Error::Configuration("contact has no address hint"))?;
        let protocol = if kind == 0 { config::ALPN } else { files::ALPN };
        let client = transport::client_config(
            &shared.identity,
            &peer,
            protocol,
            transport::Policy::Interactive,
        )?;
        let connection = shared
            .endpoint
            .connect_with(client, destination, config::TLS_NAME)
            .map_err(|_| Error::Network)?
            .await
            .map_err(|_| Error::Network)?;
        transport::verify_peer(&connection, &peer)?;
        shared.registry.find_peer(&peer, &shared.identity)?;
        *cached = Some(connection.clone());
        Ok((shared.clone(), peer, connection))
    }
    pub async fn send_text(
        &self,
        alias: &str,
        text: &str,
        sensitive: bool,
        deadline: Duration,
    ) -> Result<()> {
        transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        if text.len() > config::MAX_MESSAGE_BYTES {
            return Err(Error::MessageTooLarge);
        }
        tokio::time::timeout(deadline, async {
            let (shared, peer, connection) = self.link(alias, Action::Text, 0).await?;
            let mut attempt = LinkAttempt {
                connection: connection.clone(),
                complete: false,
            };
            let (request, response) = connection.open_bi().await.map_err(|_| Error::Network)?;
            session::send_stream(text, sensitive, request, response, || {
                shared
                    .registry
                    .authorize_peer(&peer, &shared.identity, Action::Text, false)
            })
            .await?;
            shared.sent.fetch_add(1, Ordering::Relaxed);
            attempt.complete = true;
            Ok(())
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    pub async fn send_file(
        &self,
        alias: &str,
        source: &std::path::Path,
        sensitive: bool,
        deadline: Duration,
    ) -> Result<files::Receipt> {
        transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        tokio::time::timeout(deadline, async {
            let (shared, peer, connection) = self.link(alias, Action::Files, 1).await?;
            let mut file = files::source_file(source)?;
            let info = files::info(&mut file).await?;
            let mut attempt = LinkAttempt {
                connection: connection.clone(),
                complete: false,
            };
            let (request, response) = connection.open_bi().await.map_err(|_| Error::Network)?;
            let receipt =
                files::stream::send(&mut file, info, sensitive, request, response, || {
                    shared
                        .registry
                        .authorize_peer(&peer, &shared.identity, Action::Files, false)
                })
                .await?;
            shared.sent.fetch_add(1, Ordering::Relaxed);
            attempt.complete = true;
            Ok(receipt)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    pub async fn status(&self) -> Result<Status> {
        let shared = self
            .shared
            .upgrade()
            .ok_or(Error::Node("session is stopped"))?;
        if shared.stopping.load(Ordering::SeqCst) {
            return Err(Error::Node("session is stopping"));
        }
        let active_connections = {
            shared
                .connections
                .lock()
                .map_err(|_| Error::Node("connection state unavailable"))?
                .len()
        };
        let outgoing_links = shared
            .outgoing
            .lock()
            .await
            .values()
            .filter(|slot| {
                slot.try_lock().map_or(true, |connection| {
                    connection
                        .as_ref()
                        .is_some_and(|connection| connection.close_reason().is_none())
                })
            })
            .count();
        Ok(Status {
            connections_accepted: shared.next_connection.load(Ordering::Relaxed),
            active_connections,
            outgoing_links,
            received_operations: shared.received.load(Ordering::Relaxed),
            sent_operations: shared.sent.load(Ordering::Relaxed),
            file_budget_remaining: shared.budget.load(Ordering::Relaxed),
            file_attempts_remaining: shared.file_attempts.load(Ordering::Relaxed),
            local_text_operations: config::NODE_MAX_STREAMS - shared.local_text.available_permits(),
            local_file_operations: config::NODE_MAX_FILES - shared.local_files.available_permits(),
        })
    }
}
