//! Real transaction/archive tests with MOCK gh/curl, NOT provenance qualification.
//! The module path permits testing before Main wires the public library export.
#![cfg(target_os = "linux")]
#[path = "../src/install.rs"]
mod install;

use flate2::{Compression, write::GzEncoder};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
};

fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn tar(entries: &[(String, Vec<u8>, u8)]) -> Vec<u8> {
    let mut archive = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for (name, bytes, kind) in entries {
        let mut header = tar::Header::new_ustar();
        assert!(name.len() < 100);
        header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
        header.set_mode(0o644);
        header.set_size(bytes.len() as u64);
        header.set_entry_type(tar::EntryType::new(*kind));
        header.set_cksum();
        archive.append(&header, bytes.as_slice()).unwrap();
    }
    archive.into_inner().unwrap().finish().unwrap()
}

fn bundle(version: &str, marker: &[u8], zip: bool) -> Vec<u8> {
    // Actual host ELF bytes, never executed by the installer. Append a harmless
    // marker to distinguish versions without inventing fake executable evidence.
    let mut binary = fs::read("/bin/true").unwrap();
    if marker == b"missing-library" || marker == b"too-new" {
        let (from, to) = if marker == b"missing-library" {
            (b"libc.so.6".as_slice(), b"libx.so.6".as_slice())
        } else {
            (b"GLIBC_2.34".as_slice(), b"GLIBC_9.99".as_slice())
        };
        for offset in 0..=binary.len() - from.len() {
            if &binary[offset..offset + from.len()] == from {
                binary[offset..offset + from.len()].copy_from_slice(to);
            }
        }
    }
    binary.extend_from_slice(marker);
    let required = binary
        .split(|b| *b == 0)
        .filter_map(|s| std::str::from_utf8(s).ok()?.strip_prefix("GLIBC_"))
        .filter_map(|s| {
            let mut parts = s
                .split('.')
                .map(str::parse::<u32>)
                .collect::<std::result::Result<Vec<_>, _>>()
                .ok()?;
            if !(2..=3).contains(&parts.len()) {
                return None;
            }
            parts.resize(3, 0);
            Some((parts, s))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, s)| s.to_owned());
    let files = BTreeMap::from([("kk", hash(&binary)), ("LICENSE", hash(b"test fixture"))]);
    let manifest = serde_json::to_vec(&serde_json::json!({
        "format":1, "version":version, "commit":"a".repeat(40),
        "target":install::host_target().unwrap(), "binary_glibc_required":if marker == b"manifest-lie" {Some("2.0".to_owned())} else {required}, "files":files
    }))
    .unwrap();
    let entries = vec![
        ("root/kk".into(), binary, b'0'),
        ("root/LICENSE".into(), b"test fixture".to_vec(), b'0'),
        ("root/build-manifest.json".into(), manifest, b'0'),
    ];
    if !zip {
        return tar(&entries);
    }
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes, _) in entries {
        archive
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .unix_permissions(0o644),
            )
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

fn executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn scenario(name: &str) {
    let temp = tempfile::Builder::new()
        .prefix("kk real transaction é ")
        // Do not inherit a group-writable TMPDIR ancestor: the production
        // transaction correctly refuses such a prefix. Linux /tmp is sticky.
        .tempdir_in("/tmp")
        .unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let tools = temp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    executable(
        &tools.join("gh"),
        r#"#!/bin/sh
# MOCK: byte comparison is not provenance verification.
if [ "${3:-}" = --help ]; then
    printf '%s\n' '--repo --signer-workflow --source-ref --source-digest --deny-self-hosted-runners --bundle'
    [ "$KK_INSTALL_SCENARIO" = offline_root_incapable ] || printf '%s\n' --custom-trusted-root
    exit 0
fi
printf '%s\n' "$@" > "$KK_GH_LOG"
[ "${KK_GH_FAIL:-0}" = 0 ] || exit 1
[ "$1" = attestation ] && [ "$2" = verify ] || exit 2
cmp -- "$3" "$KK_EXPECTED" || exit 3
previous=''
for argument do
    if [ "$previous" = --bundle ]; then
        case "$argument" in *.json|*.jsonl) :;; *) exit 5;; esac
    fi
    if [ "$previous" = --custom-trusted-root ]; then
        cmp -- "$argument" "$KK_INSTALL_ROOT/trusted root é.jsonl" || exit 4
    fi
    previous=$argument
