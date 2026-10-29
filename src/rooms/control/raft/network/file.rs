//! Consented direct room files reuse the existing verified-prefix file protocol.
use super::*;
use crate::files::{self, Inbox, Receipt};
use std::path::{Path, PathBuf};

/// Accounting lasts for the service lifetime; each consent grant has a distinct
/// identity so withdrawal followed by re-enabling cannot revive old transfers.
#[derive(Default)]
pub(super) struct Consent {
    configured: Option<(PathBuf, u64, u64, Arc<Inbox>)>,
    pub(super) active: Option<Arc<Grant>>,
}

pub(super) struct Grant {
    inbox: Arc<Inbox>,
    // Held only for a synchronous no-replace rename or revocation, never across
    // an await, file sync, hashing, transport work or the test barrier.
    publication: std::sync::Mutex<bool>,
    #[cfg(test)]
    pause: std::sync::Mutex<Option<PublicationPause>>,
}

#[cfg(test)]
struct PublicationPause {
    reached: tokio::sync::oneshot::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
    finished: tokio::sync::oneshot::Sender<bool>,
}

impl files::stream::Publication for Arc<Grant> {
    fn publish(self, rename: impl FnOnce() -> Result<()>) -> Result<()> {
        #[cfg(test)]
        let finished = {
            let pause = self.pause.lock().map_err(|_| Error::Protocol)?.take();
            if let Some(pause) = pause {
                pause.reached.send(()).map_err(|_| Error::Cancelled)?;
                pause.release.recv().map_err(|_| Error::Cancelled)?;
                Some(pause.finished)
            } else {
                None
            }
        };
        let result = (|| {
            let active = self.publication.lock().map_err(|_| Error::Protocol)?;
            if !*active {
                return Err(Error::Denied);
            }
            // The rename and withdrawal share one linearization gate. Once
            // revoke returns, this grant cannot publish again.
            rename()
        })();
        #[cfg(test)]
        if let Some(finished) = finished {
            // Some tests only need the arrival barrier, not the worker result.
            let _ = finished.send(result.is_ok());
        }
        result
    }
}

impl Grant {
    fn revoke(&self) {
        // A poisoned gate is still writable. Infallible lifecycle stops must
        // disable it rather than silently skipping revocation or panicking.
        *self
            .publication
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
    }
}

impl std::ops::Deref for Grant {
    type Target = Inbox;

    fn deref(&self) -> &Inbox {
        &self.inbox
    }
}

impl Consent {
    pub(super) fn revoke(&mut self) {
        if let Some(grant) = self.active.take() {
            grant.revoke();
        }
    }

    fn configure(
        &mut self,
        directory: Option<PathBuf>,
        max_bytes: u64,
        budget: u64,
    ) -> Result<bool> {
        let Some(directory) = directory else {
            self.revoke();
            return Ok(false);
        };
        let inbox = if let Some((path, maximum, total, inbox)) = &self.configured {
            if path != &directory || *maximum != max_bytes || *total != budget {
                return Err(Error::Configuration(
                    "restart room service to change inbox limits or path",
                ));
            }
            inbox.clone()
        } else {
            let inbox = Arc::new(Inbox::new(directory.clone(), max_bytes, budget)?);
            self.configured = Some((directory, max_bytes, budget, inbox.clone()));
            inbox
        };
        if self.active.is_none() {
            self.active = Some(Arc::new(Grant {
                inbox,
                publication: std::sync::Mutex::new(true),
                #[cfg(test)]
                pause: std::sync::Mutex::new(None),
            }));
        }
        Ok(true)
    }
}

impl Drop for Consent {
    fn drop(&mut self) {
        self.revoke();
    }
}

impl Factory {
    /// Pause one publication after final authorization and verification, on its
    /// blocking worker and before acquiring the revocation/rename gate.
    #[cfg(all(test, unix))]
    pub(crate) fn pause_file_publication(
        &self,
    ) -> Result<(
        tokio::sync::oneshot::Receiver<()>,
        std::sync::mpsc::Sender<()>,
        tokio::sync::oneshot::Receiver<bool>,
    )> {
        let consent = self.shared.room_files.read().map_err(|_| Error::Protocol)?;
        let grant = consent.active.as_ref().ok_or(Error::Denied)?;
        let mut pause = grant.pause.lock().map_err(|_| Error::Protocol)?;
        if pause.is_some() {
            return Err(Error::Node("file publication is already paused"));
        }
        let (reached, ready) = tokio::sync::oneshot::channel();
        let (release, receiver) = std::sync::mpsc::channel();
        let (finished, result) = tokio::sync::oneshot::channel();
        *pause = Some(PublicationPause {
            reached,
            release: receiver,
            finished,
        });
        Ok((ready, release, result))
    }

    pub(super) fn revoke_files(&self) {
        self.shared
            .room_files
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .revoke();
    }

