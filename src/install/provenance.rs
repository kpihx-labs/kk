//! External verification tooling is an explicit prerequisite, never auto-installed.
use super::{
    Error, InstallRequest, Result,
    archive::{MAX_BYTES, Verified},
    refuse,
};
use std::{
    fs::File,
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};

pub(super) fn snapshot(path: &Path, max: usize, cancel: &super::Cancel) -> Result<Vec<u8>> {
    cancel.check()?;
    if !path.is_absolute() {
        return refuse("local input paths must be absolute");
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(
            rustix::fs::OFlags::NOFOLLOW.bits() as i32 | rustix::fs::OFlags::NONBLOCK.bits() as i32,
        )
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > max as u64 {
        return refuse("local input is non-ordinary or oversized");
    }
    let mut data = Vec::new();
    let mut file = file.take(max as u64 + 1);
    let mut chunk = [0u8; 65536];
    loop {
        cancel.check()?;
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        data.extend_from_slice(&chunk[..count]);
    }
    if data.len() > max {
        return refuse("local input exceeds resource budget");
    }
    Ok(data)
}

struct Tool {
    child: tokio::process::Child,
    group: Option<rustix::process::Pid>,
}

impl Drop for Tool {
    fn drop(&mut self) {
        // Synchronous fallback if the runtime itself drops the cleanup owner.
        // The unreaped direct child reserves the process-group identity.
        if let Some(group) = self.group {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        }
    }
}

async fn run(mut command: Command, limit: usize, seconds: u64) -> Result<Vec<u8>> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .process_group(0);
    let child = command
        .spawn()
        .map_err(|_| Error::Refused("required verification/download tool unavailable"))?;
    let group = rustix::process::Pid::from_raw(
        child.id().ok_or(Error::Refused("missing tool PID"))? as i32,
    )
    .ok_or(Error::Refused("invalid tool PID"))?;
    let mut tool = Tool {
        child,
        group: Some(group),
    };
    let mut output = tool
        .child
        .stdout
        .take()
        .ok_or(Error::Refused("cannot capture tool output"))?;
    let (stop, mut stopped) = tokio::sync::oneshot::channel::<()>();
    // Like media/process.rs, one detached owner retains child/reaping lifetime;
    // dropping the caller's sender requests cleanup rather than dropping ownership.
    let owner = tokio::spawn(async move {
        let operation = async {
            let mut bytes = Vec::new();
            (&mut output)
                .take(limit as u64 + 1)
                .read_to_end(&mut bytes)
                .await?;
            if bytes.len() > limit {
                return refuse("external tool output exceeds resource budget");
            }
            // Observe without reaping: pin the group ID until descendants are killed.
            loop {
                match rustix::process::waitid(
                    rustix::process::WaitId::Pid(group),
                    rustix::process::WaitIdOptions::EXITED
                        | rustix::process::WaitIdOptions::NOHANG
                        | rustix::process::WaitIdOptions::NOWAIT,
                ) {
                    Ok(Some(_)) => break,
                    Ok(None) | Err(rustix::io::Errno::INTR) => {
                        tokio::time::sleep(Duration::from_millis(10)).await
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Ok(bytes)
        };
        let result = tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(seconds), operation) => match result {
                Ok(result) => result,
                Err(_) => refuse("external tool deadline exceeded"),
            },
            _ = &mut stopped => refuse("external tool cancelled"),
        };
        let killed = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        if let Err(error) = killed
            && error != rustix::io::Errno::SRCH
        {
            return Err(error.into());
        }
        // Disarm only AFTER signalling the still-pinned group, BEFORE reaping its
        // leader. Drop must never signal a numeric PGID after its identity is free.
        tool.group = None;
        let status = tool.child.wait().await?;
        drop(tool);
        if result.is_ok() && !status.success() {
            return refuse("external verification/download tool failed; no installation performed");
        }
        result
    });
    let _cleanup_request = stop;
    owner
        .await
        .map_err(|_| Error::Refused("external tool cleanup owner failed"))?
}

fn sealed(bytes: &[u8], cancel: &super::Cancel) -> Result<(File, PathBuf)> {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    let descriptor = memfd_create(
        "kk-attestation-input",
        MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
    )?;
    let mut file = File::from(descriptor);
    for chunk in bytes.chunks(65536) {
        cancel.check()?;
        file.write_all(chunk)?;
    }
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::SHRINK | SealFlags::GROW | SealFlags::SEAL,
    )?;
    // gh opens the parent's sealed descriptor, not an inherited descriptor or a
    // replaceable pathname. Missing procfs/sealing support is a hard error.
    let path = PathBuf::from(format!(
        "/proc/{}/fd/{}",
        std::process::id(),
        file.as_raw_fd()
    ));
    Ok((file, path))
}

