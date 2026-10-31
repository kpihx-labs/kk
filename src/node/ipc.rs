//! Same-user local commands only. No HTTP, shell execution or network trust bootstrap.

use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::os::unix::fs::FileTypeExt;
#[cfg(any(target_os = "linux", test))]
use std::os::unix::fs::PermissionsExt;
use std::{
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};
use zeroize::{Zeroize, Zeroizing};

use super::{Handle, Status};
use crate::{Error, Result, config, files};

const MAX_FRAME: usize = config::MAX_MESSAGE_BYTES * 6 + 8192;
const MAX_BINDING_STAGES: usize = 64;

#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
pub enum Request {
    Text {
        target: String,
        text: String,
        sensitive: bool,
        seconds: u64,
    },
    File {
        target: String,
        source: PathBuf,
        sensitive: bool,
        seconds: u64,
    },
    Status,
}
impl Drop for Request {
    fn drop(&mut self) {
        if let Self::Text { text, .. } = self {
            text.zeroize();
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub ok: bool,
    pub code: String,
    pub bytes: u64,
    pub resumed_from: u64,
    pub digest: Option<[u8; 32]>,
    pub sensitive: bool,
    pub status: Option<Status>,
}
impl Reply {
    fn success(bytes: u64, sensitive: bool) -> Self {
        Self {
            ok: true,
            code: String::new(),
            bytes,
            resumed_from: 0,
            digest: None,
            sensitive,
            status: None,
        }
    }
    pub fn result(&self) -> Result<()> {
        if self.ok {
            return Ok(());
        }
        Err(match self.code.as_str() {
            "DENIED" => Error::Denied,
            "REVOKED" => Error::Revoked,
            "TIMEOUT" => Error::Timeout,
            "AUTHENTICATION" => Error::Authentication,
            "INTEGRITY" => Error::Integrity,
            "FILE_TOO_LARGE" => Error::FileTooLarge,
            "MESSAGE_TOO_LARGE" => Error::MessageTooLarge,
            "DESTINATION" => Error::Destination("session file operation refused"),
            _ => Error::Node("local session command failed; inspect its event stream"),
        })
    }
    pub fn receipt(&self) -> files::Receipt {
        files::Receipt {
            info: files::FileInfo {
                bytes: self.bytes,
                sha256: self.digest.unwrap_or([0; 32]),
            },
            resumed_from: self.resumed_from,
            sensitive: self.sensitive,
        }
    }
}

struct NodeLock(std::fs::File);
impl std::ops::Deref for NodeLock {
    type Target = std::fs::File;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for NodeLock {
    fn drop(&mut self) {
        // A forked child can retain the open description even with CLOEXEC.
        // Logical ownership ends here, including failed listener initialization.
        if let Err(error) = rustix::fs::flock(&self.0, rustix::fs::FlockOperation::Unlock) {
            eprintln!("node ownership unlock failed: {error}");
        }
    }
}

pub(crate) struct Listener {
    listener: UnixListener,
    #[cfg(test)]
    path: PathBuf,
    inode: u64,
    _lock: NodeLock,
    directory: BoundDirectory,
    _profile: BoundDirectory,
}

// Other UIDs must not control any ancestor: only root/current-user ownership,
// no group/other writes except sticky directories whose owner is trusted too.
// Held descriptors and lifecycle checks detect persistent replacement; they do
// NOT isolate an arbitrary same-UID adversary or promise to detect ancestor ABA.
// Socket bind/connect remain pathname APIs on portable Unix. Their safety rests
// on this ancestor policy, not on a check/use pair being magically atomic.
struct BoundDirectory {
    path: PathBuf,
    file: std::fs::File,
    parent: Option<Box<BoundDirectory>>,
    ancestors: Vec<(PathBuf, std::fs::File)>,
}

fn trusted_ancestor(metadata: &std::fs::Metadata) -> Result<()> {
    let uid = rustix::process::geteuid().as_raw();
    if !metadata.is_dir()
        || (metadata.uid() != 0 && metadata.uid() != uid)
        || (metadata.mode() & 0o022 != 0 && metadata.mode() & 0o1000 == 0)
    {
        return Err(Error::Node("unsafe local session ancestor"));
    }
    Ok(())
}

impl BoundDirectory {
    fn open(path: &Path) -> Result<Self> {
        use rustix::fs::{Mode, OFlags, open};
        crate::profile::private_directory(path, false)?;
        // Inspect the spelling before canonicalization too. Root-owned system
        // aliases (/tmp and /var on Darwin) are allowed, user-controlled aliases
        // are not. Subsequent operations use only the canonical spelling.
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            std::env::current_dir()?.join(path)
        };
        for ancestor in absolute.ancestors().skip(1) {
            let metadata = std::fs::symlink_metadata(ancestor)?;
            if metadata.file_type().is_symlink() {
                if metadata.uid() != 0 {
                    return Err(Error::Node("untrusted local session ancestor alias"));
                }
            } else {
                trusted_ancestor(&metadata)?;
            }
        }
        let path = std::fs::canonicalize(path)?;
        let mut ancestors = Vec::new();
        for ancestor in path
            .ancestors()
            .skip(1)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            let file = std::fs::File::from(open(
                ancestor,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            trusted_ancestor(&file.metadata()?)?;
            ancestors.push((ancestor.to_owned(), file));
        }
        let file = std::fs::File::from(open(
            &path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let directory = Self {
            path,
            file,
            parent: None,
            ancestors,
        };
        directory.check()?;
        Ok(directory)
    }
    fn check(&self) -> Result<()> {
        if let Some(parent) = &self.parent {
            parent.check()?;
        }
        for (path, file) in &self.ancestors {
            let named = std::fs::symlink_metadata(path)?;
            trusted_ancestor(&named)?;
            let held = file.metadata()?;
            if (named.dev(), named.ino()) != (held.dev(), held.ino()) {
                return Err(Error::Node("local session ancestor changed"));
            }
        }
        crate::profile::private_directory(&self.path, false)?;
        let named = std::fs::symlink_metadata(&self.path)?;
        let held = self.file.metadata()?;
        if (named.dev(), named.ino()) != (held.dev(), held.ino()) || named.mode() & 0o7777 != 0o700
        {
            return Err(Error::Node("local session directory changed"));
        }
        Ok(())
    }

    fn stat(&self, name: &str) -> Result<Option<rustix::fs::Stat>> {
        match rustix::fs::statat(&self.file, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => Ok(Some(stat)),
            Err(rustix::io::Errno::NOENT) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn unlink(&self, name: &str) -> Result<()> {
        rustix::fs::unlinkat(&self.file, name, rustix::fs::AtFlags::empty())?;
        Ok(())
    }

    fn access_path(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            PathBuf::from(format!("/proc/self/fd/{}", self.file.as_raw_fd()))
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.path.clone()
        }
    }
}

// Rustix 1.1's Linux chmodat explicitly rejects SYMLINK_NOFOLLOW. fchmod on
// UnixListener changes the sockfs inode on Linux, not the filesystem endpoint.
// Pin the entry with O_PATH|NOFOLLOW and chmod its /proc FD magic link instead;
// this cannot follow a replaced named symlink. No process-global umask changes.
// Darwin uses native fchmodat(NOFOLLOW); unsupported kernels fail closed.
fn chmod_entry(directory: &std::fs::File, name: &str, mode: u32, socket: bool) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use rustix::fs::{Mode, OFlags, openat};
        use std::os::fd::AsRawFd;
        let file = std::fs::File::from(openat(
            directory,
            name,
            OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let metadata = file.metadata()?;
        if metadata.uid() != rustix::process::geteuid().as_raw()
            || if socket {
                !metadata.file_type().is_socket()
            } else {
                !metadata.is_dir()
            }
        {
            return Err(Error::Node("unsafe local session chmod entry"));
        }
        std::fs::set_permissions(
            format!("/proc/self/fd/{}", file.as_raw_fd()),
            std::fs::Permissions::from_mode(mode),
        )?;
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = socket;
        rustix::fs::chmodat(
            directory,
            name,
            rustix::fs::Mode::from_raw_mode(mode as _),
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        )?;
    }
    Ok(())
}

fn publish_binding(directory: &std::fs::File, staged: &std::ffi::OsStr) -> Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        rustix::fs::renameat_with(
            directory,
            staged,
            directory,
            "profile",
            rustix::fs::RenameFlags::NOREPLACE,
        )?;
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (directory, staged);
        Err(Error::UnsupportedPlatform)
    }
}

fn endpoint(profile: &BoundDirectory, create: bool) -> Result<Option<BoundDirectory>> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    profile.check()?;
    // 104 is the smallest supported Unix sun_path capacity (Darwin), including
    // the terminating NUL. Existing short addresses stay interoperable.
    if profile.path.join("node.sock").as_os_str().as_bytes().len() < 104 {
        return Ok(Some(BoundDirectory::open(&profile.path)?));
    }
    // Never silently bypass an older/foreign endpoint. A long legacy socket
    // cannot be addressed portably, so require explicit operator inspection.
    if profile.stat("node.sock")?.is_some() {
        return Err(Error::Node(
            "legacy socket in long profile requires inspection",
        ));
    }
    let canonical = &profile.path;
    let metadata = profile.file.metadata()?;
    let binding = serde_json::to_vec(&(
        canonical.as_os_str().as_bytes(),
        metadata.dev(),
        metadata.ino(),
    ))
    .map_err(|_| Error::Protocol)?;
    let digest = Sha256::digest(&binding);
    let name = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    // /tmp is the standard short Unix temporary namespace, not HOME, cwd or
    // TMPDIR (Darwin's per-user TMPDIR can itself exceed sun_path). Neither its
    // public name nor the digest grants authority: exact private modes, owner,
    // held directory inodes and the complete profile binding are required.
    let temporary = std::fs::canonicalize("/tmp")?;
    for ancestor in temporary.ancestors() {
        trusted_ancestor(&std::fs::symlink_metadata(ancestor)?)?;
    }
    let uid = rustix::process::geteuid().as_raw();
    // Keep the binding directories across listener lifetimes: only the owned
    // socket is runtime-cleaned. Removing/recreating a shared directory would
    // let another lifetime adopt its name. OS temporary-directory cleanup may
    // remove this namespace after reboot; the profile remains the sole state
    // authority and the next locked owner recreates its binding from that state.
    let root = temporary.join(format!("kk-ipc-{uid}"));
    let ensure = |parent: &std::fs::File, name: &str| -> Result<()> {
        match rustix::fs::mkdirat(parent, name, rustix::fs::Mode::RWXU) {
            Ok(()) => chmod_entry(parent, name, 0o700, false),
            Err(rustix::io::Errno::EXIST) => {
                let stat = rustix::fs::statat(parent, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW)?;
                // Recover only owner-private permissions narrowed by umask or
                // interruption before chmod. Never normalize broader modes,
                // foreign directories, symlinks, or special bits into trust.
                if stat.st_uid == uid
                    && rustix::fs::FileType::from_raw_mode(stat.st_mode)
                        == rustix::fs::FileType::Directory
                    && stat.st_mode & 0o7777 != 0o700
                    && stat.st_mode & 0o7077 == 0
                {
                    chmod_entry(parent, name, 0o700, false)?;
                }
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    };
    let temporary_fd = std::fs::File::from(rustix::fs::open(
        &temporary,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?);
    if create {
        ensure(&temporary_fd, &format!("kk-ipc-{uid}"))?;
    }
    if !create && missing(&root)? {
        return Ok(None);
    }
    let root = BoundDirectory::open(&root)?;
    let path = root.path.join(&name[..32]);
    if create {
        ensure(&root.file, &name[..32])?;
    }
    if !create && missing(&path)? {
        return Ok(None);
    }
    let mut directory = BoundDirectory::open(&path)?;
    root.check()?;
    use rustix::fs::{Mode, OFlags, openat};
    if create && directory.stat("profile")?.is_none() {
        // Publication is atomic. An interrupted writer leaves at most a private
        // staging file, never a partially written authoritative binding. A
        // subsequent locked owner can publish without removing any old data.
        if directory.stat("node.sock")?.is_some() {
            return Err(Error::Node("unbound local session socket"));
        }
        for (index, entry) in std::fs::read_dir(directory.access_path())?
            .take(MAX_BINDING_STAGES + 1)
            .enumerate()
        {
            if index == MAX_BINDING_STAGES {
                return Err(Error::Node(
                    "too many retained IPC stages; inspection required",
                ));
            }
            if !entry?.file_name().as_bytes().starts_with(b".profile-init-") {
                return Err(Error::Node("ambiguous unbound local session directory"));
            }
        }
        let mut staged = tempfile::Builder::new()
            .prefix(".profile-init-")
            .tempfile_in(directory.access_path())?;
        rustix::fs::fchmod(staged.as_file(), Mode::RUSR | Mode::WUSR)?;
        staged.write_all(&binding)?;
        staged.as_file().sync_all()?;
        let (_staged_file, staged_path) = staged.keep().map_err(|error| error.error)?;
        if directory.stat("profile")?.is_some() {
            return Err(Error::Node(
                "local session binding appeared during initialization",
            ));
        }
        // Cooperating writers are excluded by node.lock and other UIDs by the
        // private directory. Use rename, not link/unlink publication (a crash
        // between those two operations would leave a forbidden nlink=2).
        publish_binding(
            &directory.file,
            staged_path.file_name().ok_or(Error::Protocol)?,
        )?;
        directory.file.sync_all()?;
    }
    let mut file = std::fs::File::from(openat(
        &directory.file,
        "profile",
        (if create { OFlags::RDWR } else { OFlags::RDONLY })
            | OFlags::NOFOLLOW
            | OFlags::CLOEXEC
            | OFlags::NONBLOCK,
        Mode::empty(),
    )?);
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != uid
        || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
        || metadata.len() > binding.len() as u64
    {
        return Err(Error::Node("unsafe local session profile binding"));
    }
    let mut actual = Vec::new();
    (&mut file)
        .take(binding.len() as u64 + 1)
        .read_to_end(&mut actual)?;
    if create
        && actual.len() < binding.len()
        && binding.starts_with(&actual)
        && directory.stat("node.sock")?.is_none()
    {
        // Compatibility recovery for an interrupted pre-atomic initializer:
        // only append a verified missing suffix, under the profile owner lock.
        // Never truncate, delete, or repair mismatching/unsafe user data.
        file.write_all(&binding[actual.len()..])?;
        file.sync_all()?;
        actual = binding.clone();
    }
    if actual != binding {
        return Err(Error::Node("local session profile binding mismatch"));
    }
    let held = file.metadata()?;
    let named = directory
        .stat("profile")?
        .ok_or(Error::Node("local session binding disappeared"))?;
    if held.nlink() != 1 || (held.dev(), held.ino()) != stat_identity(&named) {
        return Err(Error::Node("local session binding identity changed"));
    }
    profile.check()?;
    directory.check()?;
    directory.parent = Some(Box::new(root));
    Ok(Some(directory))
}

fn missing(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error.into()),
    }
}
impl Listener {
    pub async fn bind(profile: &Path) -> Result<Self> {
        use rustix::fs::{self, Mode, OFlags};
        let profile = BoundDirectory::open(profile)?;
        let flags = OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
        let lock = match fs::openat(
            &profile.file,
            "node.lock",
            flags | OFlags::CREATE | OFlags::EXCL,
            Mode::RUSR | Mode::WUSR,
        ) {
            Ok(fd) => {
                fs::fchmod(&fd, Mode::RUSR | Mode::WUSR)?;
                std::fs::File::from(fd)
            }
            Err(rustix::io::Errno::EXIST) => std::fs::File::from(fs::openat(
                &profile.file,
                "node.lock",
                flags,
                Mode::empty(),
            )?),
            Err(error) => return Err(error.into()),
        };
        let metadata = lock.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o7777 != 0o600
            || metadata.nlink() != 1
        {
            return Err(Error::Node("unsafe node lock"));
        }
        fs::flock(&lock, fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| Error::Node("profile already has an active node"))?;
        let lock = NodeLock(lock);
        profile.check()?;
        check_lock(&profile, &lock)?;
        let directory = endpoint(&profile, true)?.ok_or(Error::Node("missing socket directory"))?;
        #[cfg(test)]
        let path = directory.path.join("node.sock");
        if let Some(metadata) = directory.stat("node.sock")? {
            safe_socket(&metadata)?;
            match UnixStream::connect(directory.access_path().join("node.sock")).await {
                Ok(_) => return Err(Error::Node("active node socket without lock")),
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {}
                Err(error) => return Err(error.into()),
            }
            directory.check()?;
            let current = directory
                .stat("node.sock")?
                .ok_or(Error::Node("stale socket disappeared"))?;
            safe_socket(&current)?;
            if (current.st_dev, current.st_ino) != (metadata.st_dev, metadata.st_ino) {
                return Err(Error::Node("stale local session socket changed"));
            }
            directory.unlink("node.sock")?; // Owned stale runtime endpoint, not user data.
        }
        let listener = UnixListener::bind(directory.access_path().join("node.sock"))?;
        chmod_entry(&directory.file, "node.sock", 0o600, true)?;
        let metadata = directory
            .stat("node.sock")?
            .ok_or(Error::Node("new socket disappeared"))?;
        safe_socket(&metadata)?;
        let inode = metadata.st_ino;
        directory.check()?;
        check_lock(&profile, &lock)?;
        Ok(Self {
            listener,
            #[cfg(test)]
            path,
            inode,
            _lock: lock,
            directory,
            _profile: profile,
        })
    }
    pub async fn accept(&self) -> Result<UnixStream> {
        self._profile.check()?;
        check_lock(&self._profile, &self._lock)?;
        self.directory.check()?;
        let (stream, _) = self.listener.accept().await?;
        check_user(&stream)?;
        self._profile.check()?;
        check_lock(&self._profile, &self._lock)?;
        self.directory.check()?;
        Ok(stream)
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        if self._profile.check().is_ok()
            && check_lock(&self._profile, &self._lock).is_ok()
            && self.directory.check().is_ok()
            && let Ok(Some(metadata)) = self.directory.stat("node.sock")
            && metadata.st_ino == self.inode
            && safe_socket(&metadata).is_ok()
        {
            let _ = self.directory.unlink("node.sock");
        }
    }
}
fn safe_socket(metadata: &rustix::fs::Stat) -> Result<()> {
    if rustix::fs::FileType::from_raw_mode(metadata.st_mode) != rustix::fs::FileType::Socket
        || metadata.st_uid != rustix::process::geteuid().as_raw()
        || metadata.st_mode & 0o7777 != 0o600
        || metadata.st_nlink != 1
    {
        return Err(Error::Node("unsafe local session socket"));
    }
    Ok(())
}

fn check_lock(profile: &BoundDirectory, lock: &std::fs::File) -> Result<()> {
    let held = lock.metadata()?;
    let named = profile
        .stat("node.lock")?
        .ok_or(Error::Node("node lock disappeared"))?;
    if !held.is_file()
        || held.uid() != rustix::process::geteuid().as_raw()
        || held.mode() & 0o7777 != 0o600
        || held.nlink() != 1
        || (held.dev(), held.ino()) != stat_identity(&named)
        || named.st_nlink != 1
        || named.st_mode & 0o7777 != 0o600
    {
        return Err(Error::Node("node lock identity changed"));
    }
    Ok(())
}

// Darwin's dev_t is signed while std MetadataExt exposes u64. Keep the
// platform conversion in one place; Linux already uses u64 for these fields.
fn stat_identity(stat: &rustix::fs::Stat) -> (u64, u64) {
    #[cfg(target_vendor = "apple")]
    let device = stat.st_dev as u64;
    #[cfg(not(target_vendor = "apple"))]
    let device = stat.st_dev;
    (device, stat.st_ino)
}
fn check_user(stream: &UnixStream) -> Result<()> {
    if stream.peer_cred()?.uid() != rustix::process::geteuid().as_raw() {
        return Err(Error::Denied);
    }
    Ok(())
}
pub(crate) async fn read<T: serde::de::DeserializeOwned>(stream: &mut UnixStream) -> Result<T> {
    let length = stream.read_u32().await? as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(Error::Protocol);
    }
    let mut bytes = Zeroizing::new(vec![0; length]);
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|_| Error::Protocol)
}
pub(crate) async fn write<T: Serialize>(stream: &mut UnixStream, value: &T) -> Result<()> {
    let bytes = Zeroizing::new(serde_json::to_vec(value).map_err(|_| Error::Protocol)?);
    if bytes.len() > MAX_FRAME {
        return Err(Error::Protocol);
    }
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .await?;
    stream.write_all(&bytes).await?;
    Ok(())
}

pub(super) async fn serve(
    mut stream: UnixStream,
    handle: Handle,
    admission: tokio::sync::OwnedSemaphorePermit,
    text_slots: std::sync::Arc<tokio::sync::Semaphore>,
    file_slots: std::sync::Arc<tokio::sync::Semaphore>,
) {
    let mut admission = Some(admission);
    let mut _work = None;
    let operation = async {
        let request: Request = tokio::time::timeout(config::DEFAULT_TIMEOUT, read(&mut stream))
            .await
            .map_err(|_| Error::Timeout)??;
        let outcome: Result<Reply> = async {
            _work = match &request {
                Request::Text { .. } => Some(
                    text_slots
                        .try_acquire_owned()
                        .map_err(|_| Error::Node("local text queue is full"))?,
                ),
                Request::File { .. } => Some(
                    file_slots
                        .try_acquire_owned()
                        .map_err(|_| Error::Node("local file queue is full"))?,
                ),
                Request::Status => None,
            };
            // Long work has its own bounded lane. Keep reader/status admission
            // available instead of occupying every control slot with bulk files.
            if _work.is_some() {
                drop(admission.take());
            }
            match &request {
                Request::Text {
                    target,
                    text,
                    sensitive,
                    seconds,
                } => handle
                    .send_text(target, text, *sensitive, Duration::from_secs(*seconds))
                    .await
                    .map(|()| Reply::success(text.len() as u64, *sensitive)),
                Request::File {
                    target,
                    source,
                    sensitive,
                    seconds,
                } => handle
                    .send_file(target, source, *sensitive, Duration::from_secs(*seconds))
                    .await
                    .map(|receipt| {
                        let mut reply = Reply::success(receipt.info.bytes, receipt.sensitive);
                        reply.resumed_from = receipt.resumed_from;
                        if !receipt.sensitive {
                            reply.digest = Some(receipt.info.sha256);
                        }
                        reply
                    }),
                Request::Status => handle.status().await.map(|status| {
                    let mut reply = Reply::success(0, false);
                    reply.status = Some(status);
                    reply
                }),
            }
        }
        .await;
        let reply = outcome.unwrap_or_else(|error| Reply {
            ok: false,
            code: error.code().to_owned(),
            bytes: 0,
            resumed_from: 0,
            digest: None,
            sensitive: false,
            status: None,
        });
        tokio::time::timeout(config::DEFAULT_TIMEOUT, write(&mut stream, &reply))
            .await
            .map_err(|_| Error::Timeout)?
    };
    // Operation deadlines are validated by Handle; this bounds socket shutdown too.
    let _ =
        tokio::time::timeout(config::MAX_FILE_TIMEOUT + Duration::from_secs(5), operation).await;
}

pub async fn request(
    profile: &Path,
    request: &Request,
    deadline: Duration,
) -> Result<Option<Reply>> {
    let reply: Option<Reply> = exchange(profile, request, deadline).await?;
    if let Some(reply) = &reply {
        reply.result()?;
    }
    Ok(reply)
}

fn connection_error(error: std::io::Error) -> Error {
    use std::io::ErrorKind;
    // Keep the NODE classification and avoid echoing arbitrary OS/custom reason
    // strings. A failed connection is not authority to delete or recreate IPC.
    Error::Node(match error.kind() {
        ErrorKind::ConnectionRefused => {
            "local session socket has no listener; restart its foreground owner and retry explicitly; no automatic network retry"
        }
        ErrorKind::NotFound => {
            "local session socket path is unavailable; verify the runtime namespace and foreground owner, then retry explicitly"
        }
        ErrorKind::PermissionDenied => {
            "local session access denied; verify private profile/socket ownership, permissions and OS access policy without making them public"
        }
        _ => {
            "local session IPC connection failed; inspect its foreground diagnostics and retry explicitly; no automatic network retry"
        }
    })
}

pub(crate) async fn connect(profile: &Path) -> Result<Option<UnixStream>> {
    let profile = BoundDirectory::open(profile)?;
    let Some(directory) = endpoint(&profile, false)? else {
        return Ok(None);
    };
    let before = match directory.stat("node.sock")? {
        Some(metadata) => {
            safe_socket(&metadata)?;
            metadata
        }
        None => return Ok(None),
    };
    let stream = UnixStream::connect(directory.access_path().join("node.sock"))
        .await
        .map_err(connection_error)?;
    check_user(&stream)?;
    profile.check()?;
    directory.check()?;
    let after = directory
        .stat("node.sock")?
        .ok_or(Error::Node("local session socket disappeared"))?;
    safe_socket(&after)?;
    if (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino) {
        return Err(Error::Node("local session socket changed"));
    }
    Ok(Some(stream))
}

pub(crate) async fn exchange<Q: Serialize, R: serde::de::DeserializeOwned>(
    profile: &Path,
    request: &Q,
    deadline: Duration,
) -> Result<Option<R>> {
    crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
    tokio::time::timeout(deadline, async {
        let Some(mut stream) = connect(profile).await? else {
            return Ok(None);
        };
        write(&mut stream, request).await?;
        Ok(Some(read(&mut stream).await?))
    })
    .await
    .map_err(|_| Error::Timeout)?
}

#[cfg(test)]
mod path_tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let root = crate::test_support::tempdir().unwrap();
        let profile = root.path().join("long profile with spaces".repeat(6));
        crate::profile::private_directory(&profile, true).unwrap();
        (root, profile)
    }

