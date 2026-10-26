//! One owner for explicit inbox consent, bounded offers and resume reservations.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct Inbox {
    directory: PathBuf,
    max_bytes: u64,
    budget: AtomicU64,
    attempts: AtomicU64,
}
impl Inbox {
    pub fn new(directory: PathBuf, max_bytes: u64, budget: u64) -> Result<Self> {
        if max_bytes > config::MAX_FILE_BYTES || budget > config::MAX_FILE_BYTES {
            return Err(Error::FileTooLarge);
        }
        crate::profile::private_directory(&directory, true)?;
        Ok(Self {
            directory,
            max_bytes,
            budget: AtomicU64::new(budget),
            attempts: AtomicU64::new(config::NODE_FILE_COUNT_BUDGET),
        })
    }
    pub fn destination(&self, peer: &Peer, info: &FileInfo) -> PathBuf {
        self.directory
            .join(format!("{}-{}.bin", peer.fingerprint(), info.hash_hex()))
    }
    pub fn choose(&self, peer: &Peer, info: &FileInfo) -> Result<ReceivePolicy> {
        if info.bytes > self.max_bytes {
            return Err(Error::FileTooLarge);
        }
        self.attempts
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .map_err(|_| Error::Node("file count budget exhausted"))?;
        self.budget
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(info.bytes)
            })
            .map_err(|_| Error::Node("file byte budget exhausted"))?;
        ReceivePolicy::accept(self.destination(peer, info), self.max_bytes)
    }
    pub fn complete(&self, receipt: &Receipt) {
        // Failed offers retain reservations; verified resumes refund only prefix.
        self.budget
            .fetch_add(receipt.resumed_from, Ordering::SeqCst);
    }
    pub fn remaining(&self) -> (u64, u64) {
        (
            self.budget.load(Ordering::Relaxed),
            self.attempts.load(Ordering::Relaxed),
        )
    }
}
