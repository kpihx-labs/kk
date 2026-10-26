//! Explicit, audience-bound availability hints and verified direct replica pulls.
use super::*;
use crate::files::{self, FileInfo, Receipt};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub(super) struct Offer {
    source: PathBuf,
    info: FileInfo,
    audience: BTreeSet<PeerId>,
    epoch: u64,
}

#[derive(Default)]
pub(super) struct Registry {
    epoch: Option<u64>,
    offers: BTreeMap<[u8; 32], Arc<Offer>>,
}
impl Registry {
    fn insert(&mut self, offer: Arc<Offer>) -> Result<()> {
        if self.epoch.is_some_and(|epoch| offer.epoch < epoch) {
            return Err(Error::Denied);
        }
        if self.epoch != Some(offer.epoch) {
            self.offers.clear();
            self.epoch = Some(offer.epoch);
        }
        if !self.offers.contains_key(&offer.info.sha256)
            && self.offers.len() >= config::ROOM_FILE_SHARES
        {
            return Err(Error::Node("room file share limit exhausted"));
        }
        self.offers.insert(offer.info.sha256, offer);
        Ok(())
    }
}

fn access(shared: &Shared, target: PeerId) -> Result<(crate::rooms::runtime::Handle, Peer)> {
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
        .get(&target)
        .cloned()
        .ok_or(Error::Denied)?;
    Ok((handle, peer))
}

fn selected(shared: &Shared, target: PeerId, hash: [u8; 32]) -> Result<Arc<Offer>> {
    shared
        .file_shares
        .read()
        .map_err(|_| Error::Protocol)?
        .offers
        .get(&hash)
        .filter(|offer| offer.audience.contains(&target))
        .cloned()
        .ok_or(Error::Denied)
}

fn current(shared: &Shared, target: PeerId, offer: &Arc<Offer>) -> Result<()> {
    let live = selected(shared, target, offer.info.sha256)?;
    if !authorized(shared, target)? || !Arc::ptr_eq(offer, &live) {
        return Err(Error::Denied);
    }
    Ok(())
}

async fn authorize(
    shared: &Shared,
    target: PeerId,
    offer: &Arc<Offer>,
    handle: &crate::rooms::runtime::Handle,
    peer: &Peer,
) -> Result<()> {
    current(shared, target, offer)?;
    handle
        .authorize_file(peer.clone(), false, config::DEFAULT_TIMEOUT)
        .await?;
    if handle.status(config::DEFAULT_TIMEOUT).await?.epoch != offer.epoch {
        return Err(Error::Denied);
    }
    current(shared, target, offer)
}