    #[test]
    fn connection_diagnostics_are_actionable_and_keep_node_classification() {
        use std::io::{Error as IoError, ErrorKind};
        for (kind, expected) in [
            (ErrorKind::ConnectionRefused, "has no listener"),
            (ErrorKind::NotFound, "socket path is unavailable"),
            (ErrorKind::PermissionDenied, "without making them public"),
            (ErrorKind::Other, "inspect its foreground diagnostics"),
        ] {
            let error = connection_error(IoError::new(kind, "untrusted diagnostic reason"));
            assert_eq!(error.code(), "NODE");
            let diagnostic = error.to_string();
            assert!(diagnostic.contains(expected));
            assert!(!diagnostic.contains("untrusted diagnostic reason"));
        }
    }

    #[tokio::test]
    async fn absent_local_session_probe_creates_no_profile_entries() {
        let (root, profile) = fixture();
        let missing = root.path().join("absent");
        let error = connect(&missing).await.unwrap_err();
        assert!(matches!(error, Error::Io(error) if error.kind() == std::io::ErrorKind::NotFound));
        assert!(!missing.exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(&profile).unwrap().count(), 0);
        let result = tokio::time::timeout(Duration::from_secs(1), connect(&profile))
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_none());
        assert_eq!(std::fs::read_dir(&profile).unwrap().count(), 0);
        assert_eq!(std::fs::metadata(&profile).unwrap().mode() & 0o7777, 0o700);
    }

    #[tokio::test]
    async fn stale_local_socket_diagnostic_never_cleans_or_replaces_the_endpoint() {
        let (_root, profile) = fixture();
        let owner = Listener::bind(&profile).await.unwrap();
        let socket = owner.path.clone();
        drop(owner);
        let stale = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
        drop(stale); // Retain a socket name with no listening process.
        let before = std::fs::symlink_metadata(&socket).unwrap();
        let directory = socket.parent().unwrap();
        let entries = std::fs::read_dir(directory).unwrap().count();
        let error = tokio::time::timeout(Duration::from_secs(1), connect(&profile))
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code(), "NODE");
        assert!(error.to_string().contains("socket has no listener"));
        let after = std::fs::symlink_metadata(&socket).unwrap();
        assert_eq!(
            (before.dev(), before.ino(), before.mode(), before.nlink()),
            (after.dev(), after.ino(), after.mode(), after.nlink())
        );
        assert_eq!(std::fs::read_dir(directory).unwrap().count(), entries);
    }

    #[tokio::test]
    async fn owner_release_is_not_extended_by_an_inherited_lock_description() {
        let (_root, profile) = fixture();
        let listener = Listener::bind(&profile).await.unwrap();
        let inherited = listener._lock.try_clone().unwrap();
        assert!(Listener::bind(&profile).await.is_err());
        drop(listener);
        let replacement = Listener::bind(&profile).await.unwrap();
        drop(inherited);
        assert!(Listener::bind(&profile).await.is_err());
        drop(replacement);
        assert!(Listener::bind(&profile).await.is_ok());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn binding_publication_cannot_overwrite_a_racing_entry() {
        let (root, _) = fixture();
        let directory = BoundDirectory::open(root.path()).unwrap();
        let existing = root.path().join("profile");
        let staged = root.path().join(".profile-init-race");
        std::fs::write(&existing, b"retained binding").unwrap();
        std::fs::write(&staged, b"candidate binding").unwrap();
        let inode = std::fs::metadata(&existing).unwrap().ino();
        assert!(publish_binding(&directory.file, staged.file_name().unwrap()).is_err());
        assert_eq!(std::fs::read(&existing).unwrap(), b"retained binding");
        assert_eq!(std::fs::read(&staged).unwrap(), b"candidate binding");
        assert_eq!(std::fs::metadata(&existing).unwrap().ino(), inode);
    }

    #[tokio::test]
    async fn unsafe_ancestors_and_persistent_ancestor_replacement_are_refused() {
        let (root, profile) = fixture();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(Listener::bind(&profile).await.is_err());
        assert!(connect(&profile).await.is_err());
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let parent = profile.parent().unwrap();
        let held = BoundDirectory::open(&profile).unwrap();
        std::fs::write(profile.join("owned-entry"), b"original").unwrap();
        let moved = parent.with_extension("ancestor-retained");
        std::fs::rename(parent, &moved).unwrap();
        std::fs::create_dir(parent).unwrap();
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        crate::profile::private_directory(&profile, true).unwrap();
        assert!(held.check().is_err());
        std::fs::write(profile.join("owned-entry"), b"replacement").unwrap();
        held.unlink("owned-entry").unwrap();
        assert_eq!(
            std::fs::read(profile.join("owned-entry")).unwrap(),
            b"replacement"
        );
        assert!(
            !moved
                .join(profile.file_name().unwrap())
                .join("owned-entry")
                .exists()
        );
        // Restore so the temporary fixture, not some new path, owns cleanup.
        std::fs::remove_file(profile.join("owned-entry")).unwrap();
        std::fs::remove_dir(&profile).unwrap();
        std::fs::remove_dir(parent).unwrap();
        std::fs::rename(&moved, parent).unwrap();
    }

    #[tokio::test]
    async fn named_lock_replacement_and_hardlinks_invalidate_lifecycle() {
        let (_root, profile) = fixture();
        let listener = Listener::bind(&profile).await.unwrap();
        let socket = listener.path.clone();
        let lock = profile.join("node.lock");
        std::fs::hard_link(&lock, profile.join("lock-alias")).unwrap();
        assert!(check_lock(&listener._profile, &listener._lock).is_err());
        std::fs::remove_file(profile.join("lock-alias")).unwrap();
        std::fs::rename(&lock, profile.join("retained-lock")).unwrap();
        std::fs::write(&lock, b"").unwrap();
        std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(check_lock(&listener._profile, &listener._lock).is_err());
        assert!(listener.accept().await.is_err());
        assert!(Listener::bind(&profile).await.is_err());
        drop(listener);
        assert!(socket.exists());
    }

    #[tokio::test]
    async fn interrupted_binding_is_recovered_without_deleting_ambiguous_data() {
        let (_root, profile) = fixture();
        let listener = Listener::bind(&profile).await.unwrap();
        let binding = listener.path.parent().unwrap().join("profile");
        drop(listener);
        let expected = std::fs::read(&binding).unwrap();
        std::fs::write(&binding, &expected[..expected.len() / 2]).unwrap();
        let listener = Listener::bind(&profile).await.unwrap();
        assert_eq!(std::fs::read(&binding).unwrap(), expected);
        drop(listener);
        std::fs::remove_file(&binding).unwrap();
        let staged = binding.with_file_name(".profile-init-interrupted");
        std::fs::write(&staged, b"retained staging data").unwrap();
        let listener = Listener::bind(&profile).await.unwrap();
        assert_eq!(std::fs::read(&staged).unwrap(), b"retained staging data");
        drop(listener);
        std::fs::remove_file(&binding).unwrap();
        let unknown = binding.with_file_name("unknown-user-data");
        std::fs::write(&unknown, b"do not adopt or delete").unwrap();
        assert!(Listener::bind(&profile).await.is_err());
        assert_eq!(std::fs::read(unknown).unwrap(), b"do not adopt or delete");
        assert!(!binding.exists());
    }

    #[tokio::test]
    async fn interrupted_binding_recovery_scan_is_bounded_and_preserves_stages() {
        let (_root, profile) = fixture();
        let listener = Listener::bind(&profile).await.unwrap();
        let directory = listener.path.parent().unwrap().to_owned();
        drop(listener);
        std::fs::remove_file(directory.join("profile")).unwrap();
        for index in 0..=MAX_BINDING_STAGES {
            std::fs::write(
                directory.join(format!(".profile-init-{index}")),
                b"retained",
            )
            .unwrap();
        }
        assert!(Listener::bind(&profile).await.is_err());
        assert!(!directory.join("profile").exists());
        assert_eq!(
            std::fs::read_dir(&directory).unwrap().count(),
            MAX_BINDING_STAGES + 1
        );
        assert_eq!(
            std::fs::read(directory.join(".profile-init-0")).unwrap(),
            b"retained"
        );
        std::fs::remove_file(directory.join(format!(".profile-init-{MAX_BINDING_STAGES}")))
            .unwrap();
        let listener = Listener::bind(&profile).await.unwrap();
        assert_eq!(
            std::fs::read_dir(&directory).unwrap().count(),
            MAX_BINDING_STAGES + 2
        );
        drop(listener);
    }

    #[test]
    fn chmod_never_follows_a_replaced_socket_symlink() {
        let (_root, profile) = fixture();
        let target = profile.join("retained-target");
        std::fs::write(&target, b"retained").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o400)).unwrap();
        std::os::unix::fs::symlink(&target, profile.join("node.sock")).unwrap();
        let directory = BoundDirectory::open(&profile).unwrap();
        let _ = chmod_entry(&directory.file, "node.sock", 0o600, true);
        assert_eq!(std::fs::metadata(&target).unwrap().mode() & 0o7777, 0o400);
    }

    #[test]
    fn restrictive_umask_runs_only_in_an_isolated_subprocess() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .env("KK_TEST_IPC_UMASK_CHILD", "1")
            .args([
                "--exact",
                "node::ipc::path_tests::restrictive_umask_child",
                "--ignored",
                "--nocapture",
            ])
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[tokio::test]
    #[ignore = "subprocess only: changes the child process umask"]
    async fn restrictive_umask_child() {
        // `--ignored` can also select this alongside other tests. Never mutate
        // a shared harness process unless the isolated parent explicitly armed it.
        if std::env::var("KK_TEST_IPC_UMASK_CHILD").as_deref() != Ok("1") {
            return;
        }
        let (_root, profile) = fixture();
        rustix::process::umask(rustix::fs::Mode::from_raw_mode(0o777));
        let listener = Listener::bind(&profile).await.unwrap();
        assert!(connect(&profile).await.unwrap().is_some());
        assert_eq!(
            std::fs::metadata(listener.path.parent().unwrap().join("profile"))
                .unwrap()
                .mode()
                & 0o7777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(profile.join("node.lock")).unwrap().mode() & 0o7777,
            0o600
        );
        let directory = listener.path.parent().unwrap().to_owned();
        drop(listener);
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o000)).unwrap();
        let recovered = Listener::bind(&profile).await.unwrap();
        assert_eq!(
            std::fs::metadata(&directory).unwrap().mode() & 0o7777,
            0o700
        );
        drop(recovered);
    }

    #[tokio::test]
    async fn binding_is_authority_not_the_public_hash_and_unsafe_endpoints_fail_closed() {
        let (_root, profile) = fixture();
        let listener = Listener::bind(&profile).await.unwrap();
        let socket = listener.path.clone();
        let binding = socket.parent().unwrap().join("profile");
        let original = std::fs::read(&binding).unwrap();
        std::fs::write(&binding, vec![b'x'; original.len()]).unwrap();
        assert!(connect(&profile).await.is_err());
        drop(listener);
        assert!(Listener::bind(&profile).await.is_err());
        std::fs::write(&binding, original).unwrap();
        let listener = Listener::bind(&profile).await.unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(connect(&profile).await.is_err());
        drop(listener);
        assert!(socket.exists()); // Unsafe endpoint is never removed or adopted.
        assert!(Listener::bind(&profile).await.is_err());
        std::fs::remove_file(&socket).unwrap();
        std::os::unix::fs::symlink(&binding, &socket).unwrap();
        assert!(connect(&profile).await.is_err());
        assert!(Listener::bind(&profile).await.is_err());
    }

    #[tokio::test]
    async fn interrupted_runtime_modes_require_inspection_without_normalization() {
        let (_root, profile) = fixture();
        let listener = Listener::bind(&profile).await.unwrap();
        let socket = listener.path.clone();
        drop(listener);
        let lock = profile.join("node.lock");
        std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert!(Listener::bind(&profile).await.is_err());
        assert_eq!(std::fs::metadata(&lock).unwrap().mode() & 0o7777, 0);
        // Explicit test-only inspection restores the empty owned runtime file.
        std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(std::fs::metadata(&lock).unwrap().len(), 0);
        for mode in [0o000, 0o755] {
            let stale = UnixListener::bind(&socket).unwrap();
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(mode)).unwrap();
            drop(stale);
            assert!(Listener::bind(&profile).await.is_err());
            assert_eq!(std::fs::metadata(&socket).unwrap().mode() & 0o7777, mode);
            std::fs::remove_file(&socket).unwrap();
        }
        assert!(Listener::bind(&profile).await.is_ok());
    }

    #[tokio::test]
    async fn stale_socket_requires_owner_lock_and_shutdown_never_unlinks_a_replacement() {
        // Other library tests fork children. CLOEXEC does not close a copied
        // listener until exec, so a raw-listener close fixture needs its own
        // harness process rather than assuming it owns every description.
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .env("KK_TEST_IPC_LIFECYCLE_CHILD", "1")
            .args([
                "--exact",
                "node::ipc::path_tests::stale_socket_lifecycle_child",
                "--ignored",
                "--nocapture",
            ])
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let status = match tokio::time::timeout(
            crate::config::DEFAULT_TIMEOUT.saturating_mul(4),
            child.wait(),
        )
        .await
        {
            Ok(status) => status.unwrap(),
            Err(_) => {
                child
                    .kill()
                    .await
                    .expect("kill and reap timed-out IPC fixture");
                panic!("isolated IPC lifecycle fixture exceeded its deadline");
            }
        };
        assert!(status.success());
    }

    #[tokio::test]
    #[ignore = "subprocess only: raw listener close must own all descriptor aliases"]
    async fn stale_socket_lifecycle_child() {
        if std::env::var("KK_TEST_IPC_LIFECYCLE_CHILD").as_deref() != Ok("1") {
            return;
        }
        let (_root, profile) = fixture();
        let listener = Listener::bind(&profile).await.unwrap();
        let socket = listener.path.clone();
        std::fs::remove_file(&socket).unwrap();
        let replacement = UnixListener::bind(&socket).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
        drop(listener);
        assert!(socket.exists());
        assert!(matches!(
            Listener::bind(&profile).await,
            Err(Error::Node("active node socket without lock"))
        ));
        assert!(
            socket.exists(),
            "the refused bind must preserve the live socket"
        );
        let retained = rustix::io::dup(&replacement).unwrap();
        drop(replacement);
        // An inherited/duplicated description is still a live listener, not a
        // stale socket. Both client connectivity and bind refusal must remain.
        assert!(connect(&profile).await.unwrap().is_some());
        assert!(matches!(
            Listener::bind(&profile).await,
            Err(Error::Node("active node socket without lock"))
        ));
        drop(retained);
        assert!(
            socket.exists(),
            "dropping the raw listener must leave its path"
        );
        match connect(&profile).await {
            Err(_) => {}
            Ok(None) => panic!("stale socket unexpectedly absent: {}", socket.exists()),
            Ok(Some(_)) => panic!("closed replacement listener still accepted a connection"),
        } // No client-side stale cleanup.
        let recovered = Listener::bind(&profile).await.unwrap();
        drop(recovered);
        assert!(!socket.exists());
    }

    #[tokio::test]
    async fn replaced_directory_and_cross_profile_binding_are_refused() {
        let (root, profile) = fixture();
        let listener = Listener::bind(&profile).await.unwrap();
        let socket = listener.path.clone();
        let directory = socket.parent().unwrap();
        let moved = directory.with_extension("retained");
        std::fs::rename(directory, &moved).unwrap();
        std::fs::DirBuilder::new().create(directory).unwrap();
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let replacement = UnixListener::bind(&socket).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
        drop(listener);
        assert!(socket.exists());
        assert!(connect(&profile).await.is_err());
        assert!(Listener::bind(&profile).await.is_err()); // Missing binding is not adopted.
        drop(replacement);
        let other = root.path().join("different profile".repeat(8));
        crate::profile::private_directory(&other, true).unwrap();
        let other_listener = Listener::bind(&other).await.unwrap();
        std::fs::copy(
            moved.join("profile"),
            other_listener.path.parent().unwrap().join("profile"),
        )
        .unwrap();
        assert!(connect(&other).await.is_err());
    }

    #[tokio::test]
    async fn legacy_short_profile_keeps_its_socket_and_long_legacy_is_not_bypassed() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let profile = root.path().join("short");
        crate::profile::private_directory(&profile, true).unwrap();
        let listener = Listener::bind(&profile).await.unwrap();
        // Darwin's temporary directory can use the /var -> /private/var alias.
        // Compare the physical endpoint while retaining the legacy input below.
        assert_eq!(
            listener.path,
            std::fs::canonicalize(&profile).unwrap().join("node.sock")
        );
        assert!(connect(&profile).await.unwrap().is_some());
        // A legacy client using the ordinary filesystem spelling still works
        // when Linux's new server binds through its pinned directory FD.
        assert!(UnixStream::connect(profile.join("node.sock")).await.is_ok());
        let (_root, long) = fixture();
        std::fs::write(long.join("node.sock"), b"do not adopt").unwrap();
        assert!(Listener::bind(&long).await.is_err());
        assert!(connect(&long).await.is_err());
    }

    #[tokio::test]
    async fn private_directory_mode_and_single_link_socket_are_required() {
        let (_root, profile) = fixture();
        let listener = Listener::bind(&profile).await.unwrap();
        let socket = listener.path.clone();
        let directory = socket.parent().unwrap();
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o750)).unwrap();
        assert!(connect(&profile).await.is_err());
        assert!(Listener::bind(&profile).await.is_err());
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let alias = directory.join("not-an-authority");
        std::fs::hard_link(&socket, &alias).unwrap();
        assert!(connect(&profile).await.is_err());
        drop(listener);
        assert!(socket.exists());
        assert!(Listener::bind(&profile).await.is_err());
    }
}
