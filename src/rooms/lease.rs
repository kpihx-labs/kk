//! The shared filesystem boundary for distinct room databases. Only their file
//! protection is shared: public control data and private MLS data remain separate.
#[cfg(unix)]
use crate::profile;
use crate::{Error, Result};
use std::fs::File;
use std::path::{Path, PathBuf};

pub(super) struct Lease {
    #[cfg(unix)]
    file: File,
    #[cfg(target_vendor = "apple")]
    ownership: (File, PathBuf),
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
        // Darwin SQLite can select a flock-based VFS. Its database descriptor
        // must not contend with our independent actor-ownership flock.
        #[cfg(target_vendor = "apple")]
        let ownership = {
            let ownership_path = directory.join(format!(".{name}.owner"));
            let ownership_file = match fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(
                    (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
                )
                .open(&ownership_path)
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .custom_flags(
                            (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits()
                                as i32,
                        )
                        .open(&ownership_path)?
                }
                Err(error) => return Err(error.into()),
            };
            let metadata = ownership_file.metadata()?;
            if !metadata.is_file()
                || metadata.uid() != rustix::process::geteuid().as_raw()
                || metadata.mode() & 0o7777 != 0o600
                || metadata.nlink() != 1
                || metadata.len() != 0
            {
                return Err(Error::Storage(
                    "database ownership file must be owned, private, regular, single-link and empty",
                ));
            }
            (ownership_file, ownership_path)
        };
        #[cfg(target_vendor = "apple")]
        let ownership_file = &ownership.0;
        #[cfg(not(target_vendor = "apple"))]
        let ownership_file = &file;
        // Retain the original database-inode lease on Linux for compatibility
        // with existing actors; never disable SQLite's own locking policy.
        rustix::fs::flock(
            ownership_file,
            rustix::fs::FlockOperation::NonBlockingLockExclusive,
        )
        .map_err(|_| {
            Error::Storage(match name {
                "control.sqlite3" => "public control database is already owned by another actor",
                "private.sqlite3" => "private MLS database is already owned by another actor",
                _ => "database is already owned by another actor",
            })
        })?;
        Ok(Self {
            file,
            #[cfg(target_vendor = "apple")]
            ownership,
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
        #[cfg(target_vendor = "apple")]
        {
            let before = self.ownership.0.metadata()?;
            let after = fs::symlink_metadata(&self.ownership.1)?;
            if !after.is_file()
                || after.ino() != before.ino()
                || after.dev() != before.dev()
                || after.uid() != rustix::process::geteuid().as_raw()
                || after.mode() & 0o7777 != 0o600
                || after.nlink() != 1
                || after.len() != 0
            {
                return Err(Error::Storage(
                    "database ownership file changed during open",
                ));
            }
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
    #[cfg(unix)]
    fn ownership_file(&self) -> &File {
        #[cfg(target_vendor = "apple")]
        {
            &self.ownership.0
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            &self.file
        }
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
        if let Err(error) =
            rustix::fs::flock(self.ownership_file(), rustix::fs::FlockOperation::Unlock)
        {
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
    fn actor_ownership_and_sqlite_storage_policy_can_coexist() {
        let directory = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(directory.path())
            .unwrap()
            .join("private");
        let owner = Lease::open(
            &path,
            "private.sqlite3",
            true,
            crate::config::ROOM_STORE_MAX_BYTES,
        )
        .unwrap();
        let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
            | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW
            | rusqlite::OpenFlags::SQLITE_OPEN_FULL_MUTEX;
        let connection = rusqlite::Connection::open_with_flags(owner.path(), flags).unwrap();
        super::super::database::configure(
            &connection,
            crate::config::ROOM_STORE_MAX_BYTES,
            crate::config::ROOM_STORE_VALUE_BYTES,
        )
        .unwrap();
        connection
            .execute_batch(
                "CREATE TABLE preserved(value INTEGER); INSERT INTO preserved VALUES(7);",
            )
            .unwrap();
        assert!(
            Lease::open(
                &path,
                "private.sqlite3",
                false,
                crate::config::ROOM_STORE_MAX_BYTES
            )
            .is_err()
        );
        drop(connection);
        drop(owner);
        let reopened = Lease::open(
            &path,
            "private.sqlite3",
            false,
            crate::config::ROOM_STORE_MAX_BYTES,
        )
        .unwrap();
        reopened.verify_path().unwrap();
        let connection = rusqlite::Connection::open_with_flags(reopened.path(), flags).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT value FROM preserved", [], |row| row
                    .get::<_, u64>(0))
                .unwrap(),
            7
        );
    }

    #[cfg(target_vendor = "apple")]
    #[test]
    fn apple_ownership_entry_is_private_bound_and_never_repaired() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let directory = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(directory.path())
            .unwrap()
            .join("private");
        let owner = Lease::open(&path, "private.sqlite3", true, 4096).unwrap();
        let ownership = owner.ownership.1.clone();
        let metadata = std::fs::symlink_metadata(&ownership).unwrap();
        assert_eq!(metadata.mode() & 0o7777, 0o600);
        assert_eq!(metadata.nlink(), 1);
        assert_eq!(metadata.len(), 0);
        let moved = path.join("retained-owner");
        std::fs::rename(&ownership, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &ownership).unwrap();
        assert!(owner.verify_path().is_err());
        assert!(Lease::open(&path, "private.sqlite3", false, 4096).is_err());
        assert!(
            std::fs::symlink_metadata(&ownership)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        drop(owner);
        std::fs::remove_file(&ownership).unwrap();
        std::fs::rename(&moved, &ownership).unwrap();
        std::fs::set_permissions(&ownership, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Lease::open(&path, "private.sqlite3", false, 4096).is_err());
        assert_eq!(
            std::fs::metadata(&ownership).unwrap().mode() & 0o7777,
            0o644
        );
        assert_eq!(std::fs::metadata(&ownership).unwrap().len(), 0);
    }

    #[test]
    fn owner_release_is_not_extended_by_an_inherited_file_description() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("private");
        let owner = Lease::open(&path, "private.sqlite3", true, 4096).unwrap();
        let inherited = owner.ownership_file().try_clone().unwrap();
        drop(owner);
        let reopened = Lease::open(&path, "private.sqlite3", false, 4096)
            .expect("the disposed owner must release its own lease");
        drop(inherited);
        assert!(Lease::open(&path, "private.sqlite3", false, 4096).is_err());
        drop(reopened);
        assert!(Lease::open(&path, "private.sqlite3", false, 4096).is_ok());
    }
}