impl Factory {
    pub(crate) async fn share_file(
        &self,
        source: &Path,
        audience: BTreeSet<PeerId>,
    ) -> Result<FileInfo> {
        if audience.is_empty()
            || audience.len() >= config::ROOM_MAX_MEMBERS
            || source.as_os_str().len() > config::ROOM_FILE_SHARE_PATH_BYTES
        {
            return Err(Error::Configuration(
                "select a bounded file path and exact remote audience",
            ));
        }
        let _permit = self
            .shared
            .file_data
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Node("room file lane is full"))?;
        tokio::time::timeout(config::DEFAULT_FILE_TIMEOUT, async {
            let info = files::inspect(source, config::DEFAULT_FILE_TIMEOUT).await?;
            let mut epoch = None;
            for target in &audience {
                if *target == PeerId::from_peer(&self.shared.identity.peer()) {
                    return Err(Error::Denied);
                }
                let (handle, peer) = access(&self.shared, *target)?;
                handle
                    .authorize_file(peer, false, config::DEFAULT_TIMEOUT)
                    .await?;
                let current = handle.status(config::DEFAULT_TIMEOUT).await?.epoch;
                if epoch.is_some_and(|previous| previous != current) {
                    return Err(Error::Denied);
                }
                epoch = Some(current);
            }
            let epoch = epoch.ok_or(Error::Denied)?;
            let mut shares = self
                .shared
                .file_shares
                .write()
                .map_err(|_| Error::Protocol)?;
            shares.insert(Arc::new(Offer {
                source: source.to_owned(),
                info: info.clone(),
                audience,
                epoch,
            }))?;
            Ok(info)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }

    pub(crate) fn unshare_file(&self, hash: [u8; 32]) -> Result<bool> {
        Ok(self
            .shared
            .file_shares
            .write()
            .map_err(|_| Error::Protocol)?
            .offers
            .remove(&hash)
            .is_some())
    }

    pub(crate) async fn file_availability(
        &self,
        target: PeerId,
        node: &Node,
        hash: [u8; 32],
        deadline: Duration,
    ) -> Result<Option<FileInfo>> {
        transport::validate_duration(deadline, config::DEFAULT_TIMEOUT)?;
        tokio::time::timeout(deadline, async {
            let (handle, peer) = access(&self.shared, target)?;
            handle
                .authorize_file(peer, true, config::DEFAULT_TIMEOUT)
                .await?;
            let client = Client {
                shared: Arc::downgrade(&self.shared),
                target,
                node: node.clone(),
            };
            match client
                .exchange(Request::FileAvailability(hash), RPCOption::new(deadline))
                .await?
            {
                Response::FileAvailability(info) => {
                    if info.as_ref().is_some_and(|info| {
                        info.sha256 != hash || info.bytes > config::MAX_FILE_BYTES
                    }) {
                        return Err(Error::Protocol);
                    }
                    Ok(info)
                }
                _ => Err(Error::Protocol),
            }
        })
        .await
        .map_err(|_| Error::Timeout)?
    }

    pub(crate) async fn fetch_file(
        &self,
        target: PeerId,
        node: &Node,
        hash: [u8; 32],
        deadline: Duration,
    ) -> Result<Receipt> {
        transport::validate_duration(deadline, config::DEFAULT_FILE_TIMEOUT)?;
        let shared = &self.shared;
        let inbox = shared
            .room_files
            .read()
            .map_err(|_| Error::Protocol)?
            .clone()
            .ok_or(Error::Denied)?;
        let _permit = shared
            .file_data
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Node("room file lane is full"))?;
        let (handle, peer) = access(shared, target)?;
        let client = Client {
            shared: Arc::downgrade(shared),
            target,
            node: node.clone(),
        };
        let mut used = None;
        let result = tokio::time::timeout(deadline, async {
            handle
                .authorize_file(peer.clone(), true, config::DEFAULT_TIMEOUT)
                .await?;
            let connection =
                tokio::time::timeout(config::DEFAULT_TIMEOUT, client.connection(shared))
                    .await
                    .map_err(|_| Error::Timeout)??;
            used = Some(connection.stable_id());
            if !authorized(shared, target)? {
                return Err(Error::Denied);
            }
            let (mut send, mut recv) = connection.open_bi().await.map_err(|_| Error::Network)?;
            send.set_priority(-2).map_err(|_| Error::Network)?;
            send_frame(
                &mut send,
                &Envelope {
                    room: shared.room,
                    request: Request::FileFetch(hash),
                },
            )
            .await?;
            match tokio::time::timeout(config::DEFAULT_TIMEOUT, read_prefix(&mut recv))
                .await
                .map_err(|_| Error::Timeout)??
            {
                Response::RoomFile(DataReply::Accepted(room)) if room == shared.room => {}
                Response::RoomFile(DataReply::Denied) => return Err(Error::Denied),
                Response::RoomFile(DataReply::Busy) => {
                    return Err(Error::Node("remote replica file lane is full"));
                }
                _ => return Err(Error::Protocol),
            }
            let receipt = files::stream::receive_with_async(
                &peer,
                recv,
                send,
                || handle.authorize_file(peer.clone(), true, config::DEFAULT_TIMEOUT),
                |info| {
                    if info.sha256 != hash {
                        return Err(Error::Authentication);
                    }
                    inbox.choose(&peer, info)
                },
            )
            .await?;
            inbox.complete(&receipt);
            Ok(receipt)
        })
        .await
        .unwrap_or(Err(Error::Timeout));
        if matches!(&result, Err(Error::Timeout | Error::Network)) {
            client.retire_unresponsive(shared, used);
        }
        result
    }
}

