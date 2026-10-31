//! Bounded same-user JSON metadata. This stores preferences, never trust decisions.
#[cfg(unix)]
use crate::profile;
use crate::{Error, Result};
use serde::{Serialize, de::DeserializeOwned};
use std::path::Path;
#[cfg(unix)]
use std::{
    fs::File,
    io::{Read, Write},
};

#[cfg(unix)]
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
fn same_file(a: &File, b: &File) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let (a, b) = (a.metadata()?, b.metadata()?);
    Ok(a.dev() == b.dev() && a.ino() == b.ino())
}

#[cfg(unix)]
fn trusted_ancestor(metadata: &std::fs::Metadata) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let uid = rustix::process::geteuid().as_raw();
    if !metadata.is_dir()
        || (metadata.uid() != 0 && metadata.uid() != uid)
        || (metadata.mode() & 0o022 != 0 && !(metadata.uid() == 0 && metadata.mode() & 0o1000 != 0))
    {
        return Err(Error::Destination("unsafe profile metadata ancestor"));
    }
    Ok(())
}

#[cfg(unix)]
fn directory(path: &Path) -> Result<File> {
    use rustix::fs::{self, Mode, OFlags};
    use std::{os::unix::fs::MetadataExt, path::Component};
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    if absolute
        .components()
        .any(|component| matches!(component, Component::ParentDir))
        || !std::fs::symlink_metadata(&absolute)?.is_dir()
    {
        return Err(Error::Destination(
            "profile metadata directory must be a non-symlink",
        ));
    }
    // Darwin's OS temporary roots commonly traverse root-owned /var or /tmp
    // aliases. Trust only root-owned ancestor aliases, never a user alias or a
    // final-component link. Inspect both spellings; follow neither during the
    // descriptor walk that establishes the actual publication authority.
    for ancestor in absolute.ancestors().skip(1) {
        let metadata = std::fs::symlink_metadata(ancestor)?;
        if metadata.file_type().is_symlink() {
            if metadata.uid() != 0 {
                return Err(Error::Destination(
                    "untrusted profile metadata ancestor alias",
                ));
            }
        } else {
            trusted_ancestor(&metadata)?;
        }
    }
    let absolute = std::fs::canonicalize(absolute)?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut current = File::from(fs::open("/", flags, Mode::empty())?);
    trusted_ancestor(&current.metadata()?)?;
    for component in absolute.components() {
        let name = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(name) => name,
            Component::ParentDir => std::ffi::OsStr::new(".."),
            Component::Prefix(_) => return Err(Error::Denied),
        };
        current = File::from(fs::openat(&current, name, flags, Mode::empty())?);
        trusted_ancestor(&current.metadata()?)?;
    }
    let metadata = current.metadata()?;
    if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o777 != 0o700 {
        return Err(Error::Destination(
            "profile publication directory must be owned and mode 0700",
        ));
    }
    Ok(current)
}

#[cfg(unix)]
fn open_at(parent: &File, name: &str) -> Result<File> {
    use rustix::fs::{self, Mode, OFlags};
    let file = File::from(fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    owned(&file)?;
    Ok(file)
}

#[cfg(unix)]
fn decode<T: DeserializeOwned>(file: &mut File) -> Result<T> {
    if file.metadata()?.len() > LIMIT {
        return Err(Error::Configuration("profile metadata is oversized"));
    }
    let mut bytes = Vec::new();
    Read::by_ref(file).take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::Configuration(
            "profile metadata grew beyond its limit",
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| Error::Configuration("profile metadata is invalid; it was not reset"))
}

#[cfg(unix)]
fn read_at<T: Default + DeserializeOwned>(parent: &File, name: &str) -> Result<T> {
    // Missing metadata means default settings, not a new file or lock.
    match open_at(parent, name) {
        Ok(mut file) => decode(&mut file),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
struct Lease<'a>(&'a File);
#[cfg(unix)]
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let _ = rustix::fs::flock(self.0, rustix::fs::FlockOperation::Unlock);
    }
}
#[cfg(unix)]
fn shared(lock: &File) -> Result<Lease<'_>> {
    rustix::fs::flock(lock, rustix::fs::FlockOperation::NonBlockingLockShared).map_err(
        |error| {
            if error == rustix::io::Errno::WOULDBLOCK {
                Error::Denied
            } else {
                error.into()
            }
        },
    )?;
    Ok(Lease(lock))
}

