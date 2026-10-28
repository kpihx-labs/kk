//! Verified prebuilt installation. Never executes the candidate or opens profiles.
//! Linux-only until filesystem and artifact gates are qualified on other hosts.
use std::path::PathBuf;

#[cfg(target_os = "linux")]
#[path = "install/archive.rs"]
mod archive;
#[cfg(target_os = "linux")]
#[path = "install/elf.rs"]
mod elf;
#[cfg(target_os = "linux")]
#[path = "install/managed.rs"]
mod managed;
#[cfg(target_os = "linux")]
#[path = "install/provenance.rs"]
mod provenance;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("installer I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("installer refused: {0}")]
    Refused(&'static str),
}
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(target_os = "linux")]
#[derive(Clone, Default)]
pub(super) struct Cancel(std::sync::Arc<std::sync::atomic::AtomicBool>);

#[cfg(target_os = "linux")]
impl Cancel {
    pub fn check(&self) -> Result<()> {
        if self.0.load(std::sync::atomic::Ordering::Acquire) {
            return refuse("installation cancelled; incomplete staging retained");
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
struct CancelOnDrop(Cancel);

#[cfg(target_os = "linux")]
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.0.store(true, std::sync::atomic::Ordering::Release);
    }
}

#[cfg(target_os = "linux")]
async fn worker<T: Send + 'static>(job: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|_| Error::Refused("installer worker failed"))?
}

#[cfg(target_os = "linux")]
impl From<rustix::io::Errno> for Error {
    fn from(value: rustix::io::Errno) -> Self {
        Self::Io(value.into())
    }
}

/// Explicit selectors, never a moving latest tag. Bundle is a gh attestation bundle.
#[derive(Clone, Debug)]
pub struct InstallRequest {
    pub version: String,
    pub commit: String,
    pub prefix: PathBuf,
    pub archive: Option<PathBuf>,
    pub bundle: Option<PathBuf>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Outcome {
    pub version: String,
    pub previous: Option<String>,
    pub executable: PathBuf,
}

pub(crate) fn refuse<T>(reason: &'static str) -> Result<T> {
    Err(Error::Refused(reason))
}

pub(crate) fn version(value: &str) -> Result<()> {
    if value.len() > 64 || !value.is_ascii() {
        return refuse("invalid release version");
    }
    let (base, suffix) = value
        .split_once('-')
        .map_or((value, None), |(a, b)| (a, Some(b)));
    let components: Vec<_> = base.split('.').collect();
    if components.len() != 3
        || components.iter().any(|s| {
            s.is_empty()
                || !s.bytes().all(|b| b.is_ascii_digit())
                || (s.len() > 1 && s.starts_with('0'))
        })
    {
        return refuse("invalid release version");
    }
    if suffix.is_some_and(|s| {
        s.is_empty()
            || s.split('.').any(|p| {
                p.is_empty()
                    || !p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    || (p.len() > 1 && p.bytes().all(|b| b.is_ascii_digit()) && p.starts_with('0'))
            })
    }) {
        return refuse("invalid prerelease version");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(crate) fn sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(target_os = "linux")]
pub(crate) fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Authenticate a frozen private copy with gh before any archive parsing or writes.
pub async fn install(request: InstallRequest) -> Result<Outcome> {
    #[cfg(target_os = "linux")]
    {
        version(&request.version)?;
        if request.commit.len() != 40
            || !request
                .commit
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return refuse("commit must be a full lowercase SHA");
        }
        if !request.prefix.is_absolute() || request.bundle.is_some() && request.archive.is_none() {
            return refuse("absolute prefix required; bundle requires local archive");
        }
        let target = host_target()?;
        let cancel = Cancel::default();
        let _lifetime = CancelOnDrop(cancel.clone());
        let bytes = provenance::authenticate(&request, target, cancel.clone()).await?;
        let parsing_cancel = cancel.clone();
        let version = request.version.clone();
        let commit = request.commit.clone();
        let (verified, archive_digest, original) = worker(move || {
            parsing_cancel.check()?;
            let verified = archive::verify(&bytes, &version, &commit, target, &parsing_cancel)?;
            parsing_cancel.check()?;
            let archive_digest = digest(&bytes);
            Ok((verified, archive_digest, bytes))
        })
        .await?;
        provenance::check_abi(&verified, cancel.clone()).await?;
        worker(move || {
            managed::install(
                &request.prefix,
                &verified,
                &archive_digest,
                &original,
                cancel,
            )
        })
        .await
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = request;
        refuse(
            "native installation is qualified only for Linux; Windows/macOS releases are unavailable",
        )
    }
}

/// Roll back to retained, fully rechecked managed bytes; never downloads anything.
pub async fn rollback(prefix: PathBuf, selected: String) -> Result<Outcome> {
    rollback_with_bundle(prefix, selected, None).await
}

/// Reauthenticate the retained original archive, optionally with an exported gh bundle.
pub async fn rollback_with_bundle(
    prefix: PathBuf,
    selected: String,
    bundle: Option<PathBuf>,
) -> Result<Outcome> {
    version(&selected)?;
    #[cfg(target_os = "linux")]
    {
        let cancel = Cancel::default();
        let _lifetime = CancelOnDrop(cancel.clone());
        let target = host_target()?;
        let reading_prefix = prefix.clone();
        let reading_version = selected.clone();
        let reading_cancel = cancel.clone();
        let (original, commit) = worker(move || {
            managed::rollback_input(&reading_prefix, &reading_version, reading_cancel)
        })
        .await?;
        let request = InstallRequest {
            version: selected.clone(),
            commit: commit.clone(),
            prefix: prefix.clone(),
            archive: None,
            bundle,
        };
        let original = provenance::authenticate_bytes(&request, original, cancel.clone()).await?;
        let parsing_cancel = cancel.clone();
        let (fresh, archive_digest) = worker(move || {
            let fresh = archive::verify(&original, &selected, &commit, target, &parsing_cancel)?;
            Ok((fresh, digest(&original)))
        })
        .await?;
        provenance::check_abi(&fresh, cancel.clone()).await?;
        worker(move || managed::rollback(&prefix, &fresh, &archive_digest, cancel)).await
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (prefix, bundle);
        refuse("native rollback is qualified only for Linux")
    }
}

#[cfg(target_os = "linux")]
pub fn host_target() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x86_64-unknown-linux-gnu"),
        "aarch64" => Ok("aarch64-unknown-linux-gnu"),
        _ => refuse("no qualified artifact for this CPU"),
    }
}
