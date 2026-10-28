//! Optional, room-scoped presentation metadata. TLS selects both identities;
//! names carry no authentication authority. The optional cache is memory-only.
use super::*;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

static WORK: Semaphore = Semaphore::const_new(config::ROOM_PROFILE_RPCS);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ticket {
    room: [u8; 32],
    peer: PeerId,
    epoch: u64,
    generation: u64,
    serial: u64,
}
struct Flight {
    ticket: Ticket,
    abort: Option<tokio::task::AbortHandle>,
}
struct Entry {
    name: Option<String>,
    due: Instant,
}
#[derive(Default)]
struct Counters {
    started: u64,
    completed: u64,
    discarded: u64,
    cancelled: u64,
}
/// All transitions take an explicit clock so TTL/backoff and delayed results
/// can be tested without timing guesses. No lock is held across an await.
pub(super) struct Cache {
    room: [u8; 32],
    own: PeerId,
    epoch: Option<u64>,
    generation: u64,
    serial: u64,
    active: bool,
    members: BTreeSet<PeerId>,
    routes: BTreeMap<PeerId, Node>,
    route_seen: bool,
    route_log: Option<openraft::LogId<PeerId>>,
    routes_valid: bool,
    entries: BTreeMap<PeerId, Entry>,
    // Retain aborted reservations until their future is actually dropped. Even
    // across generations, a peer cannot have two live refresh tasks.
    flights: BTreeMap<PeerId, Flight>,
    counters: Counters,
}
impl Cache {
    pub(super) fn new(room: [u8; 32], own: PeerId) -> Self {
        Self {
            room,
            own,
            epoch: None,
            generation: 0,
            serial: 0,
            active: false,
            members: BTreeSet::new(),
            routes: BTreeMap::new(),
            route_seen: false,
            route_log: None,
            routes_valid: false,
            entries: BTreeMap::new(),
            flights: BTreeMap::new(),
            counters: Counters::default(),
        }
    }
    pub(super) fn invalidate(&mut self) {
        // A generation can never wrap and accidentally validate an old ticket.
        match self.generation.checked_add(1) {
            Some(next) => self.generation = next,
            None => self.active = false,
        }
        self.entries.clear();
        self.routes.clear();
        self.routes_valid = false;
        for flight in self.flights.values() {
            if let Some(abort) = &flight.abort {
                abort.abort();
            }
        }
    }
    pub(super) fn suspend(&mut self) {
        self.active = false;
        self.invalidate();
    }
    pub(super) fn register(&mut self, epoch: u64, members: &[PeerId]) -> Result<()> {
        let members = member_set(self.own, members)?;
        if self.epoch.is_some_and(|current| epoch < current) || self.generation == u64::MAX {
            return Err(Error::Denied);
        }
        self.invalidate();
        self.epoch = Some(epoch);
        self.members = members;
        self.active = true;
        Ok(())
    }
    /// Complete membership snapshots, not additive interest lists. An older
    /// epoch, or a same-epoch expansion after removal, cannot restore names.
    pub(super) fn observe(&mut self, epoch: u64, members: &[PeerId]) -> Result<bool> {
        let members = member_set(self.own, members)?;
        if let Some(current) = self.epoch {
            if epoch < current || (epoch == current && !members.is_subset(&self.members)) {
                return Ok(false);
            }
            if epoch == current && members == self.members {
                return Ok(true);
            }
        }
        self.invalidate();
        self.epoch = Some(epoch);
        self.members = members;
        Ok(true)
    }
    fn current(&self, ticket: Ticket) -> bool {
        self.active
            && self.room == ticket.room
            && self.epoch == Some(ticket.epoch)
            && self.generation == ticket.generation
            && self.members.contains(&ticket.peer)
    }
    fn install_routes(
        &mut self,
        log: Option<openraft::LogId<PeerId>>,
        routes: BTreeMap<PeerId, Node>,
    ) -> Result<bool> {
        if self.route_seen {
            if let Some(current) = self.route_log {
                let Some(incoming) = log else {
                    return Ok(false);
                };
                if incoming.index < current.index {
                    return Ok(false);
                }
                if incoming.index == current.index && incoming != current {
                    return Err(Error::Authentication);
                }
            }
            if log == self.route_log && self.routes_valid {
                if self.routes != routes {
                    return Err(Error::Authentication);
                }
                return Ok(true);
            }
        }
        self.invalidate();
        self.route_seen = true;
        self.route_log = log;
        self.routes = routes;
        self.routes_valid = true;
        Ok(true)
    }
    fn names(&mut self, now: Instant) -> BTreeMap<PeerId, String> {
        self.entries
            .iter_mut()
            .filter_map(|(peer, entry)| {
                if now >= entry.due {
                    entry.name = None;
                }
                entry.name.as_ref().map(|name| (*peer, name.clone()))
            })
            .collect()
    }
    fn due(&self, peer: PeerId, now: Instant) -> bool {
        self.active
            && self.members.contains(&peer)
            && !self.flights.contains_key(&peer)
            && self.entries.get(&peer).is_none_or(|entry| now >= entry.due)
    }
    fn reserve(&mut self, peer: PeerId, now: Instant) -> Result<Ticket> {
        if !self.due(peer, now) {
            return Err(Error::Denied);
        }
        self.serial = self.serial.checked_add(1).ok_or(Error::Protocol)?;
        let ticket = Ticket {
            room: self.room,
            peer,
            epoch: self.epoch.ok_or(Error::Denied)?,
            generation: self.generation,
            serial: self.serial,
        };
        self.entries.remove(&peer);
        self.flights.insert(
            peer,
            Flight {
                ticket,
                abort: None,
            },
        );
        self.counters.started = self.counters.started.saturating_add(1);
        Ok(ticket)
    }
    fn admit<'a>(
        &mut self,
        peer: PeerId,
        now: Instant,
        budget: &'a Semaphore,
    ) -> Result<(Ticket, tokio::sync::SemaphorePermit<'a>)> {
        if !self.due(peer, now) {
            return Err(Error::Denied);
        }
        let work = match budget.try_acquire() {
            Ok(work) => work,
            Err(_) => {
                self.unavailable(peer, now);
                return Err(Error::Denied);
            }
        };
        Ok((self.reserve(peer, now)?, work))
    }
    fn unavailable(&mut self, peer: PeerId, now: Instant) {
        self.entries.insert(
            peer,
            Entry {
                name: None,
                due: now + config::ROOM_PROFILE_CACHE_BACKOFF,
            },
        );
    }
    fn complete(&mut self, ticket: Ticket, result: Result<ProfileMetadata>, now: Instant) {
        if !self
            .flights
            .get(&ticket.peer)
            .is_some_and(|flight| flight.ticket == ticket)
        {
            return;
        }
        self.flights.remove(&ticket.peer);
        if !self.current(ticket) {
            self.counters.discarded = self.counters.discarded.saturating_add(1);
            return;
        }
        self.counters.completed = self.counters.completed.saturating_add(1);
        let name = result
            .ok()
            .filter(|response| response.epoch == ticket.epoch && response.validate().is_ok())
            .and_then(|response| response.name);
        match name {
            Some(name) => {
                self.entries.insert(
                    ticket.peer,
                    Entry {
                        name: Some(name),
                        due: now + config::ROOM_PROFILE_CACHE_TTL,
                    },
                );
            }
            None => self.unavailable(ticket.peer, now),
        }
    }
    fn cancelled(&mut self, ticket: Ticket, now: Instant) {
        if self
            .flights
            .get(&ticket.peer)
            .is_some_and(|flight| flight.ticket == ticket)
        {
            self.flights.remove(&ticket.peer);
            self.counters.cancelled = self.counters.cancelled.saturating_add(1);
            if self.current(ticket) {
                self.unavailable(ticket.peer, now);
            }
        }
    }
}
fn member_set(own: PeerId, members: &[PeerId]) -> Result<BTreeSet<PeerId>> {
    if members.len() > config::ROOM_MAX_MEMBERS {
        return Err(Error::Limit {
            resource: "profile cache members",
            max: config::ROOM_MAX_MEMBERS,
        });
    }
    let mut set = BTreeSet::new();
    for peer in members {
        if !set.insert(*peer) {
            return Err(Error::Configuration("duplicate profile cache member"));
        }
    }
    set.remove(&own);
    Ok(set)
}

