//! One transfer's bounded observation and owner-acknowledged cancellation.
//! No task registry, file paths, identities, retries or budget authority live here.
#[cfg(unix)]
use super::Receipt;
#[cfg(unix)]
use crate::{Error, Result};
#[cfg(unix)]
use std::{future::Future, pin::Pin};
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TransferPhase {
    Preparing,
    Negotiating,
    Transferring,
    Verifying,
    Publishing,
    AwaitingReceipt,
    Completed,
    Cancelled,
    Uncertain,
    Failed,
}

impl TransferPhase {
    pub(crate) fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Cancelled | Self::Uncertain | Self::Failed
        )
    }
}

/// Counters are local observations, never proof of remote persistence. On send,
/// source_bytes counts complete chunks submitted to QUIC; on receive it counts
/// body bytes written to staging. Neither includes the verified resume prefix.
#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct TransferProgress {
    pub phase: TransferPhase,
    pub total_bytes: Option<u64>,
    pub resumed_from: u64,
    pub source_bytes: u64,
    pub handed_off: bool,
    pub error: Option<&'static str>,
}

#[derive(Clone)]
#[cfg(unix)]
pub(crate) struct TransferControl {
    cancel: watch::Sender<bool>,
    progress: watch::Receiver<TransferProgress>,
}

#[cfg(unix)]
impl TransferControl {
    /// Idempotent request only. Await a terminal snapshot for acknowledgement.
    pub fn cancel(&self) {
        self.cancel.send_replace(true);
    }

    pub fn subscribe(&self) -> watch::Receiver<TransferProgress> {
        self.progress.clone()
    }
}

/// Only the owning future decides the terminal outcome; stream hooks can publish
/// nonterminal observations but cannot manufacture a verified receipt.
#[cfg(unix)]
pub(crate) struct TransferContext {
    cancel: watch::Receiver<bool>,
    observer: TransferObserver,
}

#[derive(Clone)]
pub(crate) struct TransferObserver(watch::Sender<TransferProgress>);

#[cfg(unix)]
pub(crate) enum TransferOutcome {
    Completed(Receipt),
    Cancelled,
    Uncertain(Error),
    Failed(Error),
}

#[cfg(unix)]
impl TransferOutcome {
    pub fn into_result(self) -> Result<Receipt> {
        match self {
            Self::Completed(receipt) => Ok(receipt),
            Self::Cancelled => Err(Error::Cancelled),
            Self::Uncertain(error) | Self::Failed(error) => Err(error),
        }
    }
}

#[cfg(unix)]
pub(crate) fn transfer_control() -> (TransferControl, TransferContext) {
    let (progress, receiver) = watch::channel(TransferProgress {
        phase: TransferPhase::Preparing,
        total_bytes: None,
        resumed_from: 0,
        source_bytes: 0,
        handed_off: false,
        error: None,
    });
    let (cancel, requested) = watch::channel(false);
    (
        TransferControl {
            cancel,
            progress: receiver,
        },
        TransferContext {
            cancel: requested,
            observer: TransferObserver(progress),
        },
    )
}

impl TransferObserver {
    fn update(&self, change: impl FnOnce(&mut TransferProgress)) {
        self.0.send_if_modified(|progress| {
            if progress.phase.terminal() {
                return false;
            }
            change(progress);
            true
        });
    }

    /// Call before the first network effect, including the enclosing room RPC.
    /// Once set, failure/cancellation without a receipt is conservatively uncertain.
    pub fn handoff(&self) {
        self.update(|progress| {
            progress.handed_off = true;
            progress.phase = TransferPhase::Negotiating;
        });
    }

    pub(crate) fn offered(&self, bytes: u64) {
        self.update(|progress| progress.total_bytes = Some(bytes));
    }

    pub(super) fn resumed(&self, offset: u64) {
        self.update(|progress| {
            progress.resumed_from = offset.min(progress.total_bytes.unwrap_or(0));
            progress.source_bytes = progress.source_bytes.min(
                progress
                    .total_bytes
                    .unwrap_or(0)
                    .saturating_sub(progress.resumed_from),
            );
            progress.phase = TransferPhase::Transferring;
        });
    }

