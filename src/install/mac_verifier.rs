//! Private Darwin verifier inputs. Not Linux-style kernel seals.
//! Other UIDs cannot replace/read/write inputs; same-UID/root compromise is excluded.
use super::{Cancel, Error, Result, archive::MAX_BYTES, digest, refuse};
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Component, Path, PathBuf},
};

pub(super) struct VerifierFile {
    _temporary: tempfile::TempDir,
    directory: File,
    file: File,
    path: PathBuf,
    leaf: String,
    sha256: String,
    length: usize,
}

fn parent_directory(path: &Path, cancel: &Cancel) -> Result<File> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    if !path.is_absolute() || path.components().count() > 64 {
        return refuse("invalid verification temporary parent depth");
    }
    let uid = rustix::process::geteuid().as_raw();
    let mut directory = File::from(fs::open("/", flags, Mode::empty())?);
    for component in path.components().skip(1) {
        cancel.check()?;
        let Component::Normal(name) = component else {
            return refuse("noncanonical verification temporary parent");
        };
        directory = File::from(fs::openat(&directory, name, flags, Mode::empty())?);
        let metadata = directory.metadata()?;
        if (metadata.uid() != 0 && metadata.uid() != uid)
            || metadata.mode() & 0o022 != 0
                && !(metadata.uid() == 0 && metadata.mode() & 0o1000 != 0)
        {
            return refuse("unsafe verification temporary ancestor");
        }
    }
    Ok(directory)
}

impl VerifierFile {
    pub fn new(bytes: &[u8], suffix: &str, cancel: &Cancel) -> Result<Self> {
        cancel.check()?;
        if bytes.len() > MAX_BYTES || !matches!(suffix, ".bin" | ".json" | ".jsonl") {
            return refuse("invalid private verification input size or suffix");
        }
        // Darwin's usual /var temporary prefix is a system symlink. Resolve the
        // selected OS temporary directory once, then use no-follow opens.
        let parent_path = std::env::temp_dir().canonicalize()?;
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        // Unlike Linux procfs descriptor names, gh needs this physical pathname.
        // Validate every ancestor so another UID cannot rename an enclosing dir.
        let parent = parent_directory(&parent_path, cancel)?;
        let uid = rustix::process::geteuid().as_raw();
        let temporary = tempfile::Builder::new()
            .prefix("kk-verification-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(&parent_path)?;
        let name = temporary
            .path()
            .file_name()
            .ok_or(Error::Refused("missing verification directory name"))?;
        let directory = File::from(fs::openat(&parent, name, flags, Mode::empty())?);
        let metadata = directory.metadata()?;
        if metadata.uid() != uid || metadata.mode() & 0o7777 != 0o700 {
            return refuse("unsafe private verification directory");
        }
        let leaf = format!("input{suffix}");
        let mut file = File::from(fs::openat(
            &directory,
            &leaf,
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?);
        for chunk in bytes.chunks(65536) {
            cancel.check()?;
            file.write_all(chunk)?;
        }
        file.sync_all()?;
        fs::fchmod(&file, Mode::from_raw_mode(0o400))?;
        // Do not retain a writable descriptor even though the threat model already
        // excludes malicious code running as this UID.
        drop(file);
        let file = File::from(fs::openat(
            &directory,
            &leaf,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let path = temporary.path().join(&leaf);
        let input = Self {
            _temporary: temporary,
            directory,
            file,
            path,
            leaf,
            sha256: digest(bytes),
            length: bytes.len(),
        };
        input.recheck()?;
        Ok(input)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn recheck(&self) -> Result<()> {
        let metadata = self.file.metadata()?;
        let named = fs::statat(&self.directory, &self.leaf, AtFlags::SYMLINK_NOFOLLOW)?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.len() != self.length as u64
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o7777 != 0o400
            || FileType::from_raw_mode(named.st_mode) != FileType::RegularFile
            || i128::from(named.st_dev) != i128::from(metadata.dev())
            || named.st_ino != metadata.ino()
        {
            return refuse("private verification input identity or permissions changed");
        }
        let mut file = self.file.try_clone()?;
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.take(self.length as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() != self.length || digest(&bytes) != self.sha256 {
            return refuse("private verification input bytes changed");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writable_ancestor_cannot_redirect_a_private_verifier_path() {
        let fixture = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let shared = fixture.path().join("shared");
        let private = shared.join("private");
        std::fs::create_dir(&shared).unwrap();
        std::fs::create_dir(&private).unwrap();
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(parent_directory(&private.canonicalize().unwrap(), &Cancel::default()).is_err());
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o700)).unwrap();
        parent_directory(&private.canonicalize().unwrap(), &Cancel::default()).unwrap();
    }

    #[test]
    fn preserves_decoder_suffix_readonly_bytes_and_lifetime() {
        let input = VerifierFile::new(b"public attestation", ".jsonl", &Cancel::default()).unwrap();
        assert_eq!(input.path().extension().unwrap(), "jsonl");
        assert_eq!(std::fs::read(input.path()).unwrap(), b"public attestation");
        input.recheck().unwrap();
        let path = input.path().to_owned();
        drop(input);
        assert!(!path.exists());
    }

    #[test]
    fn recheck_rejects_changed_content_and_named_symlink() {
        let input = VerifierFile::new(b"public", ".bin", &Cancel::default()).unwrap();
        std::fs::set_permissions(input.path(), std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(input.path(), b"edited").unwrap();
        std::fs::set_permissions(input.path(), std::fs::Permissions::from_mode(0o400)).unwrap();
        assert!(input.recheck().is_err());
        std::fs::remove_file(input.path()).unwrap();
        std::os::unix::fs::symlink("missing", input.path()).unwrap();
        assert!(input.recheck().is_err());
    }
}