    pub(crate) fn configure_files(
        &self,
        directory: Option<PathBuf>,
        max_bytes: u64,
        budget: u64,
    ) -> Result<bool> {
        let mut consent = self
            .shared
            .room_files
            .write()
            .map_err(|_| Error::Protocol)?;
        if self.shared.stopping.load(Ordering::SeqCst) {
            consent.revoke();
            return Err(Error::Cancelled);
        }
        consent.configure(directory, max_bytes, budget)
    }

    pub(crate) fn files_enabled(&self) -> Result<bool> {
        Ok(self
            .shared
            .room_files
            .read()
            .map_err(|_| Error::Protocol)?
            .active
            .is_some())
    }

    pub(crate) fn enable_files(
        &self,
        directory: PathBuf,
        max_bytes: u64,
        budget: u64,
    ) -> Result<()> {
        let mut selected = self
            .shared
            .room_files
            .write()
            .map_err(|_| Error::Protocol)?;
        if self.shared.stopping.load(Ordering::SeqCst) {
            selected.revoke();
            return Err(Error::Cancelled);
        }
        if selected.configured.is_some() {
            return Err(Error::Configuration("room inbox is already configured"));
        }
        selected.configure(Some(directory), max_bytes, budget)?;
        Ok(())
    }
    pub(crate) async fn room_file(
        &self,
        target: PeerId,
        node: &Node,
        source: &Path,
        sensitive: bool,
        deadline: Duration,
    ) -> Result<Receipt> {
        transport::validate_duration(deadline, config::DEFAULT_FILE_TIMEOUT)?;
        let shared = &self.shared;
        let _permit = shared
            .file_data
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Node("room file lane is full"))?;
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
        let client = Client {
            shared: Arc::downgrade(shared),
            target,
            node: node.clone(),
        };
        let mut used = None;
        let result = tokio::time::timeout(deadline, async {
            handle
                .authorize_file(peer.clone(), false, config::DEFAULT_TIMEOUT)
                .await?;
            let mut file = files::source_file(source)?;
            let info = files::info(&mut file).await?;
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
                    request: Request::RoomFile,
                },
            )
            .await?;
            let response = tokio::time::timeout(config::DEFAULT_TIMEOUT, read_prefix(&mut recv))
                .await
                .map_err(|_| Error::Timeout)??;
            match response {
                Response::RoomFile(DataReply::Accepted(room)) if room == shared.room => {}
                Response::RoomFile(DataReply::Denied) => return Err(Error::Denied),
                Response::RoomFile(DataReply::Busy) => {
                    return Err(Error::Node("remote room inbox is busy"));
                }
                _ => return Err(Error::Protocol),
            }
            files::stream::send_with_async(&mut file, info, sensitive, send, recv, || {
                handle.authorize_file(peer.clone(), false, config::DEFAULT_TIMEOUT)
            })
            .await
        })
        .await
        .unwrap_or(Err(Error::Timeout));
        if matches!(&result, Err(Error::Timeout | Error::Network)) {
            client.retire_unresponsive(shared, used);
        }
        result
    }
}

pub(super) fn current_inbox(shared: &Shared, inbox: &Arc<Grant>) -> Result<()> {
    let current = shared.room_files.read().map_err(|_| Error::Protocol)?;
    if current
        .active
        .as_ref()
        .is_some_and(|selected| Arc::ptr_eq(selected, inbox))
    {
        Ok(())
    } else {
        Err(Error::Denied)
    }
}

// Test-only numeric endpoint/stream evidence; never identities, paths or payloads.
// The first three streams cover revoked, fresh and exhausted-budget offers.
#[cfg(test)]
pub(super) fn trace(shared: &Shared, stream: quinn::StreamId, phase: &str) {
    if stream.index() < 3 {
        eprintln!(
            "room stream {:?}/{stream:?}: {phase}",
            shared.endpoint.local_addr()
        );
    }
}

