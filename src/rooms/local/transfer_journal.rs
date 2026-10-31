//! Bounded metadata checkpoints only: never source recipes or runnable jobs.
use super::{JobStatus, State};
use crate::{Error, Result, config};
use ring::rand::{SecureRandom, SystemRandom};
use rustix::{
    fd::OwnedFd,
    fs::{self, Mode, OFlags},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::Path,
};

const NAME: &str = "file-transfers.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    version: u8,
    next: u64,
    jobs: Vec<(u64, JobStatus)>,
}

pub(super) struct Journal(OwnedFd);

impl Journal {
    pub fn open(directory: &Path) -> Result<Self> {
        crate::profile::private_directory(directory, false)?;
        let fd = fs::open(
            directory,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let metadata = fs::fstat(&fd)?;
        if metadata.st_uid != rustix::process::geteuid().as_raw()
            || metadata.st_mode & 0o777 != 0o700
        {
            return Err(Error::Destination("unsafe file transfer journal directory"));
        }
        Ok(Self(fd))
    }

    fn existing(&self) -> Result<Option<File>> {
        let file = match fs::openat(
            &self.0,
            NAME,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        ) {
            Ok(fd) => File::from(fd),
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o777 != 0o600
            || metadata.nlink() != 1
        {
            return Err(Error::Destination("unsafe file transfer journal"));
        }
        Ok(Some(file))
    }

    pub fn load(&self) -> Result<(u64, BTreeMap<u64, JobStatus>)> {
        let Some(file) = self.existing()? else {
            return Ok((0, BTreeMap::new()));
        };
        let limit = config::MAX_RECORD_PLAINTEXT_BYTES;
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(Error::Limit {
                resource: "file transfer journal",
                max: limit,
            });
        }
        let checkpoint: Checkpoint = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Configuration("invalid file transfer journal"))?;
        if checkpoint.version != 1 || checkpoint.jobs.len() > config::ROOM_FILE_JOB_RECORDS {
            return Err(Error::Configuration(
                "unsupported or oversized file transfer journal",
            ));
        }
        let mut jobs = BTreeMap::new();
        for (key, mut job) in checkpoint.jobs {
            job.validate(key)?;
            if key >= checkpoint.next || jobs.contains_key(&key) {
                return Err(Error::Configuration(
                    "invalid file transfer journal sequence",
                ));
            }
            if !job.progress.phase.terminal() {
                // A stale Preparing snapshot cannot prove that no network effect
                // occurred before the crash. Recovery never guesses Cancelled.
                job.progress.phase = crate::files::progress::TransferPhase::Uncertain;
                job.progress.error = Some("OWNER_RESTARTED".to_owned());
                job.receipt = None;
                job.recovered = true;
            }
            job.historical = true;
            job.terminal_durable = true;
            job.retry_allowed = job.retryable();
            jobs.insert(key, job);
        }
        Ok((checkpoint.next, jobs))
    }

    /// Single foreground/local executor serializes these bounded checkpoints.
    /// No await occurs between the terminal observation, fsync and visibility.
    pub fn save(&self, state: &State, owner: &str) -> Result<()> {
        drop(self.existing()?);
        let mut jobs = state
            .history
            .iter()
            .map(|(&key, job)| (key, job.clone()))
            .collect::<Vec<_>>();
        jobs.extend(
            state
                .records
                .iter()
                .map(|(&key, record)| (key, record.raw_status(owner, key))),
        );
        jobs.sort_by_key(|(key, _)| *key);
        for (_, job) in &mut jobs {
            job.terminal_durable = job.progress.phase.terminal();
            job.retry_allowed = job.retryable();
        }
        let bytes = serde_json::to_vec(&Checkpoint {
            version: 1,
            next: state.next,
            jobs,
        })
        .map_err(|_| Error::Protocol)?;
        let limit = config::MAX_RECORD_PLAINTEXT_BYTES;
        if bytes.len() > limit {
            return Err(Error::Limit {
                resource: "file transfer journal",
                max: limit,
            });
        }
        let mut nonce = [0; 16];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| Error::Node("file journal randomness unavailable"))?;
        let suffix = nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let temporary = format!(".file-transfers-{suffix}.tmp");
        let file = fs::openat(
            &self.0,
            temporary.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )?;
        let result = (|| -> Result<()> {
            let mut file = File::from(file);
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::renameat(&self.0, temporary.as_str(), &self.0, NAME)?;
            fs::fsync(&self.0)?;
            Ok(())
        })();
        if result.is_err() {
            match fs::unlinkat(&self.0, temporary.as_str(), fs::AtFlags::empty()) {
                Ok(()) | Err(rustix::io::Errno::NOENT) => (),
                Err(error) => return Err(error.into()),
            }
        }
        result
    }
}
