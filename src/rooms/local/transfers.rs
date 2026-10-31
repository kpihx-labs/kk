//! Foreground-owned outgoing file jobs. No global registry or transport protocol.
use super::ControlNode;
use crate::{Error, Result, config, files, rooms::control::raft::PeerId};
use files::progress::{self, TransferContext, TransferControl, TransferOutcome, TransferProgress};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    future::Future,
    path::Path,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

#[path = "transfer_journal.rs"]
mod journal;
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
    time::Instant,
};

pub(super) struct Admission {
    slot: OwnedSemaphorePermit,
    expires: Instant,
}

impl Admission {
    pub fn remaining(&self) -> Result<Duration> {
        self.expires
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or(Error::Timeout)
    }
}

/// A receipt summary deliberately contains no content digest, even for ordinary
/// transfers. Only the original synchronous API returns full receipt metadata.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VerifiedReceipt {
    bytes: u64,
    resumed_from: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobProgress {
    phase: progress::TransferPhase,
    total_bytes: Option<u64>,
    resumed_from: u64,
    source_bytes: u64,
    handed_off: bool,
    error: Option<String>,
}

impl From<TransferProgress> for JobProgress {
    fn from(value: TransferProgress) -> Self {
        Self {
            phase: value.phase,
            total_bytes: value.total_bytes,
            resumed_from: value.resumed_from,
            source_bytes: value.source_bytes,
            handed_off: value.handed_off,
            error: value.error.map(str::to_owned),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct JobStatus {
    id: String,
    progress: JobProgress,
    cancel_requested: bool,
    receipt: Option<VerifiedReceipt>,
    terminal_durable: bool,
    historical: bool,
    recovered: bool,
    retry_allowed: bool,
}

impl JobStatus {
    fn retryable(&self) -> bool {
        self.terminal_durable
            && !self.recovered
            && !self.progress.handed_off
            && matches!(
                self.progress.phase,
                progress::TransferPhase::Cancelled | progress::TransferPhase::Failed
            )
            && self.receipt.is_none()
    }

    fn validate(&self, key: u64) -> Result<()> {
        let invalid = || Error::Configuration("invalid file job checkpoint");
        let bytes = self.id.as_bytes();
        if bytes.len() != 49
            || bytes[32] != b'-'
            || !bytes[..32].iter().all(u8::is_ascii_hexdigit)
            || u64::from_str_radix(&self.id[33..], 16).ok() != Some(key)
        {
            return Err(invalid());
        }
        let total = self.progress.total_bytes.unwrap_or(0);
        if total > config::MAX_FILE_BYTES
            || self.progress.resumed_from > total
            || self.progress.source_bytes > total - self.progress.resumed_from
        {
            return Err(invalid());
        }
        if let Some(error) = &self.progress.error
            && ![
                "IO",
                "IDENTITY",
                "UNSUPPORTED_PLATFORM",
                "AUTHENTICATION",
                "NETWORK",
                "TIMEOUT",
                "PROTOCOL",
                "MESSAGE_TOO_LARGE",
                "LIMIT",
                "CANCELLED",
                "CONFIGURATION",
                "SETTINGS_CONFLICT",
                "DISCOVERY",
                "INTEGRITY",
                "DESTINATION",
                "FILE_TOO_LARGE",
                "ENCRYPTION",
                "PAIRING",
                "DENIED",
                "REVOKED",
                "NODE",
                "STORAGE",
                "OWNER_RESTARTED",
                "STATE_NOT_DURABLE",
            ]
            .contains(&error.as_str())
        {
            return Err(invalid());
        }
        let completed = self.progress.phase == progress::TransferPhase::Completed;
        if completed != self.receipt.is_some() || (completed && self.progress.error.is_some()) {
            return Err(invalid());
        }
        if self.progress.phase.terminal() && !completed && self.progress.error.is_none() {
            return Err(invalid());
        }
        if !self.progress.phase.terminal() && self.progress.error.is_some() {
            return Err(invalid());
        }
        if self.progress.phase == progress::TransferPhase::Cancelled
            && self.progress.error.as_deref() != Some("CANCELLED")
        {
            return Err(invalid());
        }
        if matches!(
            self.progress.phase,
            progress::TransferPhase::Cancelled | progress::TransferPhase::Failed
        ) && self.progress.handed_off
        {
            return Err(invalid());
        }
        if let Some(receipt) = &self.receipt
            && (receipt.bytes > config::MAX_FILE_BYTES || receipt.resumed_from > receipt.bytes)
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Serialize)]
pub(super) struct CancelReply {
    requested: bool,
    acknowledged: bool,
    job: JobStatus,
}

#[derive(Clone)]
struct Record {
    control: TransferControl,
    cancel_requested: bool,
    receipt: Rc<RefCell<Option<VerifiedReceipt>>>,
    durable: Rc<Cell<bool>>,
}

impl Record {
    fn raw_status(&self, owner: &str, key: u64) -> JobStatus {
        let mut progress = JobProgress::from(self.control.subscribe().borrow().clone());
        let receipt = self.receipt.borrow().clone();
        if let Some(receipt) = &receipt {
            progress.total_bytes = Some(receipt.bytes);
            progress.resumed_from = receipt.resumed_from;
            progress.source_bytes = progress
                .source_bytes
                .min(receipt.bytes - receipt.resumed_from);
        }
        let mut job = JobStatus {
            id: format!("{owner}-{key:016x}"),
            progress,
            cancel_requested: self.cancel_requested,
            receipt,
            terminal_durable: self.durable.get(),
            historical: false,
            recovered: false,
            retry_allowed: false,
        };
        job.retry_allowed = job.retryable();
        job
    }

    fn status(&self, owner: &str, key: u64) -> JobStatus {
        let mut job = self.raw_status(owner, key);
        if job.progress.phase.terminal() && !job.terminal_durable {
            job.progress.phase = progress::TransferPhase::Uncertain;
            job.progress.error = Some("STATE_NOT_DURABLE".to_owned());
            job.receipt = None;
            job.retry_allowed = false;
        }
        job
    }
}

#[derive(Clone)]
struct State {
    next: u64,
    closing: bool,
    records: BTreeMap<u64, Record>,
    history: BTreeMap<u64, JobStatus>,
}

fn checkpoint(journal: &journal::Journal, state: &RefCell<State>, owner: &str) -> Result<()> {
    let state = state.borrow();
    journal.save(&state, owner)?;
    for record in state.records.values() {
        if record.control.subscribe().borrow().phase.terminal() {
            record.durable.set(true);
        }
    }
    Ok(())
}

pub(super) struct Transfers {
    owner: String,
    slots: Arc<Semaphore>,
    state: Rc<RefCell<State>>,
    journal: Rc<journal::Journal>,
    tasks: RefCell<JoinSet<()>>,
}

impl Transfers {
    pub fn new(directory: &Path) -> Result<Rc<Self>> {
        if config::ROOM_FILE_JOB_RECORDS < config::NODE_MAX_FILES || config::NODE_MAX_FILES == 0 {
            return Err(Error::Configuration(
                "file job record cap must cover active slots",
            ));
        }
        let mut nonce = [0; 16];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| Error::Node("file job owner randomness unavailable"))?;
        let journal = Rc::new(journal::Journal::open(directory)?);
        let (next, history) = journal.load()?;
        let jobs = Rc::new(Self {
            owner: nonce.iter().map(|byte| format!("{byte:02x}")).collect(),
            slots: Arc::new(Semaphore::new(config::NODE_MAX_FILES)),
            state: Rc::new(RefCell::new(State {
                next,
                closing: false,
                records: BTreeMap::new(),
                history,
            })),
            journal,
            tasks: RefCell::new(JoinSet::new()),
        });
        jobs.checkpoint()?;
        Ok(jobs)
    }

    fn checkpoint(&self) -> Result<()> {
        checkpoint(&self.journal, &self.state, &self.owner)
    }

    /// An old ID is not a recipe. This only admits a fresh source after a known,
    /// durably recorded pre-handoff failure; uncertain/completed work cannot replay.
    pub fn reserve_retry(&self, previous: &str) -> Result<Admission> {
        if !self.status(previous)?.retry_allowed {
            return Err(Error::Denied);
        }
        self.reserve()
    }

    /// Reserve before source verification so even pre-ack work is bounded. The
    /// absolute expiry follows the reservation through verification and sending.
    pub fn reserve(&self) -> Result<Admission> {
        self.reap();
        if self.state.borrow().closing {
            return Err(Error::Cancelled);
        }
        let slot = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Node("outgoing file job slots are full"))?;
        Ok(Admission {
            slot,
            expires: Instant::now() + config::DEFAULT_FILE_TIMEOUT,
        })
    }

    /// This accepts only an already retained descriptor, never a queued pathname.
    pub fn start(
        &self,
        admission: Admission,
        control: Arc<ControlNode>,
        target: PeerId,
        source: files::SendSource,
        sensitive: bool,
    ) -> Result<JobStatus> {
        let deadline = admission.remaining()?;
        let (cancel, context) = progress::transfer_control();
        let observer = context.observer();
        self.launch(admission, cancel, context, async move {
            control
                .send_room_file_source_monitored(target, source, sensitive, deadline, observer)
                .await
        })
    }

    // File-specific result only. Factoring lifetime ownership here also permits
    // deterministic cancellation/retention tests without a simulated UI abort.
    fn launch(
        &self,
        admission: Admission,
        cancel: TransferControl,
        context: TransferContext,
        operation: impl Future<Output = Result<files::Receipt>> + 'static,
    ) -> Result<JobStatus> {
        admission.remaining()?;
        self.reap();
        let receipt = Rc::new(RefCell::new(None));
        let previous = self.state.borrow().clone();
        let key = {
            let mut state = self.state.borrow_mut();
            if state.closing {
                return Err(Error::Cancelled);
            }
            while state.records.len() + state.history.len() >= config::ROOM_FILE_JOB_RECORDS {
                let live = state.records.iter().find_map(|(key, record)| {
                    (record.control.subscribe().borrow().phase.terminal() && record.durable.get())
                        .then_some(*key)
                });
                let historical = state.history.keys().next().copied();
                let oldest = live
                    .into_iter()
                    .chain(historical)
                    .min()
                    .ok_or(Error::Node("file job records are full"))?;
                state.records.remove(&oldest);
                state.history.remove(&oldest);
            }
            let key = state.next;
            state.next = state
                .next
                .checked_add(1)
                .ok_or(Error::Node("file job IDs exhausted"))?;
            state.records.insert(
                key,
                Record {
                    control: cancel,
                    cancel_requested: false,
                    receipt: receipt.clone(),
                    durable: Rc::new(Cell::new(false)),
                },
            );
            key
        };
        // A source is retained but unpolled until its intent is durable. A failed
        // checkpoint cannot acknowledge Started or cause a network effect.
        if let Err(error) = self.checkpoint() {
            // A rejected intent must not evict the last acknowledged terminals.
            // Keep the consumed sequence: even an unacknowledged attempt never
            // reuses an ID if an atomic rename preceded a durability failure.
            let next = self.state.borrow().next;
            let mut restored = previous;
            restored.next = next;
            *self.state.borrow_mut() = restored;
            return Err(error);
        }
        let state = self.state.clone();
        let journal = self.journal.clone();
        let owner = self.owner.clone();
        self.tasks.borrow_mut().spawn_local(async move {
            let _slot = admission.slot;
            let result = context
                .run(async {
                    tokio::time::timeout_at(admission.expires, operation)
                        .await
                        .unwrap_or(Err(Error::Timeout))
                })
                .await;
            match result {
                TransferOutcome::Completed(verified) => {
                    *receipt.borrow_mut() = Some(VerifiedReceipt {
                        bytes: verified.info.bytes,
                        resumed_from: verified.resumed_from,
                    });
                }
                other => drop(other.into_result()),
            }
            if let Err(error) = checkpoint(&journal, &state, &owner) {
                // Expose uncertain/non-durable, never a retryable terminal result.
                eprintln!("Room file terminal checkpoint failed: {}", error.code());
            }
        });
        self.snapshot(key)
    }

    fn id(&self, key: u64) -> String {
        format!("{}-{key:016x}", self.owner)
    }

    fn key(&self, id: &str) -> Result<u64> {
        let bytes = id.as_bytes();
        if bytes.len() != 49 || bytes[32] != b'-' || !bytes[..32].iter().all(u8::is_ascii_hexdigit)
        {
            return Err(Error::Configuration("invalid file transfer ID"));
        }
        let key = u64::from_str_radix(&id[33..], 16)
            .map_err(|_| Error::Configuration("invalid file transfer ID"))?;
        let state = self.state.borrow();
        if (state.records.contains_key(&key) && self.id(key) == id)
            || state.history.get(&key).is_some_and(|job| job.id == id)
        {
            Ok(key)
        } else {
            Err(Error::Configuration(
                "unknown file transfer for this room owner",
            ))
        }
    }

    fn snapshot(&self, key: u64) -> Result<JobStatus> {
        let state = self.state.borrow();
        if let Some(job) = state.history.get(&key) {
            return Ok(job.clone());
        }
        let record = state
            .records
            .get(&key)
            .ok_or(Error::Configuration("unknown or retired file transfer"))?;
        Ok(record.status(&self.owner, key))
    }

    pub fn status(&self, id: &str) -> Result<JobStatus> {
        self.reap();
        self.snapshot(self.key(id)?)
    }

    pub fn list(&self) -> Result<Vec<JobStatus>> {
        self.reap();
        let state = self.state.borrow();
        let mut keys = state
            .records
            .keys()
            .chain(state.history.keys())
            .copied()
            .collect::<Vec<_>>();
        keys.sort_unstable();
        drop(state);
        keys.into_iter().map(|key| self.snapshot(key)).collect()
    }

    pub async fn cancel(&self, id: &str) -> Result<CancelReply> {
        self.reap();
        let key = self.key(id)?;
        if let Some(job) = self.state.borrow().history.get(&key).cloned() {
            return Ok(CancelReply {
                requested: false,
                acknowledged: job.terminal_durable,
                job,
            });
        }
        let (requested, record) = {
            let mut state = self.state.borrow_mut();
            let record = state
                .records
                .get_mut(&key)
                .ok_or(Error::Configuration("unknown or retired file transfer"))?;
            let updates = record.control.subscribe();
            let active = !updates.borrow().phase.terminal();
            if active {
                record.cancel_requested = true;
                record.control.cancel();
            }
            (active, record.clone())
        };
        let mut updates = record.control.subscribe();
        // No record/table lock is held while waiting for the actual owner drop.
        let _ = tokio::time::timeout(config::DEFAULT_TIMEOUT, async {
            while !updates.borrow().phase.terminal() {
                if updates.changed().await.is_err() {
                    break;
                }
            }
        })
        .await;
        // A reserved control waiter keeps its selected observation even if a
        // subsequent start evicts the now-terminal record from recent history.
        let job = record.status(&self.owner, key);
        Ok(CancelReply {
            requested,
            acknowledged: job.progress.phase.terminal() && job.terminal_durable,
            job,
        })
    }

    fn reap(&self) {
        while let Some(joined) = self.tasks.borrow_mut().try_join_next() {
            if joined.is_err() {
                eprintln!("Room outgoing file job terminated unexpectedly");
            }
        }
    }

    pub async fn shutdown(&self) -> Result<()> {
        {
            let mut state = self.state.borrow_mut();
            state.closing = true;
            for record in state.records.values_mut() {
                if !record.control.subscribe().borrow().phase.terminal() {
                    record.cancel_requested = true;
                    record.control.cancel();
                }
            }
        }
        let mut tasks = std::mem::take(&mut *self.tasks.borrow_mut());
        let mut clean = true;
        let drained = tokio::time::timeout(config::DEFAULT_TIMEOUT, async {
            while let Some(joined) = tasks.join_next().await {
                clean &= joined.is_ok();
            }
        })
        .await;
        if drained.is_err() {
            tasks.abort_all();
            while let Some(joined) = tasks.join_next().await {
                clean &= joined.is_ok();
            }
        }
        self.checkpoint()?;
        if clean {
            Ok(())
        } else {
            Err(Error::Node("outgoing file job shutdown required abort"))
        }
    }
}

impl Drop for Transfers {
    fn drop(&mut self) {
        // Normal foreground exit awaits shutdown; this is a panic/error fallback,
        // never permission for jobs to survive the owning session.
        self.tasks.get_mut().abort_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::progress::TransferPhase;
    use std::{cell::Cell, io::Write};
    use tokio::{
        io::{AsyncReadExt, AsyncSeekExt},
        sync::{oneshot, watch},
    };

    fn fixture() -> (tempfile::TempDir, Rc<Transfers>) {
        let root = crate::test_support::tempdir().unwrap();
        let jobs = Transfers::new(root.path()).unwrap();
        (root, jobs)
    }

    #[tokio::test]
    async fn restart_keeps_terminal_facts_without_replay_and_rejected_intent_preserves_view() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (root, jobs) = fixture();
                let (cancel, context) = progress::transfer_control();
                let cancelled = jobs
                    .launch(
                        jobs.reserve().unwrap(),
                        cancel,
                        context,
                        std::future::pending::<Result<files::Receipt>>(),
                    )
                    .unwrap();
                let ack = jobs.cancel(&cancelled.id).await.unwrap();
                assert!(ack.acknowledged && ack.job.terminal_durable && ack.job.retry_allowed);
                let (cancel, context) = progress::transfer_control();
                let completed = jobs
                    .launch(jobs.reserve().unwrap(), cancel, context, async {
                        Ok(files::Receipt {
                            info: files::FileInfo {
                                bytes: 8,
                                sha256: [7; 32],
                            },
                            resumed_from: 3,
                            sensitive: true,
                        })
                    })
                    .unwrap();
                assert!(terminal(&jobs, &completed.id).await.terminal_durable);

                // Deliberate owner loss, not graceful shutdown. The journal contains
                // only a durable intent, while a receiver may have an unpublished part.
                struct NotifyDrop(Option<oneshot::Sender<()>>);
                impl Drop for NotifyDrop {
                    fn drop(&mut self) {
                        if let Some(sender) = self.0.take() {
                            let _ = sender.send(());
                        }
                    }
                }
                let (cancel, context) = progress::transfer_control();
                let observer = context.observer();
                let (ready, started) = oneshot::channel();
                let (notice, dropped) = oneshot::channel();
                let guard = NotifyDrop(Some(notice));
                let interrupted = jobs
                    .launch(jobs.reserve().unwrap(), cancel, context, async move {
                        let _guard = guard;
                        observer.handoff();
                        ready.send(()).unwrap();
                        std::future::pending::<Result<files::Receipt>>().await
                    })
                    .unwrap();
                started.await.unwrap();
                let partial = root.path().join("receiver-copy.bin.kk-part");
                std::fs::write(&partial, b"unpublished partial").unwrap();
                drop(jobs);
                dropped.await.unwrap();

                let path = root.path().join("file-transfers.json");
                let mut image: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                let row = image["jobs"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|row| row[1]["id"] == interrupted.id)
                    .unwrap();
                // Even a last snapshot at 100% submitted bytes is not a receipt.
                row[1]["progress"]["phase"] = "awaiting-receipt".into();
                row[1]["progress"]["total_bytes"] = 4096.into();
                row[1]["progress"]["source_bytes"] = 4096.into();
                row[1]["progress"]["handed_off"] = true.into();
                std::fs::write(&path, serde_json::to_vec(&image).unwrap()).unwrap();

                let recovered = Transfers::new(root.path()).unwrap();
                let known = recovered.status(&cancelled.id).unwrap();
                assert_eq!(known.progress.phase, TransferPhase::Cancelled);
                assert!(known.historical && known.terminal_durable && known.retry_allowed);
                drop(recovered.reserve_retry(&cancelled.id).unwrap());
                let known = recovered.status(&completed.id).unwrap();
                assert_eq!(known.progress.phase, TransferPhase::Completed);
                assert!(known.terminal_durable && known.receipt.is_some());
                assert!(recovered.reserve_retry(&completed.id).is_err());
                let unknown = recovered.status(&interrupted.id).unwrap();
                assert_eq!(unknown.progress.phase, TransferPhase::Uncertain);
                assert_eq!(unknown.progress.error.as_deref(), Some("OWNER_RESTARTED"));
                assert!(unknown.historical && unknown.recovered && unknown.terminal_durable);
                assert!(!unknown.retry_allowed && unknown.receipt.is_none());
                assert!(recovered.reserve_retry(&interrupted.id).is_err());
                assert!(recovered.tasks.borrow().is_empty());
                assert_eq!(recovered.slots.available_permits(), config::NODE_MAX_FILES);
                assert_eq!(std::fs::read(&partial).unwrap(), b"unpublished partial");
                assert!(!root.path().join("receiver-copy.bin").exists());

                // A malformed failed checkpoint must never authorize a certain retry.
                let valid = std::fs::read(&path).unwrap();
                let mut corrupt: serde_json::Value = serde_json::from_slice(&valid).unwrap();
                let row = corrupt["jobs"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|row| row[1]["id"] == cancelled.id)
                    .unwrap();
                row[1]["progress"]["phase"] = "failed".into();
                row[1]["progress"]["error"] = serde_json::Value::Null;
                std::fs::write(&path, serde_json::to_vec(&corrupt).unwrap()).unwrap();
                assert!(Transfers::new(root.path()).is_err());
                std::fs::write(&path, valid).unwrap();

                // Fill the bounded view, then force a journal refusal without global
                // permission changes. No source future may run or terminal disappear.
                for _ in 0..config::ROOM_FILE_JOB_RECORDS {
                    let (cancel, context) = progress::transfer_control();
                    let job = recovered
                        .launch(recovered.reserve().unwrap(), cancel, context, async {
                            Err(Error::Denied)
                        })
                        .unwrap();
                    terminal(&recovered, &job.id).await;
                }
                let before = serde_json::to_value(recovered.list().unwrap()).unwrap();
                let alias = root.path().join("journal-alias");
                std::fs::hard_link(&path, &alias).unwrap();
                let polled = Rc::new(Cell::new(false));
                let observed = polled.clone();
                let (cancel, context) = progress::transfer_control();
                let result =
                    recovered.launch(recovered.reserve().unwrap(), cancel, context, async move {
                        observed.set(true);
                        std::future::pending::<Result<files::Receipt>>().await
                    });
                assert!(matches!(result, Err(Error::Destination(_))));
                assert!(!polled.get());
                assert_eq!(
                    serde_json::to_value(recovered.list().unwrap()).unwrap(),
                    before
                );
                std::fs::remove_file(alias).unwrap();
                recovered.shutdown().await.unwrap();
            })
            .await;
    }

    async fn terminal(jobs: &Transfers, id: &str) -> JobStatus {
        let mut updates = jobs.state.borrow().records[&jobs.key(id).unwrap()]
            .control
            .subscribe();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !updates.borrow().phase.terminal() {
                updates.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        jobs.status(id).unwrap()
    }

    struct Dropped {
        flag: Rc<Cell<bool>>,
        progress: watch::Receiver<TransferProgress>,
    }
    impl Drop for Dropped {
        fn drop(&mut self) {
            assert!(
                !self.progress.borrow().phase.terminal(),
                "terminal acknowledgement preceded source drop"
            );
            self.flag.set(true);
        }
    }

    #[tokio::test]
    async fn cancel_acknowledges_owned_drop_and_preserves_post_handoff_uncertainty() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (_root, jobs) = fixture();
                let (cancel, context) = progress::transfer_control();
                let observer = context.observer();
                let flag = Rc::new(Cell::new(false));
                let guard = Dropped {
                    flag: flag.clone(),
                    progress: cancel.subscribe(),
                };
                let (ready, reached) = oneshot::channel();
                let job = jobs
                    .launch(jobs.reserve().unwrap(), cancel, context, async move {
                        let _guard = guard;
                        observer.handoff();
                        ready.send(()).unwrap();
                        std::future::pending::<Result<files::Receipt>>().await
                    })
                    .unwrap();
                reached.await.unwrap();
                assert!(!jobs.status(&job.id).unwrap().progress.phase.terminal());
                let reply = jobs.cancel(&job.id).await.unwrap();
                assert!(reply.requested && reply.acknowledged);
                assert!(flag.get());
                assert_eq!(reply.job.progress.phase, TransferPhase::Uncertain);
                assert_eq!(reply.job.progress.error.as_deref(), Some("CANCELLED"));
                assert!(reply.job.receipt.is_none());
                jobs.shutdown().await.unwrap();
            })
            .await;
    }

    #[tokio::test]
    async fn queued_upload_keeps_its_descriptor_after_http_temp_guard_cleanup() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (_root, jobs) = fixture();
                let mut upload = tempfile::NamedTempFile::new().unwrap();
                upload.write_all(b"private queued upload").unwrap();
                let source = files::SendSource::capture(upload.path()).unwrap();
                let (cancel, context) = progress::transfer_control();
                let (release, wait) = oneshot::channel();
                let verified = Rc::new(Cell::new(false));
                let checked = verified.clone();
                let job = jobs
                    .launch(jobs.reserve().unwrap(), cancel, context, async move {
                        wait.await.unwrap();
                        let (mut file, info) = source.prepare().await?;
                        // The real stream sender verifies/seeks its resume prefix first.
                        file.seek(std::io::SeekFrom::Start(0)).await?;
                        let mut bytes = Vec::new();
                        file.read_to_end(&mut bytes).await?;
                        assert_eq!(bytes, b"private queued upload");
                        assert_eq!(info.bytes, bytes.len() as u64);
                        checked.set(true);
                        Err(Error::Denied)
                    })
                    .unwrap();
                drop(upload);
                release.send(()).unwrap();
                let status = terminal(&jobs, &job.id).await;
                assert!(verified.get());
                assert_eq!(status.progress.phase, TransferPhase::Failed);
                assert!(status.receipt.is_none());
                let serialized = serde_json::to_string(&status).unwrap();
                assert!(!serialized.contains("private queued upload"));
                assert!(!serialized.contains("sha256"));
                assert!(!serialized.contains("\"source\":"));
                jobs.shutdown().await.unwrap();
            })
            .await;
    }

    #[tokio::test]
    async fn active_work_is_bounded_and_terminal_history_cannot_evict_an_active_job() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (_root, jobs) = fixture();
                let (cancel, context) = progress::transfer_control();
                let active = jobs
                    .launch(
                        jobs.reserve().unwrap(),
                        cancel,
                        context,
                        std::future::pending::<Result<files::Receipt>>(),
                    )
                    .unwrap();
                let reserved = (1..config::NODE_MAX_FILES)
                    .map(|_| jobs.reserve().unwrap())
                    .collect::<Vec<_>>();
                assert!(jobs.reserve().is_err());
                assert_eq!(jobs.list().unwrap().len(), 1);
                drop(reserved);
                let mut oldest = None;
                for _ in 0..config::ROOM_FILE_JOB_RECORDS + 2 {
                    let (cancel, context) = progress::transfer_control();
                    let done = jobs
                        .launch(jobs.reserve().unwrap(), cancel, context, async {
                            Err(Error::Denied)
                        })
                        .unwrap();
                    terminal(&jobs, &done.id).await;
                    oldest.get_or_insert(done.id);
                    assert!(jobs.list().unwrap().len() <= config::ROOM_FILE_JOB_RECORDS);
                    assert!(jobs.status(&active.id).is_ok());
                }
                assert!(jobs.status(&oldest.unwrap()).is_err());
                let reply = jobs.cancel(&active.id).await.unwrap();
                assert!(reply.acknowledged);
                assert_eq!(reply.job.progress.phase, TransferPhase::Cancelled);
                jobs.shutdown().await.unwrap();
            })
            .await;
    }

    #[tokio::test]
    async fn shutdown_reaps_unpolled_jobs_and_old_owner_ids_never_replay() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (_root, jobs) = fixture();
                let (cancel, context) = progress::transfer_control();
                let flag = Rc::new(Cell::new(false));
                let guard = Dropped {
                    flag: flag.clone(),
                    progress: cancel.subscribe(),
                };
                let job = jobs
                    .launch(jobs.reserve().unwrap(), cancel, context, async move {
                        let _guard = guard;
                        std::future::pending::<Result<files::Receipt>>().await
                    })
                    .unwrap();
                jobs.shutdown().await.unwrap();
                assert!(flag.get());
                assert_eq!(jobs.slots.available_permits(), config::NODE_MAX_FILES);
                assert!(jobs.tasks.borrow().is_empty());
                assert!(jobs.reserve().is_err());
                assert_eq!(
                    jobs.status(&job.id).unwrap().progress.phase,
                    TransferPhase::Cancelled
                );
                let (_other_root, replacement) = fixture();
                assert!(replacement.status(&job.id).is_err());
                assert!(replacement.cancel(&job.id).await.is_err());
                assert!(replacement.list().unwrap().is_empty());
                replacement.shutdown().await.unwrap();
            })
            .await;
    }

    #[tokio::test]
    async fn only_receipt_result_creates_completed_summary_without_disclosing_hash() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (_root, jobs) = fixture();
                let (cancel, context) = progress::transfer_control();
                let job = jobs
                    .launch(jobs.reserve().unwrap(), cancel, context, async {
                        Ok(files::Receipt {
                            info: files::FileInfo {
                                bytes: 8,
                                sha256: [7; 32],
                            },
                            resumed_from: 3,
                            sensitive: true,
                        })
                    })
                    .unwrap();
                let status = terminal(&jobs, &job.id).await;
                assert_eq!(status.progress.phase, TransferPhase::Completed);
                assert_eq!(status.receipt.as_ref().unwrap().bytes, 8);
                assert_eq!(status.receipt.as_ref().unwrap().resumed_from, 3);
                let serialized = serde_json::to_value(&status).unwrap();
                assert!(serialized["receipt"].get("sha256").is_none());
                let canceled = jobs.cancel(&job.id).await.unwrap();
                assert!(!canceled.requested && canceled.acknowledged);
                assert_eq!(canceled.job.progress.phase, TransferPhase::Completed);
                jobs.shutdown().await.unwrap();
            })
            .await;
    }
}
