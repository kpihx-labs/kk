//! Descriptor-relative private identity IO, shared by Unix and macOS.
use crate::{Error, Result, config};
use rustix::fs::{self, Mode, OFlags, RawMode};
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, Path},
};

pub(super) struct Directory(File);

// Root-owned system aliases (Darwin /var and /tmp) are allowed, never user
// aliases. Walk the resolved spelling using retained no-follow directory handles.
fn open_directory(path: &Path) -> Result<File> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    if absolute
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(Error::Identity(
            "identity path must not contain parent traversal",
        ));
    }
    for ancestor in absolute.ancestors() {
        let metadata = std::fs::symlink_metadata(ancestor)?;
        if metadata.file_type().is_symlink() && metadata.uid() != 0 {
            return Err(Error::Identity("identity path contains an untrusted alias"));
        }
        if !metadata.file_type().is_symlink() {
            trusted_metadata(&metadata)?;
        }
    }
    let physical = std::fs::canonicalize(&absolute)?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = File::from(fs::open("/", flags, Mode::empty())?);
    trusted_ancestor(&directory)?;
    for component in physical.components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(name) => {
                directory = File::from(fs::openat(&directory, name, flags, Mode::empty())?);
                trusted_ancestor(&directory)?;
            }
            _ => return Err(Error::Identity("invalid physical identity path")),
        }
    }
    Ok(directory)
}

fn trusted_ancestor(file: &File) -> Result<()> {
    trusted_metadata(&file.metadata()?)
}

fn trusted_metadata(metadata: &std::fs::Metadata) -> Result<()> {
    let uid = rustix::process::geteuid().as_raw();
    if !metadata.is_dir()
        || (metadata.uid() != 0 && metadata.uid() != uid)
        || (metadata.mode() & 0o022 != 0 && !(metadata.uid() == 0 && metadata.mode() & 0o1000 != 0))
    {
        return Err(Error::Identity("unsafe identity path ancestor"));
    }
    Ok(())
}

impl Directory {
    fn checked(file: File) -> Result<Self> {
        let metadata = file.metadata()?;
        if !metadata.is_dir()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o7777 != 0o700
        {
            return Err(Error::Identity(
                "identity directory must be owned and mode 0700",
            ));
        }
        Ok(Self(file))
    }

    pub(super) fn open(path: &Path) -> Result<Self> {
        // Never accept a symlink as the identity directory itself, including
        // root-owned aliases; only system aliases in its ancestors are permitted.
        let name = path
            .file_name()
            .ok_or(Error::Identity("missing identity name"))?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = open_directory(parent)?;
        Self::checked(File::from(fs::openat(
            &parent,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?))
    }

    pub(super) fn create(path: &Path) -> Result<Self> {
        let name = path
            .file_name()
            .ok_or(Error::Identity("missing identity name"))?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = open_directory(parent)?;
        fs::mkdirat(&parent, name, Mode::from_raw_mode(0o700))?;
        let file = File::from(fs::openat(
            &parent,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let directory = Self::checked(file)?;
        // Persist the new directory entry separately from the contents.
        parent.sync_all()?;
        Ok(directory)
    }

    pub(super) fn write(&self, name: &str, bytes: &[u8], mode: RawMode) -> Result<()> {
        let mut file = File::from(fs::openat(
            &self.0,
            name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(mode),
        )?);
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    }

    pub(super) fn read(&self, name: &str, private: bool) -> Result<Vec<u8>> {
        let file = File::from(fs::openat(
            &self.0,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.nlink() != 1
            || (private && metadata.mode() & 0o7777 != 0o600)
            || (!private && metadata.mode() & 0o7022 != 0)
        {
            return Err(Error::Identity(
                "identity file must be owned, unaliased and protected",
            ));
        }
        if metadata.len() > config::MAX_IDENTITY_BYTES as u64 {
            return Err(Error::Identity("identity file is too large"));
        }
        let mut bytes = Vec::new();
        file.take((config::MAX_IDENTITY_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > config::MAX_IDENTITY_BYTES {
            return Err(Error::Identity("identity file is too large"));
        }
        Ok(bytes)
    }

    pub(super) fn sync(&self) -> Result<()> {
        Ok(self.0.sync_all()?)
    }
}

#[cfg(test)]
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
    fn retained_identity_directory_is_authority_after_path_replacement() {
        let root = fixture();
        let path = root.path().join("identity");
        let original = Directory::create(&path).unwrap();
        original
            .write("certificate.der", b"original", 0o644)
            .unwrap();
        let moved = root.path().join("retained");
        std::fs::rename(&path, &moved).unwrap();
        let replacement = Directory::create(&path).unwrap();
        replacement
            .write("certificate.der", b"replacement", 0o644)
            .unwrap();
        assert_eq!(
            original.read("certificate.der", false).unwrap(),
            b"original"
        );
        assert_eq!(
            replacement.read("certificate.der", false).unwrap(),
            b"replacement"
        );
        original
            .write("private-key.der", b"synthetic", 0o600)
            .unwrap();
        original.sync().unwrap();
        assert!(!path.join("private-key.der").exists());
        assert_eq!(
            std::fs::read(moved.join("private-key.der")).unwrap(),
            b"synthetic"
        );
    }

    #[test]
    fn opened_identity_directory_rejects_symlink_and_special_mode_files() {
        let root = fixture();
        let directory = Directory::create(&root.path().join("identity")).unwrap();
        let outside = root.path().join("outside");
        std::fs::write(&outside, b"synthetic").unwrap();
        symlink(&outside, root.path().join("identity/certificate.der")).unwrap();
        assert!(directory.read("certificate.der", false).is_err());
        directory
            .write("private-key.der", b"synthetic", 0o600)
            .unwrap();
        let key = root.path().join("identity/private-key.der");
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o4600)).unwrap();
        assert!(directory.read("private-key.der", true).is_err());
    }
}