// gh selects the bundle decoder by suffix. Only a private name is on disk;
// its target remains the kernel-sealed descriptor, never a mutable byte copy.
struct BundleView {
    // Drop the temporary name while its procfs parent descriptor is still held.
    _temporary: tempfile::TempDir,
    _directory: File,
    _parent: File,
    path: PathBuf,
}

impl BundleView {
    fn new(frozen: &Path, original: &Path) -> Result<Self> {
        use rustix::fs::{Mode, OFlags, open, openat, symlinkat};
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let parent = File::from(open(std::env::temp_dir(), flags, Mode::empty()).map_err(
            |error| {
                let error = std::io::Error::from(error);
                std::io::Error::new(
                    error.kind(),
                    format!("opening verification temporary parent: {error}"),
                )
            },
        )?);
        let metadata = parent.metadata()?;
        let uid = rustix::process::geteuid().as_raw();
        if (metadata.uid() != 0 && metadata.uid() != uid)
            || (metadata.mode() & 0o022 != 0
                && !(metadata.uid() == 0 && metadata.mode() & 0o1000 != 0))
        {
            return refuse("unsafe verification temporary parent");
        }
        let parent_path = PathBuf::from(format!(
            "/proc/{}/fd/{}",
            std::process::id(),
            parent.as_raw_fd()
        ));
        let temporary = tempfile::Builder::new()
            .prefix("kk-verification-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(parent_path)
            .map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    format!("creating private verification name: {error}"),
                )
            })?;
        let name = temporary.path().file_name().ok_or(Error::Refused(
            "missing private verification directory name",
        ))?;
        let directory = File::from(openat(&parent, name, flags, Mode::empty()).map_err(
            |error| {
                let error = std::io::Error::from(error);
                std::io::Error::new(
                    error.kind(),
                    format!("opening private verification directory: {error}"),
                )
            },
        )?);
        let metadata = directory.metadata()?;
        if metadata.uid() != uid || metadata.mode() & 0o777 != 0o700 {
            return refuse("unsafe private verification directory");
        }
        let leaf = if original
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            "bundle.json"
        } else {
            "bundle.jsonl"
        };
        symlinkat(frozen, &directory, leaf).map_err(|error| {
            let error = std::io::Error::from(error);
            std::io::Error::new(
                error.kind(),
                format!("naming sealed verification input: {error}"),
            )
        })?;
        let path = PathBuf::from(format!(
            "/proc/{}/fd/{}/{leaf}",
            std::process::id(),
            directory.as_raw_fd()
        ));
        Ok(Self {
            _temporary: temporary,
            _directory: directory,
            _parent: parent,
            path,
        })
    }
}

pub(super) async fn authenticate(
    request: &InstallRequest,
    target: &str,
    cancel: super::Cancel,
) -> Result<Vec<u8>> {
    verifier(request).await?;
    // The filename convention is release.py's existing Linux release convention.
    // It selects a candidate only; the authenticated manifest must match the host.
    let bytes = if let Some(path) = &request.archive {
        let path = path.clone();
        let cancel = cancel.clone();
        super::worker(move || snapshot(&path, MAX_BYTES, &cancel)).await?
    } else {
        let cpu = target
            .split('-')
            .next()
            .ok_or(Error::Refused("invalid host target"))?;
        let url = format!(
            "https://github.com/kpihx-labs/kk/releases/download/v{0}/kk-v{0}-linux-{cpu}.tar.gz",
            request.version
        );
        tokio::time::timeout(Duration::from_secs(125), download(url, &cancel))
            .await
            .map_err(|_| Error::Refused("archive download deadline exceeded"))??
    };
    verify_snapshot(request, bytes, cancel).await
}