    pub(super) fn advance(&self, count: u64) {
        self.update(|progress| {
            let remaining = progress
                .total_bytes
                .unwrap_or(0)
                .saturating_sub(progress.resumed_from);
            progress.source_bytes = progress.source_bytes.saturating_add(count).min(remaining);
        });
    }

    pub(super) fn phase(&self, phase: TransferPhase) {
        debug_assert!(!phase.terminal());
        self.update(|progress| progress.phase = phase);
    }

    #[cfg(unix)]
    fn finish(&self, error: Option<&Error>) {
        self.update(|progress| {
            progress.error = error.map(Error::code);
            progress.phase = match error {
                None => TransferPhase::Completed,
                Some(_) if progress.handed_off => TransferPhase::Uncertain,
                Some(Error::Cancelled) => TransferPhase::Cancelled,
                Some(_) => TransferPhase::Failed,
            };
        });
    }
}

// Fields drop in declaration order: the owned transfer (including stream/store
// cancellation guards) is dropped before a terminal cancellation is observable.
#[cfg(unix)]
struct Running<F> {
    future: Pin<Box<F>>,
    guard: RunGuard,
}

#[cfg(unix)]
struct RunGuard {
    observer: TransferObserver,
    armed: bool,
}

#[cfg(unix)]
impl Drop for RunGuard {
    fn drop(&mut self) {
        if self.armed {
            self.observer.finish(Some(&Error::Cancelled));
        }
    }
}

#[cfg(unix)]
impl TransferContext {
    pub fn observer(&self) -> TransferObserver {
        self.observer.clone()
    }

    /// Consume one context around the complete owned operation, not just its HTTP
    /// waiter. Dropping this wrapper also acknowledges cancellation only after
    /// dropping the inner future. Queued blocking publication remains governed by
    /// the existing storage gate; a possible prior publication is not denied.
    pub async fn run(mut self, future: impl Future<Output = Result<Receipt>>) -> TransferOutcome {
        let mut running = Running {
            future: Box::pin(future),
            guard: RunGuard {
                observer: self.observer.clone(),
                armed: true,
            },
        };
        let already_cancelled = *self.cancel.borrow();
        let result = if already_cancelled {
            Err(Error::Cancelled)
        } else {
            tokio::select! {
                // An already available verified receipt wins a simultaneous cancel.
                biased;
                result = &mut running.future => result,
                () = cancelled(&mut self.cancel) => Err(Error::Cancelled),
            }
        };
        running.guard.armed = false;
        drop(running);
        self.observer.finish(result.as_ref().err());
        let handed_off = self.observer.0.borrow().handed_off;
        match result {
            Ok(receipt) => TransferOutcome::Completed(receipt),
            Err(error) if handed_off => TransferOutcome::Uncertain(error),
            Err(Error::Cancelled) => TransferOutcome::Cancelled,
            Err(error) => TransferOutcome::Failed(error),
        }
    }
}

