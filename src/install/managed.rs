//! Descriptor-relative Unix filesystem transaction. No path-based extraction.
use super::{
    Error, Outcome, Result,
    archive::{MAX_BYTES, Verified, path},
    digest, refuse,
};
use rustix::{
    fd::OwnedFd,
    fs::{self, AtFlags, FileType, Mode, OFlags},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Write},
    path::{Component, Path},
};

struct Dir(OwnedFd);

fn owner(stat: &fs::Stat) -> Result<()> {
    if stat.st_uid != rustix::process::geteuid().as_raw() {
        return refuse("managed object is not owned by this user");
    }
    Ok(())
}

impl Dir {
    fn open(&self, name: &str) -> Result<Self> {
        Ok(Self(fs::openat(
            &self.0,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?))
    }
    fn child(&self, name: &str, private: bool) -> Result<Self> {
        match fs::mkdirat(&self.0, name, Mode::from_raw_mode(0o700)) {
            Ok(()) => fs::fsync(&self.0)?,
            Err(rustix::io::Errno::EXIST) => (),
            Err(error) => return Err(error.into()),
        }
        let dir = self.open(name)?;
        let stat = fs::fstat(&dir.0)?;
        owner(&stat)?;
        if stat.st_mode & if private { 0o077 } else { 0o022 } != 0 {
            return refuse("managed directory permissions are unsafe");
        }
        Ok(dir)
    }
    fn write(
        &self,
        name: &str,
        data: &[u8],
        executable: bool,
        cancel: &super::Cancel,
    ) -> Result<()> {
        cancel.check()?;
        let descriptor = fs::openat(
            &self.0,
            name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?;
        let mut file = File::from(descriptor);
        for chunk in data.chunks(65536) {
            cancel.check()?;
            file.write_all(chunk)?;
        }
        fs::fchmod(
            &file,
            Mode::from_raw_mode(if executable { 0o555 } else { 0o444 }),
        )?;
        file.sync_all()?;
        Ok(())
    }
    fn read(
        &self,
        name: &str,
        limit: usize,
        mode: fs::RawMode,
        cancel: &super::Cancel,
    ) -> Result<Vec<u8>> {
        cancel.check()?;
        let descriptor = fs::openat(
            &self.0,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let stat = fs::fstat(&descriptor)?;
        owner(&stat)?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
            || stat.st_nlink != 1
            || stat.st_size < 0
            || stat.st_size as u64 > limit as u64
            || stat.st_mode & 0o7777 != mode
        {
            return refuse("managed file type, mode, links or size differs");
        }
        let mut bytes = Vec::new();
        let mut file = File::from(descriptor).take(limit as u64 + 1);
        let mut chunk = [0u8; 65536];
        loop {
            cancel.check()?;
            let count = file.read(&mut chunk)?;
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
        if bytes.len() > limit {
            return refuse("managed file exceeds budget");
        }
        Ok(bytes)
    }
    fn names(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in fs::Dir::read_from(&self.0)? {
            let entry = entry?;
            let bytes = entry.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            if names.len() >= 4098 {
                return refuse("managed directory exceeds member budget");
            }
            let name = std::str::from_utf8(bytes)
                .map_err(|_| Error::Refused("invalid managed filename"))?;
            path(name)?;
            names.push(name.to_owned());
        }
        Ok(names)
    }
    fn freeze(&self, cancel: &super::Cancel) -> Result<()> {
        self.freeze_descendants(cancel)?;
        self.freeze_root(cancel)
    }
    fn freeze_descendants(&self, cancel: &super::Cancel) -> Result<()> {
        cancel.check()?;
        for name in self.names()? {
            let stat = fs::statat(&self.0, &name, AtFlags::SYMLINK_NOFOLLOW)?;
            if FileType::from_raw_mode(stat.st_mode) == FileType::Directory {
                self.open(&name)?.freeze(cancel)?;
            }
        }
        fs::fsync(&self.0)?;
        Ok(())
    }
    fn freeze_root(&self, cancel: &super::Cancel) -> Result<()> {
        cancel.check()?;
        fs::fchmod(&self.0, Mode::from_raw_mode(0o500))?;
        fs::fsync(&self.0)?;
        Ok(())
    }
}

fn anchor(prefix: &Path, cancel: &super::Cancel) -> Result<Dir> {
    cancel.check()?;
    if rustix::process::geteuid().is_root()
        || rustix::process::geteuid() != rustix::process::getuid()
    {
        return refuse("installer refuses root or elevated execution");
    }
    if !prefix.is_absolute() {
        return refuse("prefix must be absolute");
    }
    let mut dir = Dir(fs::open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    let parts: Vec<_> = prefix.components().collect();
    if parts.len() < 2 || parts.len() > 64 {
        return refuse("invalid prefix depth");
    }
    for (index, component) in parts.iter().enumerate().skip(1) {
        cancel.check()?;
        let Component::Normal(name) = component else {
            return refuse("noncanonical prefix");
        };
        // Unicode and spaces are valid filesystem components, not shell input.
        match fs::mkdirat(&dir.0, *name, Mode::from_raw_mode(0o700)) {
            Ok(()) => fs::fsync(&dir.0)?,
            Err(rustix::io::Errno::EXIST) => (),
            Err(error) => return Err(error.into()),
        }
        dir = Dir(fs::openat(
            &dir.0,
            *name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let stat = fs::fstat(&dir.0)?;
        let uid = rustix::process::geteuid().as_raw();
        let last = index + 1 == parts.len();
        if stat.st_uid != uid && (last || stat.st_uid != 0) {
            return refuse("prefix ancestor has foreign owner");
        }
        if stat.st_mode & 0o022 != 0 && (last || stat.st_uid != 0 || stat.st_mode & 0o1000 == 0) {
            return refuse("prefix ancestor is writable by another user");
        }
    }
    Ok(dir)
}

fn nonce() -> Result<String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut bytes = [0u8; 16];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::Refused("cannot create transaction nonce"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

struct Managed {
    versions: Dir,
    bin: Dir,
    _lock: File,
    cancel: super::Cancel,
}

impl Managed {
    fn open(prefix: &Path, cancel: super::Cancel) -> Result<Self> {
        let prefix = anchor(prefix, &cancel)?;
        let versions = prefix.child("lib", false)?.child("kk", true)?;
        let lock = fs::openat(
            &versions.0,
            ".install.lock",
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?;
        let stat = fs::fstat(&lock)?;
        owner(&stat)?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
            || stat.st_nlink != 1
            || stat.st_mode & 0o7777 != 0o600
        {
            return refuse("foreign installation lock");
        }
        fs::flock(&lock, fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| Error::Refused("another installer holds the transaction lock"))?;
        let bin = prefix.child("bin", false)?;
        Ok(Self {
            versions,
            bin,
            _lock: File::from(lock),
            cancel,
        })
    }
    fn active(&self, target: &str) -> Result<Option<String>> {
        self.cancel.check()?;
        let stat = match fs::statat(&self.bin.0, "kk", AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        owner(&stat)?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::Symlink || stat.st_nlink != 1 {
            return refuse("refusing foreign active executable");
        }
        let link = fs::readlinkat(&self.bin.0, "kk", Vec::new())?;
        let text = link
            .to_str()
            .map_err(|_| Error::Refused("foreign activation link"))?;
        let Some(selected) = text
            .strip_prefix("../lib/kk/")
            .and_then(|s| s.strip_suffix("/kk"))
        else {
            return refuse("foreign activation link");
        };
        super::version(selected)?;
        self.check(selected, target)?;
        Ok(Some(selected.to_owned()))
    }
    // Ownership/corruption consistency only, NOT origin authentication. Requested
    // versions must additionally match a freshly gh-authenticated original archive.
    fn check(&self, version: &str, target: &str) -> Result<Receipt> {
        self.check_root_mode(version, target, 0o500)
    }
    fn check_root_mode(&self, version: &str, target: &str, mode: fs::RawMode) -> Result<Receipt> {
        let dir = self.versions.open(version)?;
        let bytes = dir.read(".kk-receipt.json", 4 * 1024 * 1024, 0o444, &self.cancel)?;
        let receipt: Receipt = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Refused("invalid managed receipt"))?;
        if receipt.format != 1
            || receipt.version != version
            || receipt.target != target
            || !super::sha(&receipt.archive_sha256)
            || receipt.files.is_empty()
            || receipt.files.len() > 4096
            || !receipt.files.contains_key("kk")
            || !receipt.files.contains_key("build-manifest.json")
        {
            return refuse("foreign managed version");
        }
        let mut actual = BTreeMap::new();
        let mut allowed = BTreeSet::from([".kk-receipt.json".to_owned(), ".kk-archive".to_owned()]);
        for name in receipt.files.keys() {
            path(name)?;
            let parts: Vec<_> = name.split('/').collect();
            for count in 1..=parts.len() {
                allowed.insert(parts[..count].join("/"));
            }
        }
        walk(
            &dir,
            ("", mode),
            &mut actual,
            &mut 0usize,
            &allowed,
            &self.cancel,
        )?;
        actual.remove(".kk-receipt.json");
        if actual.remove(".kk-archive").as_deref() != Some(&receipt.archive_sha256) {
            return refuse("retained original archive changed");
        }
        if actual != receipt.files {
            return refuse("managed version bytes have changed");
        }
        // Recheck the manifest binding, not just a path under the managed root.
        let manifest: super::archive::Manifest = serde_json::from_slice(&dir.read(
            "build-manifest.json",
            4 * 1024 * 1024,
            0o444,
            &self.cancel,
        )?)
        .map_err(|_| Error::Refused("invalid retained manifest"))?;
        let mut declared = receipt.files.clone();
        declared.remove("build-manifest.json");
        if manifest.format != 1
            || manifest.version != receipt.version
            || manifest.commit != receipt.commit
            || manifest.target != receipt.target
            || manifest.files != declared
        {
            return refuse("retained manifest differs from receipt");
        }
        Ok(receipt)
    }
    fn activate(&self, version: &str, previous: Option<&str>) -> Result<()> {
        self.cancel.check()?;
        // Frozen mode alone does not prove that an interrupted finalization's
        // directory sync succeeded. Retry this barrier for install and rollback.
        fs::fsync(&self.versions.open(version)?.0)?;
        let transaction = nonce()?;
        let record = serde_json::to_vec(&serde_json::json!({"format":1, "state":"prepared", "previous":previous, "requested":version}))
            .map_err(|_| Error::Refused("cannot encode activation record"))?;
        self.versions.write(
            &format!(".activation-{transaction}.json"),
            &record,
            false,
            &self.cancel,
        )?;
        fs::fsync(&self.versions.0)?;
        let temporary = format!(".kk-next-{transaction}");
        fs::symlinkat(format!("../lib/kk/{version}/kk"), &self.bin.0, &temporary)?;
        fs::fsync(&self.bin.0)?;
        // Atomic commit point: cancellation racing the rename sees old or new
        // verified activation, never partially written executable bytes.
        self.cancel.check()?;
        fs::renameat(&self.bin.0, &temporary, &self.bin.0, "kk")?;
        fs::fsync(&self.bin.0)?;
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    format: u32,
    version: String,
    target: String,
    commit: String,
    archive_sha256: String,
    files: BTreeMap<String, String>,
}

fn walk(
    dir: &Dir,
    (prefix, mode): (&str, fs::RawMode),
    files: &mut BTreeMap<String, String>,
    total: &mut usize,
    allowed: &BTreeSet<String>,
    cancel: &super::Cancel,
) -> Result<()> {
    cancel.check()?;
    let stat = fs::fstat(&dir.0)?;
    owner(&stat)?;
    if stat.st_mode & 0o7777 != mode {
        return refuse("managed directory mode changed");
    }
    for name in dir.names()? {
        let relative = format!("{prefix}{name}");
        path(&relative)?;
        if !allowed.contains(&relative) {
            return refuse("undeclared managed entry");
        }
        let stat = fs::statat(&dir.0, &name, AtFlags::SYMLINK_NOFOLLOW)?;
        if FileType::from_raw_mode(stat.st_mode) == FileType::Directory {
            walk(
                &dir.open(&name)?,
                (&(relative + "/"), 0o500),
                files,
                total,
                allowed,
                cancel,
            )?;
        } else {
            let bytes = dir.read(
                &name,
                if relative == ".kk-archive" {
                    MAX_BYTES
                } else {
                    128 * 1024 * 1024
                },
                if relative == "kk" { 0o555 } else { 0o444 },
                cancel,
            )?;
            *total = total
                .checked_add(bytes.len())
                .ok_or(Error::Refused("managed size overflow"))?;
            if *total > MAX_BYTES * 2 + 4 * 1024 * 1024 || files.len() >= 4098 {
                return refuse("managed tree exceeds budget");
            }
            files.insert(relative, digest(&bytes));
        }
    }
    Ok(())
}

pub(super) fn install(
    prefix: &Path,
    archive: &Verified,
    archive_sha256: &str,
    original: &[u8],
    cancel: super::Cancel,
) -> Result<Outcome> {
    let managed = Managed::open(prefix, cancel)?;
    let previous = managed.active(&archive.manifest.target)?;
    let version = &archive.manifest.version;
    match fs::statat(&managed.versions.0, version, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(_) => {
            let existing = managed.versions.open(version)?;
            let pending = fs::fstat(&existing.0)?.st_mode & 0o7777 == 0o700;
            // Only an explicit install retry with freshly authenticated inputs may
            // finalize a post-rename interruption. All descendants remain strict.
            let receipt = managed.check_root_mode(
                version,
                &archive.manifest.target,
                if pending { 0o700 } else { 0o500 },
            )?;
            let fresh: BTreeMap<_, _> = archive
                .files
                .iter()
                .map(|(name, bytes)| (name.clone(), digest(bytes)))
                .collect();
            if receipt.archive_sha256 != archive_sha256
                || receipt.commit != archive.manifest.commit
                || receipt.files != fresh
            {
                return refuse("same version already exists with different archive bytes");
            }
            if pending {
                // No mutation until the retained archive, full tree, receipt and
                // manifest all match the fresh authenticated digest and bytes.
                existing.freeze_root(&managed.cancel)?;
            }
            // An earlier retry may already have frozen the root but failed the
            // publication-directory sync. Re-establish that barrier explicitly.
            fs::fsync(&managed.versions.0)?;
            if pending {
                managed.check(version, &archive.manifest.target)?;
            }
        }
        Err(rustix::io::Errno::NOENT) => {
            let staging = format!(".stage-{}", nonce()?);
            managed.cancel.check()?;
            fs::mkdirat(&managed.versions.0, &staging, Mode::from_raw_mode(0o700))?;
            let stage = managed.versions.open(&staging)?;
            let mut files = BTreeMap::new();
            for (name, bytes) in &archive.files {
                managed.cancel.check()?;
                let components: Vec<_> = name.split('/').collect();
                let mut dir = stage.open(".")?;
                for component in &components[..components.len() - 1] {
                    managed.cancel.check()?;
                    dir = dir.child(component, true)?;
                }
                dir.write(
                    components[components.len() - 1],
                    bytes,
                    name == "kk",
                    &managed.cancel,
                )?;
                files.insert(name.clone(), digest(bytes));
            }
            let receipt = Receipt {
                format: 1,
                version: version.clone(),
                target: archive.manifest.target.clone(),
                commit: archive.manifest.commit.clone(),
                archive_sha256: archive_sha256.to_owned(),
                files,
            };
            stage.write(".kk-archive", original, false, &managed.cancel)?;
            stage.write(
                ".kk-receipt.json",
                &serde_json::to_vec(&receipt)
                    .map_err(|_| Error::Refused("cannot encode receipt"))?,
                false,
                &managed.cancel,
            )?;
            // Darwin can refuse renaming a write-disabled directory. Descendants
            // are immutable, but keep the private stage root 0700 until published.
            stage.freeze_descendants(&managed.cancel)?;
            // Publish the complete tree without replacing any prior version;
            // final root immutability is required before validation/activation.
            managed.cancel.check()?;
            fs::renameat_with(
                &managed.versions.0,
                &staging,
                &managed.versions.0,
                version,
                fs::RenameFlags::NOREPLACE,
            )?;
            // Failure here leaves a visible, unactivated version: not "unchanged"
            // and not grounds for automatic rollback or overwriting that version.
            stage.freeze_root(&managed.cancel)?;
            fs::fsync(&managed.versions.0)?;
            managed.check(version, &archive.manifest.target)?;
        }
        Err(error) => return Err(error.into()),
    }
    managed.activate(version, previous.as_deref())?;
    Ok(Outcome {
        version: version.clone(),
        previous,
        executable: prefix.join("bin/kk"),
    })
}

pub(super) fn rollback(
    prefix: &Path,
    fresh: &Verified,
    original_digest: &str,
    cancel: super::Cancel,
) -> Result<Outcome> {
    let managed = Managed::open(prefix, cancel)?;
    let version = &fresh.manifest.version;
    let target = &fresh.manifest.target;
    let previous = managed
        .active(target)?
        .ok_or(Error::Refused("no managed activation to roll back"))?;
    let receipt = managed.check(version, target)?;
    let fresh_files: BTreeMap<_, _> = fresh
        .files
        .iter()
        .map(|(name, bytes)| (name.clone(), digest(bytes)))
        .collect();
    if receipt.files != fresh_files
        || receipt.archive_sha256 != original_digest
        || receipt.commit != fresh.manifest.commit
    {
        return refuse("retained version differs from freshly authenticated archive");
    }
    managed.activate(version, Some(&previous))?;
    Ok(Outcome {
        version: version.to_owned(),
        previous: Some(previous),
        executable: prefix.join("bin/kk"),
    })
}

// Selectors in this local record are UNTRUSTED until gh checks the original
// archive against repository/workflow/tag/source. The receipt is not provenance.
pub(super) fn rollback_input(
    prefix: &Path,
    version: &str,
    cancel: super::Cancel,
) -> Result<(Vec<u8>, String)> {
    let managed = Managed::open(prefix, cancel)?;
    let dir = managed.versions.open(version)?;
    let record = dir.read(".kk-receipt.json", 4 * 1024 * 1024, 0o444, &managed.cancel)?;
    let receipt: Receipt = serde_json::from_slice(&record)
        .map_err(|_| Error::Refused("invalid rollback selectors"))?;
    if receipt.version != version
        || receipt.commit.len() != 40
        || !receipt
            .commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return refuse("invalid rollback selectors");
    }
    Ok((
        dir.read(".kk-archive", MAX_BYTES, 0o444, &managed.cancel)?,
        receipt.commit,
    ))
}

#[cfg(test)]
#[path = "managed_publication_tests.rs"]
mod publication_tests;