/// A retained inode snapshot, not a held lease. A lease exists only inside one
/// synchronous publication callback and is never carried across an async wait.
pub(super) struct Publication {
    #[cfg(unix)]
    path: std::path::PathBuf,
    #[cfg(unix)]
    parent: File,
    #[cfg(unix)]
    lock: File,
    #[cfg(unix)]
    snapshot: File,
    #[cfg(unix)]
    stamp: std::fs::Metadata,
    #[cfg(unix)]
    serial: std::sync::Mutex<()>,
}
impl Publication {
    #[cfg(unix)]
    pub(super) fn try_publish<T>(&self, publish: impl FnOnce() -> T) -> Result<T> {
        use std::os::unix::fs::MetadataExt;
        let _serial = self.serial.try_lock().map_err(|_| Error::Denied)?;
        let _lease = shared(&self.lock)?;
        let current_parent = directory(&self.path)?;
        if !same_file(&self.parent, &current_parent)? {
            return Err(Error::Denied);
        }
        let lock = open_at(&current_parent, "settings.json.lock")?;
        let snapshot = open_at(&current_parent, "settings.json")?;
        if !same_file(&self.lock, &lock)? || !same_file(&self.snapshot, &snapshot)? {
            return Err(Error::Denied);
        }
        owned(&self.snapshot)?;
        let now = snapshot.metadata()?;
        if now.len() != self.stamp.len()
            || now.mtime() != self.stamp.mtime()
            || now.mtime_nsec() != self.stamp.mtime_nsec()
            || now.ctime() != self.stamp.ctime()
            || now.ctime_nsec() != self.stamp.ctime_nsec()
        {
            return Err(Error::Denied);
        }
        Ok(publish())
    }
    #[cfg(not(unix))]
    pub(super) fn try_publish<T>(&self, _: impl FnOnce() -> T) -> Result<T> {
        Err(Error::UnsupportedPlatform)
    }
}

#[cfg(unix)]
pub(super) fn publication<T: DeserializeOwned>(path: &Path) -> Result<(Publication, T)> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let parent = directory(&path)?;
    let lock = open_at(&parent, "settings.json.lock")?;
    let lease = shared(&lock)?;
    let mut snapshot = open_at(&parent, "settings.json")?;
    let stamp = snapshot.metadata()?;
    let value = decode(&mut snapshot)?;
    drop(lease);
    let publication = Publication {
        path,
        parent,
        lock,
        snapshot,
        stamp,
        serial: std::sync::Mutex::new(()),
    };
    // Reject a directory/lock/path replacement during preparation as well.
    publication.try_publish(|| ())?;
    Ok((publication, value))
}
#[cfg(not(unix))]
pub(super) fn publication<T: DeserializeOwned>(_: &Path) -> Result<(Publication, T)> {
    Err(Error::UnsupportedPlatform)
}

#[cfg(unix)]
pub(super) fn read<T: Default + DeserializeOwned>(directory: &Path, name: &str) -> Result<T> {
    let parent = match self::directory(directory) {
        Ok(parent) => parent,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(T::default());
        }
        Err(error) => return Err(error),
    };
    read_at(&parent, name)
}