const REDIRECT_BYTES: usize = 8192;

fn allowed_download_url(url: &str) -> bool {
    url.len() <= REDIRECT_BYTES
        && url.bytes().all(|byte| byte.is_ascii_graphic())
        && [
            "https://github.com/",
            "https://release-assets.githubusercontent.com/",
            "https://objects.githubusercontent.com/",
        ]
        .iter()
        .any(|prefix| url.starts_with(prefix))
}

// The trailer is emitted by curl AFTER the binary body. Split from the end,
// never search for a marker inside attacker-controlled archive bytes.
fn download_response(bytes: &[u8]) -> Result<(&[u8], &[u8], &str)> {
    let mut fields = bytes.rsplitn(4, |byte| *byte == b'\n');
    if fields.next() != Some(b"".as_slice()) {
        return refuse("invalid archive response trailer");
    }
    let redirect = fields
        .next()
        .filter(|value| value.len() <= REDIRECT_BYTES)
        .and_then(|value| std::str::from_utf8(value).ok())
        .ok_or(Error::Refused("invalid archive redirect metadata"))?;
    let status = fields
        .next()
        .filter(|value| value.len() == 3 && value.iter().all(u8::is_ascii_digit))
        .ok_or(Error::Refused("invalid archive response status"))?;
    let body = fields
        .next()
        .filter(|value| value.len() <= MAX_BYTES)
        .ok_or(Error::Refused(
            "archive response body exceeds resource budget",
        ))?;
    Ok((body, status, redirect))
}

async fn download(mut url: String, cancel: &super::Cancel) -> Result<Vec<u8>> {
    for hop in 0..=5 {
        cancel.check()?;
        if !allowed_download_url(&url) {
            return refuse("refused archive redirect authority or URL");
        }
        let mut curl = Command::new("curl");
        // No --location: validate each resolved next URL before connecting.
        // Keep signed redirect URLs out of stderr and error messages.
        curl.args([
            "--disable",
            "--silent",
            "--max-redirs",
            "0",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "15",
            "--max-time",
            "120",
            "--max-filesize",
            &MAX_BYTES.to_string(),
            "--write-out",
            "\n%{http_code}\n%{redirect_url}\n",
            "--url",
            &url,
        ]);
        let response = run(curl, MAX_BYTES + REDIRECT_BYTES + 6, 125).await?;
        let (body, status, redirect) = download_response(&response)?;
        match status {
            b"200" if redirect.is_empty() => return Ok(body.to_vec()),
            b"301" | b"302" | b"303" | b"307" | b"308" if hop < 5 => {
                if !allowed_download_url(redirect) {
                    return refuse("refused archive redirect authority or URL");
                }
                url = redirect.to_owned();
            }
            _ => return refuse("unexpected archive HTTP status or redirect limit"),
        }
    }
    refuse("archive redirect limit exceeded")
}

async fn verifier(request: &InstallRequest) -> Result<()> {
    if request.trusted_root.is_some() && request.bundle.is_none() {
        return refuse("trusted root requires a local attestation bundle");
    }
    let mut gh = Command::new("gh");
    gh.args(["attestation", "verify", "--help"])
        .env("GH_HOST", "github.com")
        .env_remove("GH_DEBUG");
    let help = run(gh, 131072, 10).await?;
    let help =
        std::str::from_utf8(&help).map_err(|_| Error::Refused("invalid gh capability response"))?;
    for flag in [
        "--repo",
        "--signer-workflow",
        "--source-ref",
        "--source-digest",
        "--deny-self-hosted-runners",
    ] {
        if !help.contains(flag) {
            return refuse(
                "gh with required attestation policy flags must be provisioned separately",
            );
        }
    }
    if request.bundle.is_some() && !help.contains("--bundle") {
        return refuse("gh lacks offline bundle verification support");
    }
    if request.trusted_root.is_some() && !help.contains("--custom-trusted-root") {
        return refuse("gh lacks explicit trusted-root verification support");
    }
    Ok(())
}