#[cfg(unix)]
async fn cancelled(requested: &mut watch::Receiver<bool>) {
    loop {
        if *requested.borrow_and_update() {
            return;
        }
        if requested.changed().await.is_err() {
            // Losing an observer does not silently cancel an owned transfer.
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::files::FileInfo;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    fn receipt() -> Receipt {
        Receipt {
            info: FileInfo {
                bytes: 8,
                sha256: [7; 32],
            },
            resumed_from: 3,
            sensitive: false,
        }
    }

    struct Dropped {
        flag: Arc<AtomicBool>,
        progress: watch::Receiver<TransferProgress>,
    }
    impl Drop for Dropped {
        fn drop(&mut self) {
            assert!(
                !self.progress.borrow().phase.terminal(),
                "ack preceded owned future drop"
            );
            self.flag.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn precancel_drops_unpolled_work_before_acknowledging_cancellation() {
        let (control, context) = transfer_control();
        let progress = control.subscribe();
        let flag = Arc::new(AtomicBool::new(false));
        let dropped = Dropped {
            flag: flag.clone(),
            progress: progress.clone(),
        };
        control.cancel();
        control.cancel();
        let outcome = context
            .run(async move {
                let _dropped = dropped;
                panic!("pre-cancelled work must never be polled");
            })
            .await;
        assert!(matches!(outcome, TransferOutcome::Cancelled));
        assert!(flag.load(Ordering::SeqCst));
        assert_eq!(progress.borrow().phase, TransferPhase::Cancelled);
    }

    #[tokio::test]
    async fn active_cancel_acknowledges_future_drop_but_not_remote_nonpublication() {
        let (control, context) = transfer_control();
        let progress = control.subscribe();
        let observer = context.observer();
        let flag = Arc::new(AtomicBool::new(false));
        let dropped = Dropped {
            flag: flag.clone(),
            progress: progress.clone(),
        };
        let (ready, reached) = tokio::sync::oneshot::channel();
        let transfer = context.run(async move {
            let _dropped = dropped;
            observer.offered(8);
            observer.handoff();
            observer.resumed(3);
            observer.advance(2);
            ready.send(()).unwrap();
            std::future::pending::<Result<Receipt>>().await
        });
        let cancel = async {
            reached.await.unwrap();
            assert_eq!(progress.borrow().phase, TransferPhase::Transferring);
            assert_eq!(progress.borrow().source_bytes, 2);
            control.cancel();
            assert!(
                !progress.borrow().phase.terminal(),
                "request is not acknowledgement"
            );
        };
        let (outcome, ()) = tokio::join!(transfer, cancel);
        assert!(matches!(
            outcome,
            TransferOutcome::Uncertain(Error::Cancelled)
        ));
        assert!(flag.load(Ordering::SeqCst));
        assert_eq!(progress.borrow().phase, TransferPhase::Uncertain);
        assert_eq!(progress.borrow().resumed_from, 3);
        assert_eq!(progress.borrow().source_bytes, 2);
    }

    #[tokio::test]
    async fn dropping_the_owner_future_publishes_terminal_only_after_inner_drop() {
        let (control, context) = transfer_control();
        let progress = control.subscribe();
        let observer = context.observer();
        let flag = Arc::new(AtomicBool::new(false));
        let dropped = Dropped {
            flag: flag.clone(),
            progress: progress.clone(),
        };
        let mut running = Box::pin(context.run(async move {
            let _dropped = dropped;
            observer.handoff();
            std::future::pending::<Result<Receipt>>().await
        }));
        assert!(futures_util::poll!(running.as_mut()).is_pending());
        drop(running);
        assert!(flag.load(Ordering::SeqCst));
        assert_eq!(progress.borrow().phase, TransferPhase::Uncertain);
    }

    #[tokio::test]
    async fn ready_verified_result_wins_simultaneous_cancel_and_late_hooks_are_ignored() {
        let (control, context) = transfer_control();
        let progress = control.subscribe();
        let observer = context.observer();
        let (ready, result) = tokio::sync::oneshot::channel();
        let transfer = context.run(async { result.await.unwrap() });
        let complete = async {
            ready.send(Ok(receipt())).unwrap();
            control.cancel();
        };
        let (outcome, ()) = tokio::join!(transfer, complete);
        assert!(matches!(outcome, TransferOutcome::Completed(_)));
        assert_eq!(progress.borrow().phase, TransferPhase::Completed);
        observer.handoff();
        observer.offered(999);
        observer.advance(u64::MAX);
        assert_eq!(progress.borrow().phase, TransferPhase::Completed);
        assert_eq!(progress.borrow().total_bytes, None);
    }

    #[tokio::test]
    async fn failure_classification_and_counter_bounds_do_not_claim_delivery() {
        for handed_off in [false, true] {
            let (control, context) = transfer_control();
            let observer = context.observer();
            observer.offered(8);
            if handed_off {
                observer.handoff();
            }
            observer.resumed(3);
            observer.advance(u64::MAX);
            observer.advance(1);
            assert_eq!(control.subscribe().borrow().source_bytes, 5);
            let outcome = context.run(async { Err(Error::Network) }).await;
            assert!(matches!(outcome, TransferOutcome::Uncertain(Error::Network)) == handed_off);
            assert_eq!(control.subscribe().borrow().error, Some("NETWORK"));
            assert_eq!(
                control.subscribe().borrow().phase,
                if handed_off {
                    TransferPhase::Uncertain
                } else {
                    TransferPhase::Failed
                }
            );
        }
    }

    #[tokio::test]
    async fn losing_control_does_not_cancel_an_owned_transfer() {
        let (control, context) = transfer_control();
        drop(control);
        assert!(
            context
                .run(async { Ok(receipt()) })
                .await
                .into_result()
                .is_ok()
        );
    }
}