struct Refresh {
    shared: Weak<Shared>,
    ticket: Ticket,
}
impl Drop for Refresh {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.upgrade() {
            let mut cache = shared
                .profile_cache
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            cache.cancelled(self.ticket, Instant::now());
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileMetadata {
    pub version: u8,
    pub epoch: u64,
    // Option alone would also accept an absent field. The wire schema requires
    // explicit null when disclosure is off.
    #[serde(deserialize_with = "Option::<String>::deserialize")]
    pub name: Option<String>,
}
impl ProfileMetadata {
    fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(Error::Protocol);
        }
        if let Some(name) = &self.name {
            crate::profile::validate_name(name)?;
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
pub(super) enum Source {
    #[default]
    Off,
    Static {
        name: String,
        share: bool,
    },
    Directory(PathBuf),
}
#[derive(Clone, Default)]
pub(super) struct SourceState {
    revision: u64,
    source: Source,
}
impl SourceState {
    fn replace(&mut self, source: Source) -> Result<()> {
        let revision = self.revision.checked_add(1).ok_or(Error::Protocol)?;
        self.source = source;
        self.revision = revision;
        Ok(())
    }
}
impl Source {
    fn read(
        &self,
        identity: &Peer,
    ) -> Result<(Option<String>, Option<crate::profile::SettingsPublication>)> {
        match self {
            Self::Off => Ok((None, None)),
            Self::Static { name, share } => {
                crate::profile::validate_name(name)?;
                Ok((share.then(|| name.clone()), None))
            }
            Self::Directory(directory) => {
                check_identity(directory, identity)?;
                let settings = crate::profile::Settings::publication(directory)?;
                // Do not adopt a profile replaced while settings were read.
                check_identity(directory, identity)?;
                Ok((
                    settings.as_ref().map(|settings| settings.name().to_owned()),
                    settings,
                ))
            }
        }
    }
}

fn check_identity(directory: &Path, expected: &Peer) -> Result<()> {
    crate::profile::private_directory(directory, false)?;
    let identity = directory.join("identity");
    crate::profile::private_directory(&identity, false)?;
    // Public certificate only: never load Identity or open private-key.der.
    let actual = Peer::load(&identity.join("certificate.der"))?;
    if actual.certificate_der() != expected.certificate_der() {
        return Err(Error::Authentication);
    }
    Ok(())
}

fn port(shared: &Shared, id: PeerId) -> Result<(crate::rooms::runtime::Handle, Peer)> {
    let handle = shared
        .room_port
        .read()
        .map_err(|_| Error::Protocol)?
        .clone()
        .ok_or(Error::Denied)?;
    let peer = shared
        .peers
        .read()
        .map_err(|_| Error::Protocol)?
        .get(&id)
        .cloned()
        .ok_or(Error::Denied)?;
    Ok((handle, peer))
}

impl Factory {
    #[cfg(all(test, unix))]
    pub(in crate::rooms::control::raft) fn profile_link_id(&self, target: PeerId) -> Option<usize> {
        let slots = self.shared.slots.try_lock().ok()?;
        let cached = slots.get(&target)?.try_lock().ok()?;
        cached.as_ref().map(|cached| cached.connection.stable_id())
    }
    #[cfg(all(test, unix))]
    pub(in crate::rooms::control::raft) fn pause_profile_publication(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, reached) = tokio::sync::oneshot::channel();
        let (resume, resumed) = tokio::sync::oneshot::channel();
        *self.shared.profile_publication_pause.lock().unwrap() =
            Some(PublicationPause { entered, resumed });
        (reached, resume)
    }
    pub(in crate::rooms::control::raft) fn suspend_profile_cache(&self) {
        self.shared
            .profile_cache
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .suspend();
    }
    #[cfg(all(test, unix))]
    pub(in crate::rooms::control::raft) fn profile_cache_probe(
        &self,
    ) -> (u64, u64, u64, u64, usize) {
        let cache = self.shared.profile_cache.lock().unwrap();
        (
            cache.counters.started,
            cache.counters.completed,
            cache.counters.discarded,
            cache.counters.cancelled,
            cache.flights.len(),
        )
    }
    /// Synchronous, demand-driven cache lookup. Routes are copied by the service
    /// from its cached committed membership; never discover, dial or read quorum
    /// here. Contention returns unavailable rather than waiting for the cache.
    pub(in crate::rooms::control::raft) fn cached_peer_profiles(
        &self,
        epoch: u64,
        members: &[PeerId],
        route_log: Option<openraft::LogId<PeerId>>,
        routes: BTreeMap<PeerId, Node>,
    ) -> Result<BTreeMap<PeerId, String>> {
        let wanted = member_set(PeerId::from_peer(&self.shared.identity.peer()), members)?;
        if self.shared.stopping.load(Ordering::SeqCst) {
            return Ok(BTreeMap::new());
        }
        let now = Instant::now();
        let (names, plans) = {
            let mut cache = match self.shared.profile_cache.try_lock() {
                Ok(cache) => cache,
                Err(std::sync::TryLockError::WouldBlock) => return Ok(BTreeMap::new()),
                Err(_) => return Err(Error::Protocol),
            };
            if !cache.active || !cache.observe(epoch, members)? {
                return Ok(BTreeMap::new());
            }
            let routes: BTreeMap<_, _> = routes
                .into_iter()
                .filter(|(peer, node)| {
                    wanted.contains(peer)
                        && node.endpoint.is_some()
                        && node.certificate.len() <= config::MAX_IDENTITY_BYTES
                })
                .collect();
            if !cache.install_routes(route_log, routes)? {
                return Ok(BTreeMap::new());
            }
            let names = cache.names(now);
            let mut plans = Vec::new();
            for peer in &wanted {
                if !cache.due(*peer, now) {
                    continue;
                }
                let Some(node) = cache.routes.get(peer).cloned() else {
                    cache.unavailable(*peer, now);
                    continue;
                };
                // Admission is reserved BEFORE spawning: no unbounded pending
                // tasks, semaphore waiters or detached retry queue.
                if let Ok((ticket, work)) = cache.admit(*peer, now, &WORK) {
                    plans.push((ticket, node, work));
                }
            }
            (names, plans)
        };
        for (ticket, node, work) in plans {
            let refresh = Refresh {
                shared: Arc::downgrade(&self.shared),
                ticket,
            };
            let abort = self.shared.drivers.spawn(async move {
                let Some(shared) = refresh.shared.upgrade() else {
                    return;
                };
                let current = shared
                    .profile_cache
                    .lock()
                    .map(|cache| cache.current(ticket))
                    .unwrap_or(false);
                if !current || shared.stopping.load(Ordering::SeqCst) {
                    return;
                }
                let factory = Factory {
                    shared,
                    owner: false,
                };
                let result = factory
                    .peer_profile_admitted(ticket.peer, &node, config::ROOM_PROFILE_TIMEOUT, work)
                    .await;
                if let Ok(mut cache) = factory.shared.profile_cache.lock() {
                    if factory.shared.stopping.load(Ordering::SeqCst) {
                        cache.suspend();
                    }
                    cache.complete(ticket, result, Instant::now());
                }
                drop(refresh);
            });
            // The task may already have completed, or invalidation may have
            // raced spawning. Never attach an abort handle to another ticket.
            if let Ok(mut cache) = self.shared.profile_cache.try_lock() {
                let current = cache.current(ticket);
                match cache.flights.get_mut(&ticket.peer) {
                    Some(flight) if flight.ticket == ticket && current => {
                        flight.abort = abort;
                    }
                    _ => {
                        if let Some(abort) = abort {
                            abort.abort();
                        }
                    }
                }
            } else if let Some(abort) = abort {
                abort.abort();
            }
        }
        Ok(names)
    }

    /// Explicit SDK opt-in. Passing false withholds the name; invalid names are
    /// rejected even while sharing is off. Replaces any directory source.
    pub fn configure_profile_name(&self, name: String, share: bool) -> Result<()> {
        crate::profile::validate_name(&name)?;
        self.shared
            .profile_source
            .write()
            .map_err(|_| Error::Protocol)?
            .replace(Source::Static { name, share })
    }

    /// Select an existing profile, bound to the running public certificate.
    /// None disables disclosure. Rechecked on every read; never creates data.
    pub fn configure_profile_directory(&self, directory: Option<PathBuf>) -> Result<()> {
        let source = match directory {
            Some(directory) => {
                check_identity(&directory, &self.shared.identity.peer())?;
                crate::profile::Settings::load(&directory)?;
                Source::Directory(directory)
            }
            None => Source::Off,
        };
        self.shared
            .profile_source
            .write()
            .map_err(|_| Error::Protocol)?
            .replace(source)
    }

    /// One raw optional GET, with fresh authorization on both sides of receipt.
    /// Unsupported, malformed and slow peers fail only this stream, not the
    /// cached authenticated connection. No retries, background work or cache.
    pub async fn peer_profile(
        &self,
        target: PeerId,
        node: &Node,
        timeout: Duration,
    ) -> Result<ProfileMetadata> {
        let timeout = bounded_timeout(timeout)?;
        let work = WORK.try_acquire().map_err(|_| Error::Denied)?;
        self.peer_profile_admitted(target, node, timeout, work)
            .await
    }
    async fn peer_profile_admitted(
        &self,
        target: PeerId,
        node: &Node,
        timeout: Duration,
        _work: tokio::sync::SemaphorePermit<'static>,
    ) -> Result<ProfileMetadata> {
        tokio::time::timeout(timeout, async {
            let (handle, peer) = port(&self.shared, target)?;
            let before = handle
                .authorize_profile_metadata(peer.clone(), timeout)
                .await?;
            let client = Client {
                shared: Arc::downgrade(&self.shared),
                target,
                node: node.clone(),
            };
            let (response, connection): (ProfileMetadata, _) = client
                .exchange_policy(
                    Request::ProfileGet,
                    RPCOption::new(timeout),
                    ExchangePolicy::OptionalProfile,
                )
                .await?;
            transport::verify_peer(&connection, &peer)?;
            response.validate()?;
            let after = handle.authorize_profile_metadata(peer, timeout).await?;
            if before != after || response.epoch != after {
                return Err(Error::Denied);
            }
            Ok(response)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
}

fn bounded_timeout(timeout: Duration) -> Result<Duration> {
    if timeout.is_zero() || timeout > config::MAX_TIMEOUT {
        return Err(Error::Configuration("invalid profile metadata timeout"));
    }
    Ok(timeout.min(config::ROOM_PROFILE_TIMEOUT))
}

pub(super) async fn disclose(
    shared: &Shared,
    id: PeerId,
) -> Result<(Disclosure, tokio::sync::SemaphorePermit<'static>)> {
    let work = WORK.try_acquire().map_err(|_| Error::Denied)?;
    tokio::time::timeout(config::ROOM_PROFILE_TIMEOUT, async {
        let (handle, peer) = port(shared, id)?;
        let epoch = handle
            .authorize_profile_metadata(peer.clone(), config::ROOM_PROFILE_TIMEOUT)
            .await?;
        let source = shared
            .profile_source
            .read()
            .map_err(|_| Error::Protocol)?
            .clone();
        let identity = shared.identity.peer();
        let revision = source.revision;
        // Keep the process-wide permit inside the blocking read on cancellation,
        // so stalled filesystem operations cannot grow an unbounded task queue.
        let (name, work) =
            tokio::task::spawn_blocking(move || (source.source.read(&identity), work))
                .await
                .map_err(|_| Error::Cancelled)?;
        let (name, settings) = name?;
        let response = ProfileMetadata {
            version: 1,
            epoch,
            name,
        };
        response.validate()?;
        if handle
            .authorize_profile_metadata(peer, config::ROOM_PROFILE_TIMEOUT)
            .await?
            != epoch
        {
            return Err(Error::Denied);
        }
        // The caller retains admission through the bounded response write.
        Ok((
            Disclosure {
                response,
                revision,
                settings,
            },
            work,
        ))
    })
    .await
    .map_err(|_| Error::Timeout)?
}

// Count the serialized representation before allocating its wire buffer.
struct Size(usize);
impl std::io::Write for Size {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|size| *size <= config::ROOM_PROFILE_WIRE_BYTES)
            .ok_or_else(|| std::io::Error::other("profile metadata exceeds wire budget"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(super) struct Disclosure {
    response: ProfileMetadata,
    revision: u64,
    settings: Option<crate::profile::SettingsPublication>,
}
#[cfg(all(test, unix))]
pub(super) struct PublicationPause {
    entered: tokio::sync::oneshot::Sender<()>,
    resumed: tokio::sync::oneshot::Receiver<()>,
}
impl Disclosure {
    fn try_publish<T>(&self, shared: &Shared, publish: impl FnOnce() -> T) -> Result<T> {
        // Lock order is settings publication lease, then SDK source guard. The
        // latter protects ONLY the final source check and nonblocking write poll,
        // never directory traversal, open or stat. No guard crosses an await.
        let publish = || {
            let source = shared
                .profile_source
                .try_read()
                .map_err(|_| Error::Denied)?;
            if source.revision != self.revision || shared.stopping.load(Ordering::SeqCst) {
                return Err(Error::Denied);
            }
            Ok(publish())
        };
        match &self.settings {
            Some(settings) => settings.try_publish(publish)?,
            None => publish(),
        }
    }
    pub(super) async fn send<W: tokio::io::AsyncWrite + Unpin>(
        &self,
        shared: &Shared,
        stream: &mut W,
    ) -> Result<()> {
        use std::{pin::Pin, task::Poll};
        use tokio::io::AsyncWrite;
        #[cfg(all(test, unix))]
        {
            let pause = shared.profile_publication_pause.lock().unwrap().take();
            if let Some(pause) = pause {
                let _ = pause.entered.send(());
                let _ = pause.resumed.await;
            }
        }
        self.response.validate()?;
        let mut size = Size(0);
        serde_json::to_writer(&mut size, &self.response).map_err(|_| Error::Limit {
            resource: "profile metadata",
            max: config::ROOM_PROFILE_WIRE_BYTES,
        })?;
        let mut bytes = Vec::with_capacity(size.0 + 4);
        bytes.extend((size.0 as u32).to_be_bytes());
        serde_json::to_writer(&mut bytes, &self.response).map_err(|_| Error::Protocol)?;
        let mut offset = 0;
        while offset < bytes.len() {
            let written = std::future::poll_fn(|cx| {
                match self.try_publish(shared, || {
                    AsyncWrite::poll_write(Pin::new(&mut *stream), cx, &bytes[offset..])
                }) {
                    Ok(Poll::Ready(Ok(written))) => Poll::Ready(Ok(written)),
                    Ok(Poll::Ready(Err(error))) => Poll::Ready(Err(Error::from(error))),
                    Ok(Poll::Pending) => Poll::Pending,
                    Err(error) => Poll::Ready(Err(error)),
                }
            })
            .await?;
            if written == 0 {
                return Err(Error::Network);
            }
            offset += written;
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod cache_tests;
#[cfg(all(test, unix))]
mod publication_tests;
#[cfg(all(test, unix))]
mod tests;
