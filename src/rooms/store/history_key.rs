//! Explicit, anchored filesystem prerequisite for local history. No enrollment policy.
use crate::{Error, Result};
use age::x25519::{Identity, Recipient};
use std::path::Path;

/// No Debug, serialization, private export or implicit generation.
pub(crate) struct HistoryKey {
    identity: Identity,
}
impl HistoryKey {
    pub(crate) fn recipient(&self) -> Recipient {
        self.identity.to_public()
    }
    pub(crate) fn with_identity<T>(&self, use_key: impl FnOnce(&Identity) -> T) -> T {
        use_key(&self.identity)
    }
}

const NAME: &str = "history.agekey";
const LIMIT: usize = 512;
// Counts every directory entry, including dot entries and unrelated room files.
// Exhaustion is ambiguous state, never permission to generate another identity.
#[cfg(any(target_os = "linux", target_vendor = "apple"))]
const INITIALIZE_SCAN_ENTRIES: usize = 64;
#[cfg(any(target_os = "linux", target_vendor = "apple"))]
fn recovery_required() -> Error {
    Error::Destination("history key initialization is ambiguous; explicit recovery required")
}
fn invalid() -> Error {
    Error::Destination("history key or directory is missing, unsafe or mismatched")
}

#[cfg(unix)]
use rustix::fs::{self, Mode, OFlags};
#[cfg(unix)]
use std::{
    ffi::OsString,
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::Component,
};

/// Retained root-to-room descriptors. Callers must coordinate writers for this
/// private path scope; this is not isolation from malicious same-UID processes.
pub(crate) struct HistoryKeyAnchor {
    #[cfg(unix)]
    chain: Vec<(OsString, File)>,
    owner: [u8; 32],
    room: [u8; 32],
}

#[cfg(unix)]
fn flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}
#[cfg(unix)]
fn same(a: &File, b: &File) -> Result<()> {
    let (a, b) = (a.metadata()?, b.metadata()?);
    if (a.dev(), a.ino()) != (b.dev(), b.ino()) {
        return Err(invalid());
    }
    Ok(())
}
#[cfg(unix)]
fn private(file: &File, directory: bool) -> Result<()> {
    let m = file.metadata()?;
    if m.uid() != rustix::process::geteuid().as_raw()
        || m.mode() & 0o7777 != if directory { 0o700 } else { 0o600 }
        || if directory {
            !m.is_dir()
        } else {
            !m.is_file() || m.nlink() != 1
        }
    {
        return Err(invalid());
    }
    Ok(())
}

impl HistoryKeyAnchor {
    /// Directory must already exist, be absolute, owned, 0700, and symlink-free.
    pub(crate) fn open(path: &Path, owner: [u8; 32], room: [u8; 32]) -> Result<Self> {
        #[cfg(unix)]
        {
            if !path.is_absolute() {
                return Err(invalid());
            }
            let root = File::from(fs::open("/", flags(), Mode::empty())?);
            let mut chain = vec![(OsString::from("/"), root)];
            for component in path.components() {
                let name = match component {
                    Component::RootDir => continue,
                    Component::Normal(name) => name,
                    _ => return Err(invalid()),
                };
                let next = File::from(fs::openat(
                    &chain.last().ok_or_else(invalid)?.1,
                    name,
                    flags(),
                    Mode::empty(),
                )?);
                chain.push((name.to_owned(), next));
            }
            let anchor = Self { chain, owner, room };
            anchor.verify()?;
            Ok(anchor)
        }
        #[cfg(not(unix))]
        {
            let _ = (path, owner, room);
            Err(Error::UnsupportedPlatform)
        }
    }

    /// Revalidate every named ancestor and private root, not just the last inode.
    pub(crate) fn verify(&self) -> Result<()> {
        #[cfg(unix)]
        {
            let root = File::from(fs::open("/", flags(), Mode::empty())?);
            same(&root, &self.chain[0].1)?;
            for pair in self.chain.windows(2) {
                let reopened =
                    File::from(fs::openat(&pair[0].1, &pair[1].0, flags(), Mode::empty())?);
                same(&reopened, &pair[1].1)?;
            }
            private(self.directory(), true)
        }
        #[cfg(not(unix))]
        {
            Err(Error::UnsupportedPlatform)
        }
    }