done
if [ "${KK_REPLACE_LOCAL:-0}" = 1 ]; then printf 'replaced pathname' > "$KK_EXPECTED"; fi
"#,
    );
    executable(
        &tools.join("curl"),
        r#"#!/bin/sh
# MOCK public download transport, not network evidence.
printf '%s\n' "$@" >> "$KK_CURL_LOG"
for url do :; done
case "$KK_INSTALL_SCENARIO" in
    redirect_allowed)
        case "$url" in https://github.com/*) printf '\n302\nhttps://release-assets.githubusercontent.com/asset?signature=private\n'; exit 0;; esac ;;
    redirect_forbidden) printf '\n302\nhttps://127.0.0.1/private\n'; exit 0 ;;
    redirect_port) printf '\n302\nhttps://github.com:443/asset\n'; exit 0 ;;
    redirect_userinfo) printf '\n302\nhttps://user@github.com/asset\n'; exit 0 ;;
    redirect_downgrade) printf '\n302\nhttp://github.com/asset\n'; exit 0 ;;
    redirect_loop) printf '\n302\nhttps://github.com/loop\n'; exit 0 ;;
    redirect_empty) printf '\n302\n\n'; exit 0 ;;
    redirect_status) printf '\n404\n\n'; exit 0 ;;
esac
cat -- "$KK_EXPECTED"
printf '\n200\n\n'
"#,
    );
    if name == "abi_missing_cache" {
        executable(
            &tools.join("ldconfig"),
            "#!/bin/sh\n# MOCK empty system-library cache.\nexit 0\n",
        );
    }
    let result = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "transaction_child", "--nocapture"])
        .env("KK_INSTALL_SCENARIO", name)
        .env("KK_INSTALL_ROOT", temp.path())
        .env("KK_GH_LOG", temp.path().join("gh.log"))
        .env("KK_CURL_LOG", temp.path().join("curl.log"))
        .env("KK_EXPECTED", temp.path().join("archive.zip"))
        .env("KK_GH_FAIL", if name == "gh_failure" { "1" } else { "0" })
        .env(
            "KK_REPLACE_LOCAL",
            if name == "snapshot" { "1" } else { "0" },
        )
        .env(
            "PATH",
            format!(
                "{}:{}",
                tools.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .unwrap();
    // chmod immutable fixture directories back only for this test's temporary tree.
    // Production installation never cleans up a previous version or failed stage.
    fn writable(path: &Path) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if fs::symlink_metadata(&path).unwrap().is_dir() {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
                writable(&path);
            }
        }
    }
    writable(temp.path());
    assert!(
        result.status.success(),
        "scenario {name}: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

macro_rules! cases {
    ($($name:ident),* $(,)?) => { $(#[test] fn $name() { scenario(stringify!($name)); })* };
}
cases!(
    tar_install,
    zip_install,
    gh_failure,
    snapshot,
    online,
    redirect_allowed,
    redirect_forbidden,
    redirect_port,
    redirect_userinfo,
    redirect_downgrade,
    redirect_loop,
    redirect_empty,
    redirect_status,
    offline_bundle,
    offline_trusted_root,
    offline_trusted_root_verify_only,
    offline_trusted_root_rollback,
    offline_root_without_bundle,
    offline_root_relative,
    offline_root_symlink,
    offline_root_oversized,
    offline_root_incapable,
    foreign_binary,
    foreign_link,
    foreign_managed_link,
    version_conflict,
    tampered_previous,
    rollback_bytes,
    crash_staging,
    resume_interrupted_publication,
    ancestor_symlink,
    hostile_traversal,
    hostile_link,
    hostile_duplicate,
    hostile_case,
    hostile_extra,
    hostile_digest,
    hostile_target,
    hostile_commit,
    hostile_zip_directory,
    compressed_budget,
    selector_validation,
    abi_too_new,
    abi_missing_library,
    abi_missing_cache,
    abi_manifest_lie,
    coherent_conflict,
    coherent_rollback,
    cooperating_replacement_race,
    archive_conflict,
    idempotent_install
);

fn request(root: &Path) -> install::InstallRequest {
    install::InstallRequest {
        version: "0.8.2".into(),
        commit: "a".repeat(40),
        prefix: root.join("prefix space é"),
        archive: Some(root.join("archive.zip")),
        bundle: None,
        trusted_root: None,
    }
}

#[test]
fn transaction_child() {
    let Ok(case) = std::env::var("KK_INSTALL_SCENARIO") else {
        return;
    };
    let root = PathBuf::from(std::env::var_os("KK_INSTALL_ROOT").unwrap());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut req = request(&root);
    let archive_path = root.join("archive.zip");
    let original = bundle("0.8.2", b"first", case == "zip_install");
    fs::write(&archive_path, &original).unwrap();
    if case.starts_with("abi_") {
        let marker = match case.as_str() {
            "abi_too_new" => b"too-new".as_slice(),
            "abi_missing_library" => b"missing-library".as_slice(),
            "abi_missing_cache" => b"first".as_slice(),
            _ => b"manifest-lie".as_slice(),
        };
        fs::write(&archive_path, bundle("0.8.2", marker, false)).unwrap();
        let error = runtime
            .block_on(install::install(req.clone()))
            .unwrap_err()
            .to_string();
        let expected = match case.as_str() {
            "abi_too_new" => "lacks a required ELF symbol version",
            "abi_missing_library" => "unsupported dynamic library",
            "abi_missing_cache" => "required host library is missing",
            _ => "manifest libc requirement differs",
        };
        assert!(error.contains(expected), "{error}");
        assert!(!req.prefix.exists());
        return;
    }
    if case == "gh_failure" {
        assert!(runtime.block_on(install::install(req.clone())).is_err());
        assert!(!req.prefix.exists());
        return;
    }
    if case == "ancestor_symlink" {
        let target = root.join("foreign");
        fs::create_dir(&target).unwrap();
        symlink(&target, &req.prefix).unwrap();
        assert!(runtime.block_on(install::install(req)).is_err());
        assert_eq!(fs::read_dir(target).unwrap().count(), 0);
        return;
    }
    if case.starts_with("foreign_") {
        fs::create_dir_all(req.prefix.join("bin")).unwrap();
        let destination = req.prefix.join("bin/kk");
        match case.as_str() {
            "foreign_binary" => fs::write(&destination, b"foreign executable").unwrap(),
            "foreign_link" => symlink(root.join("unrelated"), &destination).unwrap(),
            _ => {
                let foreign = req.prefix.join("lib/kk/0.8.2");
                fs::create_dir_all(&foreign).unwrap();
                fs::write(foreign.join("kk"), b"foreign managed-looking executable").unwrap();
                symlink("../lib/kk/0.8.2/kk", &destination).unwrap();
            }
        }
        let before = fs::symlink_metadata(&destination).unwrap();
        for path in [
            &req.prefix,
            &req.prefix.join("bin"),
            &req.prefix.join("lib"),
            &req.prefix.join("lib/kk"),
        ] {
            if path.is_dir() {
                fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
        assert!(runtime.block_on(install::install(req)).is_err());
        assert_eq!(
            before.file_type(),
            fs::symlink_metadata(&destination).unwrap().file_type()
        );
        return;
    }
    if case.starts_with("hostile_") {
        let mut entries = Vec::new();
        let mut decoder = flate2::read::GzDecoder::new(original.as_slice());
        let mut unpacked = Vec::new();
        decoder.read_to_end(&mut unpacked).unwrap();
        for entry in tar::Archive::new(unpacked.as_slice()).entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_str().unwrap().to_owned();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            entries.push((name, data, b'0'));
        }
        match case.as_str() {
            "hostile_traversal" => entries[0].0 = "root/../escaped".into(),
            "hostile_link" => entries[0].2 = b'2',
            "hostile_duplicate" => entries.push(entries[0].clone()),
            "hostile_case" => {
                let mut other = entries[0].clone();
                other.0 = other.0.to_uppercase();
                entries.push(other);
            }
            "hostile_extra" => entries.push(("root/extra".into(), b"extra".to_vec(), b'0')),
            "hostile_digest" => entries[0].1.push(0),
            "hostile_target" | "hostile_commit" => {
                let manifest = entries
                    .iter_mut()
                    .find(|e| e.0.ends_with("build-manifest.json"))
                    .unwrap();
                let mut json: serde_json::Value = serde_json::from_slice(&manifest.1).unwrap();
                json[if case == "hostile_target" {
                    "target"
                } else {
                    "commit"
                }] = "foreign".into();
                manifest.1 = serde_json::to_vec(&json).unwrap();
            }
            "hostile_zip_directory" => {}
            _ => unreachable!(),
        }
        fs::write(
            &archive_path,
            if case == "hostile_zip_directory" {
                b"PK\x05\x06".to_vec()
            } else {
                tar(&entries)
            },
        )
        .unwrap();
        assert!(runtime.block_on(install::install(req.clone())).is_err());
        assert!(!req.prefix.exists(), "hostile archive wrote managed state");
        assert!(!root.join("escaped").exists());
        return;
    }
    if case == "compressed_budget" {
        fs::File::create(&archive_path)
            .unwrap()
            .set_len(256 * 1024 * 1024 + 1)
            .unwrap();
        assert!(runtime.block_on(install::install(req.clone())).is_err());
        assert!(!root.join("gh.log").exists());
        return;
    }
    if case == "selector_validation" {
        req.version = "../../escape".into();
        assert!(runtime.block_on(install::install(req)).is_err());
        assert!(!root.join("gh.log").exists());
        return;
    }
    if case == "online" || case.starts_with("redirect_") {
        req.archive = None;
    }
    if case.starts_with("redirect_") && case != "redirect_allowed" {
        let error = runtime.block_on(install::install(req.clone())).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("redirect") || message.contains("HTTP"),
            "{message}"
        );
        assert!(!message.contains("signature=private"));
        assert!(!req.prefix.exists());
        assert!(!root.join("gh.log").exists());
        let log = fs::read_to_string(root.join("curl.log")).unwrap();
        let calls = log.lines().filter(|line| *line == "--disable").count();
        assert_eq!(calls, if case == "redirect_loop" { 6 } else { 1 });
        assert!(!log.contains("--location"));
        assert!(!log.contains("https://127.0.0.1/private"));
        assert!(!log.contains("https://github.com:443/asset"));
        assert!(!log.contains("https://user@github.com/asset"));
        assert!(!log.contains("http://github.com/asset"));
        return;
    }
    if case == "offline_bundle"
        || case.starts_with("offline_root_")
        || case.starts_with("offline_trusted_root")
    {
        let bundle = root.join("attestation bundle é.jsonl");
        fs::write(&bundle, b"mock bundle").unwrap();
        req.bundle = Some(bundle);
    }
    if case.starts_with("offline_root_") || case.starts_with("offline_trusted_root") {
        let root_path = root.join("trusted root é.jsonl");
        fs::write(&root_path, b"mock separately trusted public root").unwrap();
        req.trusted_root = Some(root_path.clone());
        match case.as_str() {
            "offline_root_without_bundle" => req.bundle = None,
            "offline_root_relative" => req.trusted_root = Some("relative root.jsonl".into()),
            "offline_root_symlink" => {
                let alias = root.join("root alias.jsonl");
                symlink(&root_path, &alias).unwrap();
                req.trusted_root = Some(alias);
            }
            "offline_root_oversized" => fs::File::options()
                .write(true)
                .open(&root_path)
                .unwrap()
                .set_len(1024 * 1024 + 1)
                .unwrap(),
            _ => {}
        }
        if case.starts_with("offline_root_") {
            assert!(runtime.block_on(install::install(req.clone())).is_err());
            assert!(!req.prefix.exists());
            assert!(!root.join("gh.log").exists());
            assert!(!root.join("curl.log").exists());
            return;
        }
    }
    if case == "offline_trusted_root_verify_only" {
        // Actual public CLI -> install::verify; the existing gh fixture compares
        // the frozen archive and roots. This is not genuine signature evidence.
        let profiles = root.join("never-created-profiles");
        for existing in [false, true] {
            if existing {
                fs::create_dir(&req.prefix).unwrap();
                fs::set_permissions(&req.prefix, fs::Permissions::from_mode(0o700)).unwrap();
                fs::write(req.prefix.join("untouched"), b"preserve unrelated bytes").unwrap();
            }
            let metadata = existing.then(|| fs::metadata(&req.prefix).unwrap());
            let output = Command::new(env!("CARGO_BIN_EXE_kk"))
                .env("XDG_DATA_HOME", &profiles)
                .args([
                    "install",
                    "--verify-only",
                    "--version",
                    &req.version,
                    "--commit",
                    &req.commit,
                    "--prefix",
                ])
                .arg(&req.prefix)
                .arg("--archive")
                .arg(req.archive.as_ref().unwrap())
                .arg("--bundle")
                .arg(req.bundle.as_ref().unwrap())
                .arg("--trusted-root")
                .arg(req.trusted_root.as_ref().unwrap())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(result["activated"], false);
            assert_eq!(result["version"], req.version);
            assert_eq!(result["commit"], req.commit);
            assert_eq!(result["target"], install::host_target().unwrap());
            assert_eq!(result["archive_sha256"], hash(&original));
            assert!(!profiles.exists());
            if let Some(before) = metadata {
                let after = fs::metadata(&req.prefix).unwrap();
                assert_eq!(after.permissions().mode(), before.permissions().mode());
                assert_eq!(after.modified().unwrap(), before.modified().unwrap());
                assert_eq!(
                    fs::read(req.prefix.join("untouched")).unwrap(),
                    b"preserve unrelated bytes"
                );
                assert_eq!(fs::read_dir(&req.prefix).unwrap().count(), 1);
            } else {
                assert!(
                    !req.prefix.exists(),
                    "verify-only created the selected prefix"
                );
            }
            let log = fs::read_to_string(root.join("gh.log")).unwrap();
            for selector in [
                "--bundle",
                "--custom-trusted-root",
                "--signer-workflow",
                "--source-ref",
                "refs/tags/v0.8.2",
                "--source-digest",
                &req.commit,
            ] {
                assert!(
                    log.lines().any(|line| line == selector),
                    "missing authenticated selector {selector}"
                );
            }
            assert!(
                !root.join("curl.log").exists(),
                "local verification downloaded an archive"
            );
        }
        return;
    }
    if case == "crash_staging" {
        let stage = req.prefix.join("lib/kk/.stage-interrupted");
        fs::create_dir_all(&stage).unwrap();
        fs::set_permissions(&req.prefix, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(req.prefix.join("lib"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(req.prefix.join("lib/kk"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(stage.join("partial"), b"previous interrupted stage").unwrap();
    }
    let first = runtime.block_on(install::install(req.clone())).unwrap();
    assert_eq!(first.version, "0.8.2");
    assert!(first.previous.is_none());
    assert_eq!(
        fs::read_link(&first.executable).unwrap(),
        Path::new("../lib/kk/0.8.2/kk")
    );
    let before = fs::read(&first.executable).unwrap();
    if case == "resume_interrupted_publication" {
        let second = bundle("0.8.3", b"second", false);
        fs::write(&archive_path, &second).unwrap();
        let mut next = req.clone();
        next.version = "0.8.3".into();
        let mut seed = next.clone();
        seed.prefix = root.join("seed");
        runtime.block_on(install::install(seed.clone())).unwrap();
        let pending = req.prefix.join("lib/kk/0.8.3");
        let seed_version = seed.prefix.join("lib/kk/0.8.3");
        fs::set_permissions(&seed_version, fs::Permissions::from_mode(0o700)).unwrap();
        fs::rename(seed_version, &pending).unwrap();
        let unrelated = req.prefix.join("lib/kk/.stage-unrelated");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("keep"), b"unrelated interrupted work").unwrap();
        runtime.block_on(install::verify(next.clone())).unwrap();
        assert_eq!(fs::read(&first.executable).unwrap(), before);
        assert_eq!(
            fs::metadata(&pending).unwrap().permissions().mode() & 0o7777,
            0o700
        );
        let binary = pending.join("kk");
        let complete = fs::read(&binary).unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&binary, b"incomplete staging").unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o555)).unwrap();
        assert!(runtime.block_on(install::install(next.clone())).is_err());
        assert_eq!(fs::read(&first.executable).unwrap(), before);
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&binary, &complete).unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o555)).unwrap();
        fs::write(&archive_path, bundle("0.8.3", b"different digest", false)).unwrap();
        assert!(runtime.block_on(install::install(next.clone())).is_err());
        assert_eq!(
            fs::metadata(&pending).unwrap().permissions().mode() & 0o7777,
            0o700
        );
        assert_eq!(fs::read(&first.executable).unwrap(), before);
        fs::write(&archive_path, &second).unwrap();
        let resumed = runtime.block_on(install::install(next)).unwrap();
        assert_eq!(resumed.previous.as_deref(), Some("0.8.2"));
        assert_eq!(fs::read(&resumed.executable).unwrap(), complete);
        assert_eq!(
            fs::metadata(&pending).unwrap().permissions().mode() & 0o7777,
            0o500
        );
        assert_eq!(
            fs::read(unrelated.join("keep")).unwrap(),
            b"unrelated interrupted work"
        );
        return;
    }
    if case == "offline_trusted_root_rollback" {
        let result = runtime
            .block_on(install::rollback_with_trust(
                req.prefix.clone(),
                req.version.clone(),
                req.bundle.clone(),
                req.trusted_root.clone(),
            ))
            .unwrap();
        assert_eq!(result.previous.as_deref(), Some("0.8.2"));
        assert_eq!(fs::read(&result.executable).unwrap(), before);
    }
    if case == "coherent_conflict" || case == "coherent_rollback" {
        let directory = req.prefix.join("lib/kk/0.8.2");
        let mut forged = before.clone();
        forged.extend_from_slice(b"coherent local replacement");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("build-manifest.json")).unwrap())
                .unwrap();
        manifest["files"]["kk"] = hash(&forged).into();
        let manifest = serde_json::to_vec(&manifest).unwrap();
        let mut receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join(".kk-receipt.json")).unwrap()).unwrap();
        receipt["files"]["kk"] = hash(&forged).into();
        receipt["files"]["build-manifest.json"] = hash(&manifest).into();
        for (name, bytes, mode) in [
            ("kk", forged, 0o555),
            ("build-manifest.json", manifest, 0o444),
            (
                ".kk-receipt.json",
                serde_json::to_vec(&receipt).unwrap(),
                0o444,
            ),
        ] {
            let file = directory.join(name);
            fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
            fs::write(&file, bytes).unwrap();
            fs::set_permissions(file, fs::Permissions::from_mode(mode)).unwrap();
        }
        let error = if case == "coherent_conflict" {
            runtime.block_on(install::install(req.clone())).unwrap_err()
        } else {
            runtime
                .block_on(install::rollback(req.prefix.clone(), "0.8.2".into()))
                .unwrap_err()
        };
        assert!(
            error.to_string().contains(if case == "coherent_conflict" {
                "same version"
            } else {
                "freshly authenticated"
            }),
            "{error}"
        );
        return;
    }
    if case == "cooperating_replacement_race" {
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(req.prefix.join("lib/kk/.install.lock"))
            .unwrap();
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive).unwrap();
        let replacement = root.join("foreign replacement");
        fs::write(&replacement, b"foreign under writer lock").unwrap();
        fs::rename(replacement, &first.executable).unwrap();
        let error = runtime.block_on(install::install(req.clone())).unwrap_err();
        assert!(error.to_string().contains("transaction lock"), "{error}");
        assert_eq!(
            fs::read(&first.executable).unwrap(),
            b"foreign under writer lock"
        );
        drop(lock);
        assert!(
            runtime
                .block_on(install::install(req.clone()))
                .unwrap_err()
                .to_string()
                .contains("foreign active")
        );
        return;
    }
    if case == "archive_conflict" {
        fs::write(&archive_path, bundle("0.8.2", b"first", true)).unwrap();
        assert!(
            runtime
                .block_on(install::install(req.clone()))
                .unwrap_err()
                .to_string()
                .contains("same version")
        );
    }
    if case == "idempotent_install" {
        assert_eq!(
            runtime
                .block_on(install::install(req.clone()))
                .unwrap()
                .previous
                .as_deref(),
            Some("0.8.2")
        );
    }
    let log = fs::read_to_string(root.join("gh.log")).unwrap();
    for required in [
        "--repo\nkpihx-labs/kk",
        "--signer-workflow\nkpihx-labs/kk/.github/workflows/check.yml",
        "--source-ref\nrefs/tags/v0.8.2",
        "--source-digest",
        "--deny-self-hosted-runners",
    ] {
        assert!(log.contains(required), "missing {required}");
    }
    if case == "online" || case == "redirect_allowed" {
        let log = fs::read_to_string(root.join("curl.log")).unwrap();
        assert!(!log.contains("--location"));
        assert!(log.contains("--max-redirs\n0"));
        assert_eq!(
            log.lines().filter(|line| *line == "--disable").count(),
            if case == "redirect_allowed" { 2 } else { 1 }
        );
        for flag in [
            "--proto\n=https",
            "--proto-redir\n=https",
            "--max-time",
            "--max-filesize",
            "https://github.com/kpihx-labs/kk/releases/download/v0.8.2/",
        ] {
            assert!(log.contains(flag));
        }
    }
    if case == "offline_bundle" || case.starts_with("offline_trusted_root") {
        assert!(log.contains("--bundle"));
    }
    if case.starts_with("offline_trusted_root") {
        assert!(log.contains("--custom-trusted-root\n/proc/"));
        assert!(
            !log.contains("trusted root é.jsonl"),
            "gh must read the sealed root snapshot, not its original pathname"
        );
        assert!(!root.join("curl.log").exists());
    }
    if case == "snapshot" {
        assert_eq!(fs::read(&archive_path).unwrap(), b"replaced pathname");
    }
    if case == "crash_staging" {
        assert!(
            req.prefix
                .join("lib/kk/.stage-interrupted/partial")
                .is_file()
        );
    }
    if case == "version_conflict" {
        fs::write(&archive_path, bundle("0.8.2", b"different", false)).unwrap();
        assert!(runtime.block_on(install::install(req.clone())).is_err());
        assert_eq!(fs::read(&first.executable).unwrap(), before);
    }
    if case == "tampered_previous" {
        let binary = req.prefix.join("lib/kk/0.8.2/kk");
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(&binary, b"tampered").unwrap();
        fs::write(&archive_path, bundle("0.8.3", b"second", false)).unwrap();
        req.version = "0.8.3".into();
        assert!(runtime.block_on(install::install(req.clone())).is_err());
        assert!(!req.prefix.join("lib/kk/0.8.3").exists());
    }
    if case == "rollback_bytes" {
        fs::write(&archive_path, bundle("0.8.3", b"second", true)).unwrap();
        req.version = "0.8.3".into();
        let second = runtime.block_on(install::install(req.clone())).unwrap();
        assert_eq!(second.previous.as_deref(), Some("0.8.2"));
        assert_ne!(fs::read(&second.executable).unwrap(), before);
        fs::write(&archive_path, &original).unwrap(); // MOCK gh expects original rollback bytes.
        let reverted = runtime
            .block_on(install::rollback(req.prefix.clone(), "0.8.2".into()))
            .unwrap();
        assert_eq!(reverted.previous.as_deref(), Some("0.8.3"));
        assert_eq!(fs::read(&reverted.executable).unwrap(), before);
        assert!(req.prefix.join("lib/kk/0.8.3/kk").is_file());
    }
}
