//! Private local application data, never a global machine configuration.

use crate::{Error, Result};
use std::path::{Path, PathBuf};
pub mod catalog;
pub mod labels;
mod settings;
mod storage;
#[cfg(windows)]
pub mod windows;
pub use settings::{Appearance, Settings, SettingsPublication, SidebarMode, validate_name};

#[cfg(unix)]
pub fn private_directory(path: &Path, create: bool) -> Result<()> {
    use std::{
        fs,
        os::unix::fs::{DirBuilderExt, MetadataExt},
    };
    if create && !path.exists() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o700
    {
        return Err(Error::Destination(
            "local directory must be owned, non-symlink and mode 0700",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn private_directory(_path: &Path, _create: bool) -> Result<()> {
    Err(Error::UnsupportedPlatform)
}

#[cfg(unix)]
fn reserve_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let parent = path
        .parent()
        .ok_or(Error::Destination("missing profile parent"))?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    // Unlike ensure/create-all, this final creation is exclusive. A concurrent
    // initializer must not turn profile creation into adoption of its identity.
    std::fs::DirBuilder::new().mode(0o700).create(path)?;
    private_directory(path, false)
}

#[cfg(not(unix))]
fn reserve_directory(_path: &Path) -> Result<()> {
    Err(Error::UnsupportedPlatform)
}

pub fn parent(destination: &Path) -> Result<&Path> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if destination.file_name().is_none() {
        return Err(Error::Destination("missing output filename"));
    }
    private_directory(parent, false)?;
    Ok(parent)
}

pub fn data_directory() -> Result<PathBuf> {
    let path = base_directory()?;
    private_directory(&path, true)?;
    Ok(path)
}

pub fn base_directory() -> Result<PathBuf> {
    // Resolve the OS account home too: service/guest-agent environments may
    // intentionally omit HOME. No machine-specific fallback path is invented.
    let root = dirs::data_local_dir()
        .ok_or(Error::Configuration("no user data directory is available"))?;
    Ok(root.join("kk"))
}

pub fn identity(directory: &Path, create: bool) -> Result<crate::Identity> {
    private_directory(directory, create)?;
    let path = directory.join("identity");
    if create && !path.exists() {
        if directory.join("contacts.json").exists()
            || directory.join("contact-history").exists()
            || directory.join("rooms").exists()
        {
            return Err(Error::Identity(
                "profile identity is missing but trust data exists; no regeneration",
            ));
        }
        crate::Identity::create(&path)
    } else if !path.exists() {
        Err(Error::Identity(
            "profile is not initialized; run kk init or pair",
        ))
    } else {
        crate::Identity::load(&path)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn new_profile_reservation_has_one_winner_and_never_adopts_existing_state() {
        let root = crate::test_support::tempdir().unwrap();
        let directory = root.path().join("profiles/new");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let directory = directory.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    reserve_directory(&directory).is_ok()
                })
            })
            .collect();
        assert_eq!(
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .filter(|success| *success)
                .count(),
            1
        );
        let marker = directory.join("retained-state");
        std::fs::write(&marker, b"retained profile state").unwrap();
        assert!(reserve_directory(&directory).is_err());
        assert_eq!(std::fs::read(marker).unwrap(), b"retained profile state");
    }
    #[test]
    fn missing_identity_is_not_regenerated_over_existing_trust() {
        let root = crate::test_support::tempdir().unwrap();
        let directory = root.path().join("profile");
        identity(&directory, true).unwrap();
        std::fs::write(
            directory.join("contacts.json"),
            b"synthetic existing trust marker",
        )
        .unwrap();
        std::fs::rename(
            directory.join("identity"),
            root.path().join("retained-identity"),
        )
        .unwrap();
        assert!(matches!(
            identity(&directory, true),
            Err(Error::Identity(_))
        ));
        assert!(!directory.join("identity").exists());
        assert!(
            root.path()
                .join("retained-identity/private-key.der")
                .exists()
        );
    }
}