    #[cfg(unix)]
    fn directory(&self) -> &File {
        &self.chain[self.chain.len() - 1].1
    }

    /// SQL owner may clone the validated room descriptor for its own anchor API.
    #[cfg(unix)]
    pub(crate) fn directory_file(&self) -> Result<File> {
        self.verify()?;
        Ok(self.directory().try_clone()?)
    }

    /// Existing files are never replaced, including malformed or valid keys.
    /// Any failure after publication leaves the key intact; never retry by deleting.
    pub(crate) fn initialize(&self) -> Result<HistoryKey> {
        #[cfg(any(target_os = "linux", target_vendor = "apple"))]
        {
            self.initialize_with_sync(|file| file.sync_all())
        }
        #[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
        {
            Err(Error::UnsupportedPlatform)
        }
    }

    // The Provider's exclusive lease serializes Group writers. Raw helper callers
    // also need an inode-scoped gate spanning scan, generation and publication.
    // openat(".") creates an INDEPENDENT open file description: a try_clone would
    // share flock ownership and let concurrent calls on this same anchor race.
    // Nonblocking acquisition bounds waiting; closing releases only this lease.
    #[cfg(any(target_os = "linux", target_vendor = "apple"))]
    fn initialization_lease(&self) -> Result<File> {
        self.verify()?;
        let lease = File::from(fs::openat(self.directory(), ".", flags(), Mode::empty())?);
        same(self.directory(), &lease)?;
        private(&lease, true)?;
        fs::flock(&lease, fs::FlockOperation::NonBlockingLockExclusive).map_err(|_| {
            Error::Destination(
                "history key initialization is busy; retry after the active operation",
            )
        })?;
        self.verify()?;
        Ok(lease)
    }

    #[cfg(any(target_os = "linux", target_vendor = "apple"))]
    fn refuse_pending(&self) -> Result<()> {
        let mut entries = fs::Dir::read_from(self.directory())?;
        for _ in 0..INITIALIZE_SCAN_ENTRIES {
            let Some(entry) = entries.next() else {
                return Ok(());
            };
            let entry = entry?;
            let name = entry.file_name().to_bytes();
            if !name.starts_with(b".history-key-") && !name.ends_with(b".pending") {
                continue;
            }
            // Never open or read an artifact, even a symlink/FIFO. Validate the
            // canonical name and no-follow metadata, but preserve unsafe and
            // incomplete artifacts too: none authorize regeneration or adoption.
            name.strip_prefix(b".history-key-")
                .and_then(|name| name.strip_suffix(b".pending"))
                .filter(|nonce| {
                    nonce.len() == 32
                        && nonce
                            .iter()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
                })
                .ok_or_else(recovery_required)?;
            let stat = fs::statat(
                self.directory(),
                entry.file_name(),
                fs::AtFlags::SYMLINK_NOFOLLOW,
            )
            .map_err(|_| recovery_required())?;
            if fs::FileType::from_raw_mode(stat.st_mode) != fs::FileType::RegularFile
                || stat.st_uid != rustix::process::geteuid().as_raw()
                || stat.st_nlink != 1
                || stat.st_mode & 0o7777 != 0o600
                || stat.st_size < 0
                || stat.st_size as u64 > LIMIT as u64
            {
                return Err(recovery_required());
            }
            return Err(recovery_required());
        }
        Err(recovery_required())
    }

    #[cfg(any(target_os = "linux", target_vendor = "apple"))]
    fn initialize_with_sync(
        &self,
        sync: impl FnMut(&File) -> std::io::Result<()>,
    ) -> Result<HistoryKey> {
        self.initialize_with_io(|file, bytes| file.write_all(bytes), sync)
    }

