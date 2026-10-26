//! Explicit local subscriptions and bounded RTP routing. This layer never opens
//! capture devices or native playback; those remain owned by local consumers.
use super::*;
use crate::{
    media::{Kind, Packet},
    rooms::runtime::MediaPolicy,
};
use std::sync::{Mutex as StdMutex, atomic::AtomicUsize};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
    time::Instant,
};
mod publish;
#[cfg(all(test, unix))]
mod tests;
pub use publish::Publication;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Offer {
    pub id: [u8; 16],
    pub kind: Kind,
}

pub struct Frame {
    pub track: [u8; 16],
    pub packet: Packet,
    pub received_ms: u64,
}
#[derive(Serialize)]
pub struct Status {
    pub sessions: usize,
    pub subscriptions: usize,
    pub active_tracks: usize,
    pub dropped_packets: usize,
    pub policy_lease_valid: bool,
}
struct Queued {
    epoch: u64,
    arrived: Instant,
    frame: Frame,
}
struct Listener {
    token: u64,
    sender: mpsc::Sender<Queued>,
    active: bool,
}
struct Track {
    kind: Kind,
    connection: usize,
    listener: u64,
    seen: Instant,
    ending: Option<Instant>,
}
struct State {
    users: usize,
    refresh: Option<JoinHandle<()>>,
    listeners: BTreeMap<(PeerId, Kind), Listener>,
    tracks: BTreeMap<(PeerId, [u8; 16]), Track>,
}
struct Lease {
    policy: MediaPolicy,
    until: Instant,
}
pub(super) struct Hub {
    state: StdMutex<State>,
    lease: RwLock<Option<Lease>>,
    updated: watch::Sender<u64>,
    generation: AtomicU64,
    next: AtomicU64,
    closed: AtomicBool,
    dropped: AtomicUsize,
}
pub(super) struct Usage {
    hub: Arc<Hub>,
    generation: u64,
}
pub struct Subscription {
    usage: Usage,
    shared: Weak<Shared>,
    peer: PeerId,
    kind: Kind,
    token: u64,
    receiver: mpsc::Receiver<Queued>,
}
impl Hub {
    pub fn new() -> Arc<Self> {
        let (updated, _) = watch::channel(0);
        Arc::new(Self {
            state: StdMutex::new(State {
                users: 0,
                refresh: None,
                listeners: BTreeMap::new(),
                tracks: BTreeMap::new(),
            }),
            lease: RwLock::new(None),
            updated,
            generation: AtomicU64::new(0),
            next: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            dropped: AtomicUsize::new(0),
        })
    }
    fn use_media(self: &Arc<Self>, shared: Weak<Shared>) -> Result<Usage> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(Error::Cancelled);
        }
        let mut state = self.state.lock().map_err(|_| Error::Protocol)?;
        if state.users >= config::MEDIA_MAX_STREAMS * 2 {
            return Err(Error::Node("media session capacity exhausted"));
        }
        state.users += 1;
        if state.refresh.is_none() {
            let weak = Arc::downgrade(self);
            state.refresh = Some(tokio::spawn(async move {
                let mut timer = tokio::time::interval(config::MEDIA_POLICY_REFRESH);
                timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    timer.tick().await;
                    let Some(hub) = weak.upgrade() else {
                        break;
                    };
                    let stop = {
                        let Ok(mut state) = hub.state.lock() else {
                            break;
                        };
                        if state.users == 0 || hub.closed.load(Ordering::SeqCst) {
                            state.refresh.take();
                            true
                        } else {
                            false
                        }
                    };
                    if stop {
                        break;
                    }
                    let generation = hub.generation.load(Ordering::SeqCst);
                    let handle = shared.upgrade().and_then(|shared| {
                        shared
                            .room_port
                            .read()
                            .ok()
                            .and_then(|handle| handle.clone())
                    });
                    let policy = match handle {
                        Some(handle) => {
                            handle.media_policy(config::MEDIA_POLICY_TIMEOUT).await.ok()
                        }
                        None => None,
                    };
                    if let Ok(mut lease) = hub.lease.write() {
                        *lease = if generation == hub.generation.load(Ordering::SeqCst)
                            && !hub.closed.load(Ordering::SeqCst)
                        {
                            policy.map(|policy| Lease {
                                policy,
                                until: Instant::now() + config::MEDIA_POLICY_LEASE,
                            })
                        } else {
                            None
                        };
                    }
                    hub.updated
                        .send_modify(|version| *version = version.wrapping_add(1));
                }
            }));
        }
        Ok(Usage {
            hub: self.clone(),
            generation: self.generation.load(Ordering::SeqCst),
        })
    }
    async fn ready(&self, shared: &Shared) -> Result<()> {
        let mut updates = self.updated.subscribe();
        tokio::time::timeout(
            config::MEDIA_POLICY_TIMEOUT + config::MEDIA_POLICY_REFRESH,
            async {
                loop {
                    if self.closed.load(Ordering::SeqCst) {
                        return Err(Error::Cancelled);
                    }
                    if self
                        .lease
                        .read()
                        .map_err(|_| Error::Protocol)?
                        .as_ref()
                        .is_some_and(|lease| {
                            lease.until > Instant::now()
                                && lease.policy.epoch == shared.media_epoch.load(Ordering::SeqCst)
                        })
                    {
                        return Ok(());
                    }
                    updates.changed().await.map_err(|_| Error::Cancelled)?;
                }
            },
        )
        .await
        .map_err(|_| Error::Timeout)?
    }
    fn allowed(&self, peer: PeerId, inbound: bool, epoch: u64, generation: u64) -> bool {
        if self.closed.load(Ordering::SeqCst)
            || self.generation.load(Ordering::SeqCst) != generation
        {
            return false;
        }
        self.lease
            .read()
            .ok()
            .and_then(|lease| {
                lease.as_ref().map(|lease| {
                    lease.until > Instant::now()
                        && lease.policy.epoch == epoch
                        && if inbound {
                            lease.policy.inbound.contains(&peer)
                        } else {
                            lease.policy.outbound.contains(&peer)
                        }
                })
            })
            .unwrap_or(false)
    }
    fn status(&self, epoch: u64) -> Result<Status> {
        let state = self.state.lock().map_err(|_| Error::Protocol)?;
        let now = Instant::now();
        let policy_lease_valid = !self.closed.load(Ordering::SeqCst)
            && self
                .lease
                .read()
                .map_err(|_| Error::Protocol)?
                .as_ref()
                .is_some_and(|lease| lease.policy.epoch == epoch && lease.until > now);
        Ok(Status {
            sessions: state.users,
            subscriptions: state.listeners.len(),
            active_tracks: state
                .tracks
                .values()
                .filter(|track| {
                    track.seen.elapsed() < config::MEDIA_SOURCE_IDLE
                        && track.ending.is_none_or(|until| until > now)
                })
                .count(),
            dropped_packets: self.dropped.load(Ordering::Relaxed),
            policy_lease_valid,
        })
    }
    fn peers(&self, epoch: u64, generation: u64) -> Result<Vec<PeerId>> {
        if self.closed.load(Ordering::SeqCst)
            || self.generation.load(Ordering::SeqCst) != generation
        {
            return Err(Error::Cancelled);
        }
        let lease = self.lease.read().map_err(|_| Error::Protocol)?;
        let lease = lease
            .as_ref()
            .filter(|lease| lease.until > Instant::now() && lease.policy.epoch == epoch)
            .ok_or(Error::Denied)?;
        Ok(lease.policy.outbound.iter().copied().collect())
    }
    pub fn invalidate_sessions(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut lease) = self.lease.write() {
            *lease = None;
        }
        if let Ok(mut state) = self.state.lock() {
            state.listeners.clear();
            state.tracks.clear();
        }
        self.updated
            .send_modify(|version| *version = version.wrapping_add(1));
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.invalidate_sessions();
    }
    pub async fn shutdown(&self) -> Result<()> {
        self.close();
        let task = self
            .state
            .lock()
            .map_err(|_| Error::Protocol)?
            .refresh
            .take();
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
        Ok(())
    }
    pub(super) fn offer(
        &self,
        shared: &Shared,
        peer: PeerId,
        connection: usize,
        offer: Offer,
    ) -> Result<()> {
        let generation = self.generation.load(Ordering::SeqCst);
        if !self.allowed(
            peer,
            true,
            shared.media_epoch.load(Ordering::SeqCst),
            generation,
        ) {
            return Err(Error::Denied);
        }
        let mut state = self.state.lock().map_err(|_| Error::Protocol)?;
        let now = Instant::now();
        state.tracks.retain(|_, track| {
            now.duration_since(track.seen) < config::MEDIA_SOURCE_IDLE
                && track.ending.is_none_or(|until| until > now)
        });
        if state
            .tracks
            .get(&(peer, offer.id))
            .is_some_and(|track| track.kind != offer.kind)
        {
            return Err(Error::Authentication);
        }
        let listener = state
            .listeners
            .get(&(peer, offer.kind))
            .filter(|listener| !listener.sender.is_closed())
            .ok_or(Error::Denied)?
            .token;
        if state.tracks.iter().any(|((source, id), track)| {
            *source == peer && track.kind == offer.kind && *id != offer.id
        }) {
            return Err(Error::Node(
                "a source already occupies this media subscription",
            ));
        }
        if !state.tracks.contains_key(&(peer, offer.id))
            && state.tracks.len() >= config::MEDIA_MAX_STREAMS
        {
            return Err(Error::Node("media track capacity exhausted"));
        }
        state.tracks.insert(
            (peer, offer.id),
            Track {
                kind: offer.kind,
                connection,
                listener,
                seen: now,
                ending: None,
            },
        );
        if let Some(listener) = state.listeners.get_mut(&(peer, offer.kind)) {
            listener.active = true;
        }
        Ok(())
    }
    pub(super) fn end(&self, peer: PeerId, track: [u8; 16]) {
        if let Ok(mut state) = self.state.lock()
            && let Some(track) = state.tracks.get_mut(&(peer, track))
        {
            track.ending = Some(Instant::now() + config::MEDIA_PACKET_MAX_AGE);
        }
    }
    pub(super) fn disconnected(&self, connection: usize) {
        if let Ok(mut state) = self.state.lock() {
            state
                .tracks
                .retain(|_, track| track.connection != connection);
        }
    }
    pub(super) fn receive(&self, shared: &Shared, peer: PeerId, connection: usize, bytes: &[u8]) {
        let epoch = shared.media_epoch.load(Ordering::SeqCst);
        if bytes.len() < 16 + 12
            || bytes.len() > 16 + config::MEDIA_PACKET_BYTES
            || !self.allowed(peer, true, epoch, self.generation.load(Ordering::SeqCst))
        {
            return;
        }
        let Ok(id) = <[u8; 16]>::try_from(&bytes[..16]) else {
            return;
        };
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(track) = state.tracks.get_mut(&(peer, id)).filter(|track| {
            track.connection == connection
                && track.ending.is_none_or(|until| until > Instant::now())
        }) else {
            return;
        };
        let Ok(packet) = Packet::parse(track.kind.codec(), bytes[16..].to_vec()) else {
            return;
        };
        track.seen = Instant::now();
        let key = (peer, track.kind);
        let token = track.listener;
        let Some(listener) = state
            .listeners
            .get(&key)
            .filter(|listener| listener.token == token)
        else {
            return;
        };
        let message = Queued {
            epoch,
            arrived: Instant::now(),
            frame: Frame {
                track: id,
                packet,
                received_ms: match crate::media::now_ms() {
                    Ok(now) => now,
                    Err(_) => return,
                },
            },
        };
        if listener.sender.try_send(message).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}
