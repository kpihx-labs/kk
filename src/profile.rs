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
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or(Error::Configuration("no user data directory is available"))?;
    let path = root.join("kk");
    private_directory(&path, true)?;
    Ok(path)
}