    #[cfg(any(target_os = "linux", target_vendor = "apple"))]
    fn initialize_with_io(
        &self,
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
        mut sync: impl FnMut(&File) -> std::io::Result<()>,
    ) -> Result<HistoryKey> {
        use age::secrecy::ExposeSecret;
        use ring::rand::{SecureRandom, SystemRandom};
        use zeroize::Zeroizing;
        let _lease = self.initialization_lease()?;
        // Check existence without reading it, including dangling symlinks.
        match fs::statat(self.directory(), NAME, fs::AtFlags::SYMLINK_NOFOLLOW) {
            Err(rustix::io::Errno::NOENT) => (),
            _ => return Err(invalid()),
        }
        self.refuse_pending()?;
        self.verify()?;
        let identity = Identity::generate();
        let secret = identity.to_string(); // AGE SecretString owns its zeroization.
        let mut bytes = Zeroizing::new(Vec::with_capacity(LIMIT));
        writeln!(bytes, "{}{}", self.header(), secret.expose_secret())?;
        if bytes.len() > LIMIT {
            return Err(invalid());
        }
        let mut nonce = [0u8; 16];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| invalid())?;
        let temporary = format!(".history-key-{}.pending", hex(&nonce));
        let mut file = File::from(fs::openat(
            self.directory(),
            temporary.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )?);
        private(&file, false)?;
        write(&mut file, &bytes)?;
        sync(&file)?;
        self.verify()?;
        private(&file, false)?;
        let named = File::from(fs::openat(
            self.directory(),
            temporary.as_str(),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        same(&file, &named)?;
        fs::renameat_with(
            self.directory(),
            temporary.as_str(),
            self.directory(),
            NAME,
            fs::RenameFlags::NOREPLACE,
        )?;
        sync(self.directory())?;
        self.verify()?;
        // A post-publication failure never removes either published or pending data.
        let loaded = self.load()?;
        if loaded.recipient() != identity.to_public() {
            return Err(invalid());
        }
        Ok(loaded)
    }

    /// Pure load: no creation, enrollment, adoption or repair on any error.
    pub(crate) fn load(&self) -> Result<HistoryKey> {
        #[cfg(unix)]
        {
            use zeroize::Zeroizing;
            self.verify()?;
            let mut file = File::from(fs::openat(
                self.directory(),
                NAME,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            private(&file, false)?;
            if file.metadata()?.len() > LIMIT as u64 {
                return Err(invalid());
            }
            let mut bytes = Zeroizing::new([0u8; LIMIT + 1]);
            let mut length = 0;
            while length < bytes.len() {
                let n = file.read(&mut bytes[length..])?;
                if n == 0 {
                    break;
                }
                length += n;
            }
            if length > LIMIT {
                return Err(invalid());
            }
            let text = std::str::from_utf8(&bytes[..length]).map_err(|_| invalid())?;
            let header = self.header();
            let secret = text
                .strip_prefix(&header)
                .and_then(|s| s.strip_suffix('\n'))
                .ok_or_else(invalid)?;
            if secret.contains(['\n', '\r']) {
                return Err(invalid());
            }
            let identity: Identity = secret.parse().map_err(|_| invalid())?;
            // Require one canonical standard AGE identity, not a permissive alternate spelling.
            use age::secrecy::ExposeSecret;
            if identity.to_string().expose_secret() != secret {
                return Err(invalid());
            }
            private(&file, false)?;
            self.verify()?;
            let named = File::from(fs::openat(
                self.directory(),
                NAME,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            same(&file, &named)?;
            Ok(HistoryKey { identity })
        }
        #[cfg(not(unix))]
        {
            Err(Error::UnsupportedPlatform)
        }
    }

    fn header(&self) -> String {
        format!(
            "# K&K history key v1\n# profile: {}\n# room: {}\n",
            hex(&self.owner),
            hex(&self.room)
        )
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(text, "{byte:02x}").expect("String write");
    }
    text
}

#[cfg(all(test, any(target_os = "linux", target_vendor = "apple")))]
#[path = "history_key_tests.rs"]
mod tests;
