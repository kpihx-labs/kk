//! Private local application data, never a global machine configuration.

use crate::{Error, Result};
use std::path::{Path, PathBuf};

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
        if directory.join("contacts.json").exists() || directory.join("contact-history").exists() {
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
    fn missing_identity_is_not_regenerated_over_existing_trust() {
        let root = tempfile::tempdir().unwrap();
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