pub(super) async fn authenticate_bytes(
    request: &InstallRequest,
    bytes: Vec<u8>,
    cancel: super::Cancel,
) -> Result<Vec<u8>> {
    verifier(request).await?;
    verify_snapshot(request, bytes, cancel).await
}

async fn verify_snapshot(
    request: &InstallRequest,
    bytes: Vec<u8>,
    cancel: super::Cancel,
) -> Result<Vec<u8>> {
    if bytes.len() > MAX_BYTES {
        return refuse("compressed archive is oversized");
    }
    // Never pass the original USB/download pathname to gh and then reopen it.
    // Kernel seals make gh's input immutable even through another writable fd.
    let sealing_cancel = cancel.clone();
    let (bytes, frozen, path) = super::worker(move || {
        let (file, path) = sealed(&bytes, &sealing_cancel)?;
        Ok((bytes, file, path))
    })
    .await?;
    let mut gh = Command::new("gh");
    gh.env("GH_HOST", "github.com").env_remove("GH_DEBUG");
    gh.arg("attestation").arg("verify").arg(&path).args([
        "--repo",
        "kpihx-labs/kk",
        "--signer-workflow",
        "kpihx-labs/kk/.github/workflows/check.yml",
        "--source-ref",
        &format!("refs/tags/v{}", request.version),
        "--source-digest",
        &request.commit,
        "--deny-self-hosted-runners",
    ]);
    let mut verification_inputs = Vec::with_capacity(2);
    for (flag, path, limit) in [
        ("--bundle", request.bundle.as_ref(), 16 * 1024 * 1024),
        (
            "--custom-trusted-root",
            request.trusted_root.as_ref(),
            1024 * 1024,
        ),
    ] {
        let Some(path) = path else { continue };
        let path = path.clone();
        let cancel = cancel.clone();
        let (file, input_path, view) = super::worker(move || {
            let data = snapshot(&path, limit, &cancel)?;
            let (file, frozen_path) = sealed(&data, &cancel)?;
            let view = if flag == "--bundle" {
                Some(BundleView::new(&frozen_path, &path)?)
            } else {
                None
            };
            let input_path = view.as_ref().map_or(frozen_path, |view| view.path.clone());
            Ok((file, input_path, view))
        })
        .await?;
        verification_inputs.push((file, view));
        gh.arg(flag).arg(input_path);
    }
    cancel.check()?;
    run(gh, 1024 * 1024, 120).await?;
    drop((frozen, verification_inputs));
    Ok(bytes)
}

pub(super) async fn check_abi(archive: &Verified, cancel: super::Cancel) -> Result<()> {
    let binary = archive
        .files
        .get("kk")
        .ok_or(Error::Refused("missing executable"))?
        .clone();
    let target = archive.manifest.target.clone();
    let inspect_target = target.clone();
    let candidate =
        super::worker(move || super::elf::inspect(&binary, &inspect_target, true)).await?;
    if candidate.glibc != archive.manifest.binary_glibc_required {
        return refuse("manifest libc requirement differs from executable version needs");
    }
    if candidate.needed.is_empty() {
        return Ok(());
    }
    let mut command = Command::new("ldconfig");
    command.arg("-p");
    let cache = run(command, 4 * 1024 * 1024, 10).await?;
    super::worker(move || check_libraries(candidate, &target, &cache, &cancel)).await
}

