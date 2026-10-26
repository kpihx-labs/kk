//! Encode once outside the transport, fan out only to currently authorized peers.
use super::*;
use ring::rand::{SecureRandom, SystemRandom};

struct Target {
    id: PeerId,
    node: Node,
    connection: quinn::Connection,
}
#[derive(Default, Serialize)]
pub struct Statistics {
    pub sent: u64,
    pub dropped: u64,
    pub denied: u64,
    pub stale_routes: u64,
}
pub struct Publication {
    shared: Weak<Shared>,
    usage: Usage,
    offer: Offer,
    targets: Vec<Target>,
    statistics: Statistics,
    next: Instant,
    started: Instant,
    routes: watch::Receiver<openraft::RaftMetrics<PeerId, Node>>,
}
impl Factory {
    pub(crate) async fn publish_media(
        &self,
        kind: Kind,
        targets: Vec<PeerId>,
        routes: watch::Receiver<openraft::RaftMetrics<PeerId, Node>>,
    ) -> Result<Publication> {
        if targets.len() > config::ROOM_MAX_MEMBERS {
            return Err(Error::Protocol);
        }
        let usage = self.shared.media.use_media(Arc::downgrade(&self.shared))?;
        self.shared.media.ready(&self.shared).await?;
        let allowed = self.shared.media.peers(
            self.shared.media_epoch.load(Ordering::SeqCst),
            usage.generation,
        )?;
        let selected = if targets.is_empty() {
            allowed.clone()
        } else {
            targets
        }
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
        let mut id = [0; 16];
        SystemRandom::new()
            .fill(&mut id)
            .map_err(|_| Error::Authentication)?;
        let offer = Offer { id, kind };
        let opened = futures_util::future::join_all(
            selected
                .into_iter()
                .filter(|target| allowed.contains(target))
                .map(|target| {
                    let shared = &self.shared;
                    let offer = offer.clone();
                    let node = routes
                        .borrow()
                        .membership_config
                        .membership()
                        .get_node(&target)
                        .cloned();
                    async move {
                        let node = node.ok_or(Error::Denied)?;
                        let client = Client {
                            shared: Arc::downgrade(shared),
                            target,
                            node: node.clone(),
                        };
                        let (response, connection) = client
                            .exchange_on(
                                Request::MediaOffer(offer),
                                RPCOption::new(config::DEFAULT_TIMEOUT),
                            )
                            .await?;
                        match response {
                            Response::Media(DataReply::Accepted(room)) if room == shared.room => {}
                            Response::Media(DataReply::Denied) => return Err(Error::Denied),
                            _ => return Err(Error::Protocol),
                        }
                        if connection
                            .max_datagram_size()
                            .is_none_or(|size| size < config::MEDIA_PACKET_BYTES + 16)
                        {
                            return Err(Error::Configuration(
                                "peer does not support bounded media datagrams",
                            ));
                        }
                        Ok(Target {
                            id: target,
                            node,
                            connection,
                        })
                    }
                }),
        )
        .await;
        let targets: Vec<_> = opened.into_iter().filter_map(Result::ok).collect();
        if targets.is_empty() {
            return Err(Error::Denied);
        }
        Ok(Publication {
            shared: Arc::downgrade(&self.shared),
            usage,
            offer,
            targets,
            statistics: Statistics::default(),
            next: Instant::now(),
            started: Instant::now(),
            routes,
        })
    }
}
impl Publication {
    pub fn recipients(&self) -> Vec<PeerId> {
        self.targets.iter().map(|target| target.id).collect()
    }
    pub async fn send(&mut self, packet: &Packet) -> Result<()> {
        if packet.codec() != self.offer.kind.codec()
            || self.started.elapsed() >= config::MEDIA_MAX_DURATION
        {
            return Err(Error::Protocol);
        }
        let rate = if packet.codec() == crate::media::Codec::Opus {
            config::MEDIA_AUDIO_MAX_RATE
        } else {
            config::MEDIA_VIDEO_MAX_RATE
        };
        let spacing =
            Duration::from_nanos((packet.bytes().len() as u64 + 16) * 8 * 1_000_000_000 / rate);
        let now = Instant::now();
        if self.next > now {
            tokio::time::sleep_until(self.next).await;
        }
        self.next = self.next.max(Instant::now()) + spacing;
        let shared = self.shared.upgrade().ok_or(Error::Cancelled)?;
        let mut bytes = Vec::with_capacity(packet.bytes().len() + 16);
        bytes.extend(self.offer.id);
        bytes.extend(packet.bytes());
        let mut any = false;
        let mut queued = false;
        let mut stale = false;
        for target in &self.targets {
            if self
                .routes
                .borrow()
                .membership_config
                .membership()
                .get_node(&target.id)
                .is_none_or(|node| node.endpoint != target.node.endpoint)
            {
                self.statistics.stale_routes += 1;
                stale = true;
                continue;
            }
            if !authorized(&shared, target.id)?
                || !self.usage.hub.allowed(
                    target.id,
                    false,
                    shared.media_epoch.load(Ordering::SeqCst),
                    self.usage.generation,
                )
            {
                self.statistics.denied += 1;
                continue;
            }
            any = true;
            match target.connection.send_datagram(bytes.clone().into()) {
                Ok(()) => {
                    self.statistics.sent += 1;
                    queued = true;
                }
                Err(_) => self.statistics.dropped += 1,
            }
        }
        if queued {
            Ok(())
        } else if any || stale {
            Err(Error::Network)
        } else {
            Err(Error::Denied)
        }
    }
    pub async fn finish(self) -> Result<Statistics> {
        if let Some(shared) = self.shared.upgrade() {
            futures_util::future::join_all(self.targets.iter().map(|target| {
                let client = Client {
                    shared: Arc::downgrade(&shared),
                    target: target.id,
                    node: target.node.clone(),
                };
                let id = self.offer.id;
                async move {
                    let _ = client
                        .exchange(
                            Request::MediaEnd(id),
                            RPCOption::new(config::DEFAULT_TIMEOUT),
                        )
                        .await;
                }
            }))
            .await;
        }
        Ok(self.statistics)
    }
}
