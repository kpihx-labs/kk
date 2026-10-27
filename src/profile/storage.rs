//! Bounded same-user JSON metadata. This stores preferences, never trust decisions.
use crate::{Error, Result, profile};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

const LIMIT: u64 = 128 * 1024;

#[cfg(unix)]
fn owned(file: &File) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != rustix::process::geteuid().as_raw()
        || m.mode() & 0o777 != 0o600
        || m.nlink() != 1
    {
        return Err(Error::Configuration(
            "profile metadata must be an owned private regular file",
        ));
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn read<T: Default + DeserializeOwned>(directory: &Path, name: &str) -> Result<T> {
    use rustix::{
        fs::{self, Mode, OFlags},
        io::Errno,
    };
    if !directory.exists() {
        return Ok(T::default());
    }
    profile::private_directory(directory, false)?;
    let mut file = match fs::open(
        directory.join(name),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => File::from(fd),
        Err(Errno::NOENT) => return Ok(T::default()),
        Err(error) => return Err(error.into()),
    };
    owned(&file)?;
    if file.metadata()?.len() > LIMIT {
        return Err(Error::Configuration("profile metadata is oversized"));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::Configuration(
            "profile metadata grew beyond its limit",
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| Error::Configuration("profile metadata is invalid; it was not reset"))
}

#[cfg(unix)]
pub(super) fn update<T: Default + DeserializeOwned + Serialize>(
    directory: &Path,
    name: &str,
    change: impl FnOnce(&mut T) -> Result<()>,
) -> Result<T> {
    use rustix::fs::{self, Mode, OFlags};
    profile::private_directory(directory, true)?;
    let lock = File::from(fs::open(
        directory.join(format!("{name}.lock")),
        OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )?);
    owned(&lock)?;
    fs::flock(&lock, fs::FlockOperation::LockExclusive)?;
    let mut value = read(directory, name)?;
    change(&mut value)?;
    let bytes = serde_json::to_vec(&value)
        .map_err(|_| Error::Configuration("profile metadata encoding failed"))?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::Configuration("profile metadata limit reached"));
    }
    if directory.join(name).exists() {
        // Validate the destination too; the final rename never follows a symlink.
        let _: T = read(directory, name)?;
    }
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(directory.join(name))
        .map_err(|_| Error::Configuration("profile metadata publication failed"))?;
    File::open(directory)?.sync_all()?;
    Ok(value)
}

#[cfg(not(unix))]
pub(super) fn read<T: Default + DeserializeOwned>(_: &Path, _: &str) -> Result<T> {
    Err(Error::UnsupportedPlatform)
}
#[cfg(not(unix))]
pub(super) fn update<T: Default + DeserializeOwned + Serialize>(
    _: &Path,
    _: &str,
    _: impl FnOnce(&mut T) -> Result<()>,
) -> Result<T> {
    Err(Error::UnsupportedPlatform)
}