pub(super) async fn advertisement(
    shared: &Shared,
    target: PeerId,
    hash: [u8; 32],
) -> Result<FileInfo> {
    let offer = selected(shared, target, hash)?;
    let (handle, peer) = access(shared, target)?;
    authorize(shared, target, &offer, &handle, &peer).await?;
    // A hint only: a later fetch reopens and verifies the actual stored content.
    Ok(offer.info.clone())
}

pub(super) async fn serve(
    shared: &Shared,
    target: PeerId,
    hash: [u8; 32],
    mut send: quinn::SendStream,
    recv: quinn::RecvStream,
) -> Result<()> {
    let prepared = async {
        let permit = shared
            .file_data
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Node("room file lane is full"))?;
        let offer = selected(shared, target, hash)?;
        let (handle, peer) = access(shared, target)?;
        authorize(shared, target, &offer, &handle, &peer).await?;
        let mut file = files::source_file(&offer.source)?;
        let info = files::info(&mut file).await?;
        if info.sha256 != hash || info.bytes != offer.info.bytes {
            return Err(Error::Authentication);
        }
        authorize(shared, target, &offer, &handle, &peer).await?;
        Ok::<_, Error>((permit, offer, handle, peer, file, info))
    }
    .await;
    let Ok((_permit, offer, handle, peer, mut file, info)) = prepared else {
        send_frame(&mut send, &Response::RoomFile(DataReply::Denied)).await?;
        send.finish().map_err(|_| Error::Network)?;
        return Err(Error::Denied);
    };
    send_frame(
        &mut send,
        &Response::RoomFile(DataReply::Accepted(shared.room)),
    )
    .await?;
    files::stream::send_with_async(&mut file, info, false, send, recv, || async {
        authorize(shared, target, &offer, &handle, &peer).await
    })
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(epoch: u64, hash: u8) -> Arc<Offer> {
        Arc::new(Offer {
            source: PathBuf::from("owned-test-copy"),
            info: FileInfo {
                bytes: 1,
                sha256: [hash; 32],
            },
            audience: BTreeSet::from([PeerId([2; 32])]),
            epoch,
        })
    }

    #[test]
    fn delayed_publication_cannot_erase_newer_epoch_offers_even_after_withdrawal() {
        let mut registry = Registry::default();
        let current = offer(3, 1);
        registry.insert(current.clone()).unwrap();
        assert!(registry.insert(offer(2, 2)).is_err());
        assert!(Arc::ptr_eq(&registry.offers[&[1; 32]], &current));
        registry.offers.remove(&[1; 32]);
        assert!(registry.insert(offer(2, 1)).is_err());
        assert!(registry.offers.is_empty());
        registry.insert(offer(3, 2)).unwrap();
        registry.insert(offer(4, 3)).unwrap();
        assert_eq!(registry.offers.len(), 1);
        assert!(registry.offers.contains_key(&[3; 32]));
    }

    #[test]
    fn offer_budget_is_bounded_but_replacing_one_offer_does_not_spend_another_slot() {
        let mut registry = Registry::default();
        for index in 0..config::ROOM_FILE_SHARES {
            registry
                .insert(offer(1, u8::try_from(index).unwrap()))
                .unwrap();
        }
        assert!(registry.insert(offer(1, 255)).is_err());
        assert_eq!(registry.offers.len(), config::ROOM_FILE_SHARES);
        registry.insert(offer(1, 0)).unwrap();
        assert_eq!(registry.offers.len(), config::ROOM_FILE_SHARES);
        registry.insert(offer(2, 255)).unwrap();
        assert_eq!(registry.offers.len(), 1);
    }
}