impl Drop for Usage {
    fn drop(&mut self) {
        if let Ok(mut state) = self.hub.state.lock() {
            state.users = state.users.saturating_sub(1);
        }
    }
}
impl Subscription {
    pub async fn next(&mut self) -> Result<Frame> {
        loop {
            let shared = self.shared.upgrade().ok_or(Error::Cancelled)?;
            let epoch = shared.media_epoch.load(Ordering::SeqCst);
            if !self
                .usage
                .hub
                .allowed(self.peer, true, epoch, self.usage.generation)
            {
                return Err(Error::Denied);
            }
            drop(shared);
            let ended = {
                let state = self.usage.hub.state.lock().map_err(|_| Error::Protocol)?;
                let active = state
                    .listeners
                    .get(&(self.peer, self.kind))
                    .is_some_and(|listener| listener.token == self.token && listener.active);
                active
                    && !state.tracks.values().any(|track| {
                        track.listener == self.token
                            && track.seen.elapsed() < config::MEDIA_SOURCE_IDLE
                            && track.ending.is_none_or(|until| until > Instant::now())
                    })
            };
            if ended {
                return Err(Error::Cancelled);
            }
            tokio::select! {
                message = self.receiver.recv() => {
                    let message = message.ok_or(Error::Cancelled)?;
                    let shared = self.shared.upgrade().ok_or(Error::Cancelled)?;
                    let current = shared.media_epoch.load(Ordering::SeqCst);
                    if message.epoch == current && message.arrived.elapsed() <= config::MEDIA_PACKET_MAX_AGE
                        && self.usage.hub.allowed(self.peer, true, current, self.usage.generation) { return Ok(message.frame); }
                },
                _ = tokio::time::sleep(config::MEDIA_PACKET_MAX_AGE) => {},
            }
        }
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        if let Ok(mut state) = self.usage.hub.state.lock()
            && state
                .listeners
                .get(&(self.peer, self.kind))
                .is_some_and(|listener| listener.token == self.token)
        {
            state.listeners.remove(&(self.peer, self.kind));
            state.tracks.retain(|_, track| track.listener != self.token);
        }
    }
}
impl Factory {
    pub fn media_status(&self) -> Result<Status> {
        self.shared
            .media
            .status(self.shared.media_epoch.load(Ordering::SeqCst))
    }
    pub(crate) async fn shutdown_media(&self) -> Result<()> {
        self.shared.media.shutdown().await
    }
    pub(crate) async fn subscribe_media(&self, peer: PeerId, kind: Kind) -> Result<Subscription> {
        let hub = &self.shared.media;
        let usage = hub.use_media(Arc::downgrade(&self.shared))?;
        hub.ready(&self.shared).await?;
        if !hub.allowed(
            peer,
            true,
            self.shared.media_epoch.load(Ordering::SeqCst),
            usage.generation,
        ) {
            return Err(Error::Denied);
        }
        let (sender, receiver) = mpsc::channel(config::MEDIA_PACKET_QUEUE);
        let token = hub.next.fetch_add(1, Ordering::SeqCst);
        {
            let mut state = hub.state.lock().map_err(|_| Error::Protocol)?;
            if state
                .listeners
                .get(&(peer, kind))
                .is_some_and(|listener| !listener.sender.is_closed())
            {
                return Err(Error::Node(
                    "this source already has an active media listener",
                ));
            }
            if state.listeners.len() >= config::MEDIA_MAX_STREAMS {
                return Err(Error::Node("media subscription capacity exhausted"));
            }
            state.listeners.insert(
                (peer, kind),
                Listener {
                    token,
                    sender,
                    active: false,
                },
            );
        }
        Ok(Subscription {
            usage,
            shared: Arc::downgrade(&self.shared),
            peer,
            kind,
            token,
            receiver,
        })
    }
}
