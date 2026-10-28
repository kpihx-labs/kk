//! The shared filesystem boundary for distinct room databases. Only their file
//! protection is shared: public control data and private MLS data remain separate.
#[cfg(unix)]
use crate::profile;
use crate::{Error, Result};
#[cfg(unix)]
use std::fs::File;
use std::path::{Path, PathBuf};

pub(super) struct Lease {
    #[cfg(unix)]
    file: File,
    path: PathBuf,
    #[cfg(test)]
    release_probe: Option<Box<dyn FnOnce() + Send>>,
}
impl Lease {
    #[cfg(unix)]
    pub(super) fn open(
        directory: &Path,
        name: &'static str,
        create: bool,
        maximum: usize,
    ) -> Result<Self> {
        use std::{
            fs,
            os::unix::fs::{MetadataExt, OpenOptionsExt},
        };
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.len() > 64
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(Error::Configuration(
                "database name must be a single local filename",
            ));
        }
        profile::private_directory(directory, create)?;
        let path = directory.join(name);
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(create)
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(&path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o777 != 0o600
            || metadata.nlink() != 1
            || metadata.len() > maximum as u64
        {
            return Err(Error::Storage(
                "database must be owned, private, regular, single-link and bounded",
            ));
        }
        rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive).map_err(
            |_| {
                Error::Storage(match name {
                    "control.sqlite3" => {
                        "public control database is already owned by another actor"
                    }
                    "private.sqlite3" => "private MLS database is already owned by another actor",
                    _ => "database is already owned by another actor",
                })
            },
        )?;
        Ok(Self {
            file,
            path,
            #[cfg(test)]
            release_probe: None,
        })
    }
    #[cfg(not(unix))]
    pub(super) fn open(
        _directory: &Path,
        _name: &'static str,
        _create: bool,
        _maximum: usize,
    ) -> Result<Self> {
        Err(Error::UnsupportedPlatform)
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    #[cfg(unix)]
    pub(super) fn verify_path(&self) -> Result<()> {
        use std::{fs, os::unix::fs::MetadataExt};
        let before = self.file.metadata()?;
        let after = fs::symlink_metadata(&self.path)?;
        if !after.is_file() || after.ino() != before.ino() || after.dev() != before.dev() {
            return Err(Error::Storage("database changed during open"));
        }
        Ok(())
    }
    #[cfg(not(unix))]
    pub(super) fn verify_path(&self) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }
    #[cfg(unix)]
    pub(super) fn file(&self) -> &File {
        &self.file
    }
    #[cfg(all(test, unix))]
    pub(super) fn observe_release(&mut self, probe: impl FnOnce() + Send + 'static) {
        self.release_probe = Some(Box::new(probe));
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        #[cfg(test)]
        if let Some(probe) = self.release_probe.take() {
            probe();
        }
        // CLOEXEC does not release an inherited description until the child
        // executes. The logical owner's lifetime, not that alias, owns the lock.
        #[cfg(unix)]
        if let Err(error) = rustix::fs::flock(&self.file, rustix::fs::FlockOperation::Unlock) {
            eprintln!("database lease unlock failed: {error}");
        }
    }
}

pub(super) fn sync_creation(directory: &Path) -> Result<()> {
    File::open(directory)?.sync_all()?;
    let parent = directory
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn owner_release_is_not_extended_by_an_inherited_file_description() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("private");
        let owner = Lease::open(&path, "private.sqlite3", true, 4096).unwrap();
        let inherited = owner.file.try_clone().unwrap();
        drop(owner);
        let reopened = Lease::open(&path, "private.sqlite3", false, 4096)
            .expect("the disposed owner must release its own lease");
        drop(inherited);
        assert!(Lease::open(&path, "private.sqlite3", false, 4096).is_err());
        drop(reopened);
        assert!(Lease::open(&path, "private.sqlite3", false, 4096).is_ok());
    }
}