#[cfg(unix)]
pub(super) fn update<T: Default + DeserializeOwned + Serialize>(
    directory: &Path,
    name: &str,
    change: impl FnOnce(&mut T) -> Result<()>,
) -> Result<T> {
    use rustix::fs::{self, Mode, OFlags};
    profile::private_directory(directory, true)?;
    let parent = self::directory(directory)?;
    let lock = File::from(fs::openat(
        &parent,
        format!("{name}.lock"),
        OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )?);
    owned(&lock)?;
    fs::flock(&lock, fs::FlockOperation::LockExclusive)?;
    // Explicit unlock is necessary: a concurrent fork may briefly inherit this
    // open-file description despite CLOEXEC. Closing only our fd would let that
    // child extend a completed writer's exclusive lease until exec.
    let _lease = Lease(&lock);
    let authority = || -> Result<()> {
        if !same_file(&parent, &self::directory(directory)?)?
            || !same_file(&lock, &open_at(&parent, &format!("{name}.lock"))?)?
        {
            return Err(Error::Denied);
        }
        Ok(())
    };
    authority()?;
    let mut value = read_at(&parent, name)?;
    change(&mut value)?;
    let bytes = serde_json::to_vec(&value)
        .map_err(|_| Error::Configuration("profile metadata encoding failed"))?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::Configuration("profile metadata limit reached"));
    }
    let _: T = read_at(&parent, name)?;
    let temporary = tempfile::Builder::new().make_in(directory, |path| {
        fs::openat(
            &parent,
            path.file_name().expect("temporary filename"),
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map(File::from)
        .map_err(std::io::Error::from)
    })?;
    let (mut file, mut temporary_path) = temporary.into_parts();
    // Cleanup must also be relative to our retained directory, never a replaced
    // pathname. The writer and every publication now use one lock authority.
    temporary_path.disable_cleanup(true);
    let temporary_name = temporary_path.file_name().ok_or(Error::Protocol)?;
    let result = (|| -> Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        authority()?;
        fs::renameat(&parent, temporary_name, &parent, name)?;
        parent.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::unlinkat(&parent, temporary_name, fs::AtFlags::empty());
    }
    result?;
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn fixture() -> tempfile::TempDir {
        tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap()
    }

    #[test]
    fn metadata_read_write_and_publication_share_os_temp_path_authority() {
        let root = fixture();
        let profile = root.path().join("profile");
        let expected = vec!["synthetic".to_owned()];
        update::<Vec<String>>(&profile, "settings.json", |value| {
            value.extend(expected.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            read::<Vec<String>>(&profile, "settings.json").unwrap(),
            expected
        );
        let (authority, value) = publication::<Vec<String>>(&profile).unwrap();
        assert_eq!(value, expected);
        authority.try_publish(|| ()).unwrap();
    }

    #[test]
    fn metadata_reads_refuse_user_ancestor_aliases_like_writes_and_publication() {
        let root = fixture();
        let profile = root.path().join("profile");
        update::<Vec<String>>(&profile, "settings.json", |value| {
            value.push("retained".to_owned());
            Ok(())
        })
        .unwrap();
        let alias = root.path().join("alias");
        symlink(root.path(), &alias).unwrap();
        if rustix::process::geteuid().as_raw() != 0 {
            let alias_profile = alias.join("profile");
            assert!(read::<Vec<String>>(&alias_profile, "settings.json").is_err());
            assert!(update::<Vec<String>>(&alias_profile, "settings.json", |_| Ok(())).is_err());
            assert!(publication::<Vec<String>>(&alias_profile).is_err());
        }
        assert_eq!(
            read::<Vec<String>>(&profile, "settings.json").unwrap(),
            ["retained"]
        );
        assert!(!root.path().join("absent").exists());
        assert!(
            read::<Vec<String>>(&root.path().join("absent"), "settings.json")
                .unwrap()
                .is_empty()
        );
        assert!(!root.path().join("absent").exists());
    }

    #[test]
    fn completed_writer_unlock_is_not_extended_by_an_inherited_open_file_description() {
        use rustix::fs::{self, FlockOperation, Mode, OFlags};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json.lock");
        let writer = File::from(
            fs::open(
                &path,
                OFlags::CREATE | OFlags::RDWR | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .unwrap(),
        );
        fs::flock(&writer, FlockOperation::LockExclusive).unwrap();
        // dup shares the same open-file description, exactly the lock-lifetime
        // property relevant to a concurrent fork before its CLOEXEC processing.
        let inherited = writer.try_clone().unwrap();
        let contender = File::open(&path).unwrap();
        assert_eq!(
            fs::flock(&contender, FlockOperation::NonBlockingLockShared),
            Err(rustix::io::Errno::WOULDBLOCK)
        );
        let lease = Lease(&writer);
        drop(lease);
        drop(writer);
        fs::flock(&contender, FlockOperation::NonBlockingLockShared).unwrap();
        drop(inherited);
    }
}