pub(super) async fn receive(
    shared: &Shared,
    id: PeerId,
    mut send: quinn::SendStream,
    recv: quinn::RecvStream,
) -> Result<()> {
    #[cfg(test)]
    trace(shared, send.id(), "receive entered");
    let inbox = shared
        .room_files
        .read()
        .map_err(|_| Error::Protocol)?
        .active
        .clone();
    let handle = shared
        .room_port
        .read()
        .map_err(|_| Error::Protocol)?
        .clone();
    let permit = shared.file_data.clone().try_acquire_owned();
    let (Some(inbox), Some(handle), Ok(_permit)) = (inbox, handle, permit) else {
        send_frame(&mut send, &Response::RoomFile(DataReply::Denied)).await?;
        send.finish().map_err(|_| Error::Network)?;
        return Err(Error::Denied);
    };
    let peer = shared
        .peers
        .read()
        .map_err(|_| Error::Protocol)?
        .get(&id)
        .cloned()
        .ok_or(Error::Denied)?;
    #[cfg(test)]
    trace(shared, send.id(), "authorize start");
    if handle
        .authorize_file(peer.clone(), true, config::DEFAULT_TIMEOUT)
        .await
        .is_err()
    {
        send_frame(&mut send, &Response::RoomFile(DataReply::Denied)).await?;
        send.finish().map_err(|_| Error::Network)?;
        return Err(Error::Denied);
    }
    #[cfg(test)]
    trace(shared, send.id(), "authorize passed");
    send_frame(
        &mut send,
        &Response::RoomFile(DataReply::Accepted(shared.room)),
    )
    .await?;
    #[cfg(test)]
    trace(shared, send.id(), "Accepted queued");
    let receipt = files::stream::receive_with_publication(
        &peer,
        recv,
        send,
        || async {
            current_inbox(shared, &inbox)?;
            handle
                .authorize_file(peer.clone(), true, config::DEFAULT_TIMEOUT)
                .await?;
            current_inbox(shared, &inbox)
        },
        |info| inbox.choose(&peer, info),
        inbox.clone(),
    )
    .await;
    // The wire peer only observes EOF on rejection; preserve the actual receiver
    // failure in test output rather than diagnosing it from FinishedEarly alone.
    #[cfg(test)]
    if let Err(error) = &receipt {
        eprintln!("room file receive failed: {error:?}");
    }
    let receipt = receipt?;
    inbox.complete(&receipt);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use files::stream::Publication;

    #[test]
    fn consent_drop_revokes_worker_grants_even_when_the_grant_gate_is_poisoned() {
        for poison in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut consent = Consent::default();
            consent
                .configure(Some(root.path().join("inbox")), 1, 1)
                .unwrap();
            let grant = consent.active.clone().unwrap();
            if poison {
                let broken = grant.clone();
                assert!(
                    std::thread::spawn(move || {
                        let _gate = broken.publication.lock().unwrap();
                        panic!("intentional publication gate poison");
                    })
                    .join()
                    .is_err()
                );
            }
            drop(consent);
            assert!(
                !*grant
                    .publication
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
            );
            assert!(
                grant
                    .publish(|| panic!("a dropped owner's grant must not publish"))
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn lifecycle_revocation_recovers_poisoned_consent_and_publication_locks() {
        for stop in 0..5 {
            let root = tempfile::tempdir().unwrap();
            let identity = Arc::new(Identity::create(&root.path().join("identity")).unwrap());
            let (factory, server) = Factory::bind(
                [7; 32],
                identity.clone(),
                BTreeMap::from([(PeerId::from_peer(&identity.peer()), identity.peer())]),
                "127.0.0.1:0".parse().unwrap(),
            )
            .unwrap();
            let observer = factory.clone();
            factory
                .enable_files(root.path().join("inbox"), 1, 1)
                .unwrap();
            let grant = factory
                .shared
                .room_files
                .read()
                .unwrap()
                .active
                .clone()
                .unwrap();
            let broken_grant = grant.clone();
            let broken = factory.clone();
            assert!(
                std::thread::spawn(move || {
                    let _consent = broken.shared.room_files.write().unwrap();
                    let _gate = broken_grant.publication.lock().unwrap();
                    let _port = broken.shared.room_port.write().unwrap();
                    panic!("intentional consent and publication poison");
                })
                .join()
                .is_err()
            );
            match stop {
                0 => factory.close(),
                1 => factory.fail_closed(),
                2 => assert!(matches!(factory.unregister_room(), Err(Error::Protocol))),
                3 => drop(factory),
                _ => drop(server),
            }
            let consent = observer
                .shared
                .room_files
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert!(
                consent.active.is_none(),
                "stop must not silently skip a poisoned grant"
            );
            assert!(
                !*grant
                    .publication
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
            );
        }
    }

    #[test]
    fn consent_toggles_preserve_byte_and_attempt_budgets() {
        let root = tempfile::tempdir().unwrap();
        let identity = Identity::create(&root.path().join("identity")).unwrap();
        let peer = identity.peer();
        let directory = root.path().join("inbox");
        for (bytes, budget) in [(1, 1), (0, 0)] {
            let mut consent = Consent::default();
            let path = directory.join(format!("{bytes}"));
            consent.configure(Some(path.clone()), 1, budget).unwrap();
            let first = consent.active.clone().unwrap();
            let info = files::FileInfo {
                bytes,
                sha256: [0; 32],
            };
            let offers = if bytes == 0 {
                config::NODE_FILE_COUNT_BUDGET
            } else {
                1
            };
            for _ in 0..offers {
                first.choose(&peer, &info).unwrap();
            }
            let remaining = first.remaining();
            consent.configure(Some(path.clone()), 1, budget).unwrap();
            assert!(Arc::ptr_eq(&first, consent.active.as_ref().unwrap()));
            assert_eq!(consent.active.as_ref().unwrap().remaining(), remaining);
            consent.configure(None, 1, budget).unwrap();
            assert!(consent.active.is_none());
            consent.configure(Some(path.clone()), 1, budget).unwrap();
            let next = consent.active.as_ref().unwrap();
            assert!(
                !Arc::ptr_eq(&first, next),
                "old transfer grant must stay revoked"
            );
            assert_eq!(next.remaining(), remaining);
            assert!(next.choose(&peer, &info).is_err());
            assert!(consent.configure(Some(path), 1, budget + 1).is_err());
        }
    }
}