fn check_libraries(
    candidate: super::elf::Requirements,
    target: &str,
    cache: &[u8],
    cancel: &super::Cancel,
) -> Result<()> {
    use std::collections::{BTreeMap, BTreeSet, VecDeque};
    use std::os::unix::fs::MetadataExt;
    let text =
        std::str::from_utf8(cache).map_err(|_| Error::Refused("invalid system loader cache"))?;
    let mut paths: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        cancel.check()?;
        if index > 65536 {
            return refuse("system loader cache exceeds budget");
        }
        if let Some((left, path)) = line.split_once("=>")
            && let Some(name) = left.split_whitespace().next()
        {
            let list = paths.entry(name.to_owned()).or_default();
            if list.len() >= 32 {
                return refuse("ambiguous system library cache");
            }
            list.push(PathBuf::from(path.trim()));
        }
    }
    let mut pending = VecDeque::from([candidate]);
    let mut loaded: BTreeMap<String, super::elf::Requirements> = BTreeMap::new();
    let mut checked = BTreeSet::new();
    let mut total = 0usize;
    while let Some(requirements) = pending.pop_front() {
        cancel.check()?;
        for name in &requirements.needed {
            if ![
                "libc.so.6",
                "libm.so.6",
                "libgcc_s.so.1",
                "libpthread.so.0",
                "libdl.so.2",
                "librt.so.1",
                "libutil.so.1",
                "ld-linux-x86-64.so.2",
                "ld-linux-aarch64.so.1",
            ]
            .contains(&name.as_str())
            {
                return refuse("unsupported dynamic library requirement");
            }
            if !loaded.contains_key(name) {
                let mut found = None;
                for path in paths.get(name).into_iter().flatten() {
                    if !path.is_absolute() {
                        continue;
                    }
                    let mut file = match File::options()
                        .read(true)
                        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
                        .open(path)
                    {
                        Ok(file) => file,
                        Err(_) => continue,
                    };
                    let stat = file.metadata()?;
                    if !stat.is_file()
                        || stat.uid() != 0
                        || stat.mode() & 0o022 != 0
                        || stat.len() > 128 * 1024 * 1024
                    {
                        continue;
                    }
                    let mut bytes = Vec::new();
                    let mut input = Read::by_ref(&mut file).take(128 * 1024 * 1024 + 1);
                    let mut chunk = [0u8; 65536];
                    loop {
                        cancel.check()?;
                        let count = input.read(&mut chunk)?;
                        if count == 0 {
                            break;
                        }
                        bytes.extend_from_slice(&chunk[..count]);
                    }
                    total = total
                        .checked_add(bytes.len())
                        .ok_or(Error::Refused("system ABI budget overflow"))?;
                    if bytes.len() > 128 * 1024 * 1024 || total > MAX_BYTES {
                        return refuse("system ABI budget exceeded");
                    }
                    if let Ok(metadata) = super::elf::inspect(&bytes, target, false) {
                        found = Some(metadata);
                        break;
                    }
                }
                loaded.insert(
                    name.clone(),
                    found.ok_or(Error::Refused(
                        "required host library is missing or has incompatible ELF metadata",
                    ))?,
                );
            }
            let library = loaded.get(name).expect("library inserted above");
            if requirements
                .required
                .get(name)
                .is_some_and(|versions| !versions.is_subset(&library.provided))
            {
                return refuse("host library lacks a required ELF symbol version");
            }
            if checked.insert(name.clone()) {
                if checked.len() > 64 {
                    return refuse("too many system ABI dependencies");
                }
                pending.push_back(super::elf::Requirements {
                    needed: library.needed.clone(),
                    required: library.required.clone(),
                    provided: BTreeSet::new(),
                    glibc: None,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_authorities_are_exact_and_trailers_preserve_binary_bodies() {
        for url in [
            "https://github.com/asset",
            "https://objects.githubusercontent.com/asset",
            "https://release-assets.githubusercontent.com/asset?signature=opaque",
        ] {
            assert!(allowed_download_url(url));
        }
        for url in [
            "https://github.com:443/asset",
            "https://user@github.com/asset",
            "https://github.com.evil/asset",
            "http://github.com/asset",
            "https://127.0.0.1/",
            "https://github.com/\nasset",
            "https://github.com/asset with spaces",
            "https://github.com",
        ] {
            assert!(!allowed_download_url(url));
        }
        let body = b"\x00\xff\n302\nhttps://evil.example/\n\n200\n\n";
        let mut response = body.to_vec();
        response.extend_from_slice(b"\n200\n\n");
        let (actual, status, redirect) = download_response(&response).unwrap();
        assert_eq!(actual, body);
        assert_eq!(status, b"200");
        assert_eq!(redirect, "");
        for invalid in [b"body".as_slice(), b"body\n200\n", b"body\n20\n\n"] {
            assert!(download_response(invalid).is_err());
        }
        let mut response = b"body\n302\n".to_vec();
        response.extend(std::iter::repeat_n(b'x', REDIRECT_BYTES + 1));
        response.push(b'\n');
        assert!(download_response(&response).is_err());
    }

    fn state(pid: u32) -> Option<char> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        text.rsplit_once(')')?.1.trim_start().chars().next()
    }

    async fn descendants(mode: &'static str) {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("mock tool.sh");
        let pids = directory.path().join("pids");
        std::fs::write(&script, "#!/bin/sh\n/bin/sleep 60 </dev/null >/dev/null &\nprintf '%s %s\\n' \"$$\" \"$!\" > \"$1\"\ncase \"$2\" in normal) exit 0;; overflow) while :; do printf '0123456789012345678901234567890123456789'; done;; *) wait;; esac\n").unwrap();
        let mut command = Command::new("/bin/sh");
        command.arg(script).arg(&pids).arg(mode);
        let task = tokio::spawn(run(command, 1024, if mode == "timeout" { 1 } else { 10 }));
        let ids = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(text) = std::fs::read_to_string(&pids) {
                    let ids = text
                        .split_whitespace()
                        .map(str::parse::<u32>)
                        .collect::<std::result::Result<Vec<_>, _>>()
                        .unwrap();
                    if ids.len() == 2 {
                        break ids;
                    }
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        if mode == "cancel" {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            let result = task.await.unwrap();
            assert_eq!(result.is_ok(), mode == "normal");
        }
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                // PID1 may briefly retain an orphaned descendant zombie. The
                // direct child MUST be reaped by the cleanup owner, not PID1.
                if state(ids[0]).is_none() && state(ids[1]).is_none_or(|s| s == 'Z') {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("owned process group survived bounded cleanup");
    }

    #[tokio::test]
    async fn timeout_kills_descendants_and_reaps_leader() {
        descendants("timeout").await;
    }
    #[tokio::test]
    async fn output_overflow_kills_descendants_and_reaps_leader() {
        descendants("overflow").await;
    }
    #[tokio::test]
    async fn future_cancellation_keeps_cleanup_owner_alive() {
        descendants("cancel").await;
    }
    #[tokio::test]
    async fn success_also_closes_owned_descendants_before_reaping() {
        descendants("normal").await;
    }

    #[test]
    fn bundle_names_keep_decoder_suffix_seals_and_directory_binding() {
        use std::os::unix::fs::MetadataExt;
        let cancel = super::super::Cancel::default();
        let (_file, frozen) = sealed(b"public bundle fixture", &cancel).unwrap();
        for (original, extension) in [
            ("input.json", "json"),
            ("input.jsonl", "jsonl"),
            ("input", "jsonl"),
        ] {
            let view = BundleView::new(&frozen, Path::new(original)).unwrap();
            assert_eq!(view.path.extension().unwrap(), extension);
            assert_eq!(view._directory.metadata().unwrap().mode() & 0o777, 0o700);
            assert_eq!(std::fs::read(&view.path).unwrap(), b"public bundle fixture");
            assert!(std::fs::write(&view.path, b"replacement").is_err());
            let cleanup = std::fs::canonicalize(view._temporary.path()).unwrap();
            let moved = view._temporary.path().with_extension("moved");
            assert!(!moved.exists());
            std::fs::rename(view._temporary.path(), &moved).unwrap();
            assert_eq!(std::fs::read(&view.path).unwrap(), b"public bundle fixture");
            std::fs::rename(&moved, view._temporary.path()).unwrap();
            drop(view);
            assert!(!cleanup.exists());
        }
    }

    #[test]
    fn provenance_input_is_kernel_sealed() {
        let (mut file, path) =
            sealed(b"immutable bytes", &super::super::Cancel::default()).unwrap();
        assert!(file.write_all(b"replacement").is_err());
        assert!(file.set_len(0).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"immutable bytes");
    }
}
