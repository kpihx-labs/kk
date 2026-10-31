#!/usr/bin/env -S uv run --script
"""Build and verify clean-source release archives; no signing keys are handled."""
import argparse
import gzip
import hashlib
import io
import json
import os
import platform
import re
import stat
import struct
import subprocess
import tarfile
import tempfile
import time
import tomllib
import zipfile
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {"x86_64-unknown-linux-gnu": "linux-x86_64", "aarch64-unknown-linux-gnu": "linux-aarch64",
           "x86_64-apple-darwin": "macos-x86_64", "aarch64-apple-darwin": "macos-aarch64"}
# Explicit platform/container contract; a future Windows helper is not an ELF rename.
HELPERS = {
    "x86_64-unknown-linux-gnu": {"platform": "linux", "architecture": "x86_64", "executable_format": "elf", "suffix": ".bin"},
    "aarch64-unknown-linux-gnu": {"platform": "linux", "architecture": "aarch64", "executable_format": "elf", "suffix": ".bin"},
    "x86_64-apple-darwin": {"platform": "macos", "architecture": "x86_64", "executable_format": "macho", "suffix": ".bin"},
    "aarch64-apple-darwin": {"platform": "macos", "architecture": "aarch64", "executable_format": "macho", "suffix": ".bin"},
}
MAX_MEMBERS = 4096
MAX_UNPACKED_BYTES = 256 * 1024 * 1024
MAX_MANIFEST_BYTES = 4 * 1024 * 1024
MAX_MEMBER_BYTES = 128 * 1024 * 1024
MAX_PATH_BYTES = 4096
MAX_PATH_DEPTH = 32


def member_path(name):
    # Bound component bookkeeping as well as payload allocation.
    if (len(name) > MAX_PATH_BYTES or len(name.encode("utf-8")) > MAX_PATH_BYTES
            or name.count("/") >= MAX_PATH_DEPTH):
        raise ValueError("Oversized release member path")
    parts = PurePosixPath(name)
    if (not parts.parts or parts.is_absolute() or ".." in parts.parts
            or parts.as_posix() != name or "\\" in name or ":" in name
            or any(ord(char) < 32 or ord(char) == 127 for char in name)
            or any(part.endswith((".", " ")) for part in parts.parts)):
        raise ValueError("Unsafe release member")
    return parts


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def archive(path, root, files, epoch):
    """A new deterministic archive of explicit ordinary files, never links."""
    for name in files:
        member_path(root + "/" + name)
        member_path(name)
    if path.suffix == ".zip":
        # ZIP timestamps are UTC, two-second resolution, and cannot precede 1980.
        timestamp = time.gmtime(min(max(epoch, 315532800), 4354819198))[:6]
        with zipfile.ZipFile(path, "x", compression=zipfile.ZIP_DEFLATED, allowZip64=False) as stream:
            for name, (data, mode) in sorted(files.items()):
                info = zipfile.ZipInfo(root + "/" + name, timestamp)
                info.create_system = 3
                info.external_attr = (stat.S_IFREG | mode) << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                stream.writestr(info, data, compresslevel=9)
        return
    with path.open("xb") as output, gzip.GzipFile(filename="", fileobj=output, mode="wb", mtime=epoch) as compressed, tarfile.open(fileobj=compressed, mode="w") as stream:
        for name, (data, mode) in sorted(files.items()):
            info = tarfile.TarInfo(root + "/" + name)
            info.size, info.mode, info.mtime = len(data), mode, epoch
            stream.addfile(info, io.BytesIO(data))


def archive_snapshot(path, limit=None):
    """Open once, reject links/special files, and freeze bounded input bytes."""
    limit = MAX_UNPACKED_BYTES if limit is None else limit
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    except OSError as error:
        raise ValueError("Compressed release archive is non-ordinary or unavailable") from error
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > limit:
            raise ValueError("Compressed release archive is non-ordinary or oversized")
        with os.fdopen(descriptor, "rb", closefd=False) as source:
            snapshot = source.read(limit + 1)
    finally:
        os.close(descriptor)
    if len(snapshot) > limit:
        raise ValueError("Compressed release archive is oversized")
    return snapshot


def zip_directory(snapshot):
    """Bound the central directory before ZipFile allocates its member objects."""
    with io.BytesIO(snapshot) as source:
        size = len(snapshot)
        source.seek(max(0, size - 65557))
        tail = source.read(65557)
        end = tail.rfind(b"PK\x05\x06")
        if end < 0 or len(tail) - end < 22:
            raise ValueError("Missing ZIP directory")
        _, disk, start_disk, count_disk, count, length, offset, comment = struct.unpack_from("<4s4H2LH", tail, end)
        if (disk or start_disk or count != count_disk or count > MAX_MEMBERS
                or length > MAX_MANIFEST_BYTES or offset + length != size - len(tail) + end
                or end + 22 + comment != len(tail)):
            raise ValueError("Ambiguous or oversized ZIP directory")
        source.seek(offset)
        directory = source.read(length)
    position, actual = 0, 0
    while position < length:
        if position + 46 > length or directory[position:position + 4] != b"PK\x01\x02":
            raise ValueError("Invalid ZIP directory")
        name, extra, comment = struct.unpack_from("<3H", directory, position + 28)
        position += 46 + name + extra + comment
        actual += 1
        if actual > MAX_MEMBERS or position > length:
            raise ValueError("Oversized ZIP directory")
    if actual != count:
        raise ValueError("Inconsistent ZIP member count")


def archive_entries(path, snapshot=None):
    if snapshot is None:
        snapshot = archive_snapshot(path)
    entries, prefix, total, spellings = {}, None, 0, {}

    def accept(name, size):
        nonlocal prefix, total
        parts = member_path(name)
        if len(parts.parts) < 2:
            raise ValueError("Archive contains an unsafe member")
        if prefix is None:
            prefix = parts.parts[0]
        total += size
        if (parts.parts[0] != prefix or name in entries or size < 0
                or size > MAX_MEMBER_BYTES or total > MAX_UNPACKED_BYTES
                or len(entries) >= MAX_MEMBERS
                or (parts.name == "build-manifest.json" and size > MAX_MANIFEST_BYTES)):
            raise ValueError("Ambiguous or oversized release member")
        for index in range(1, len(parts.parts) + 1):
            component = "/".join(parts.parts[:index])
            folded = component.casefold()
            if folded in spellings and spellings[folded] != component:
                raise ValueError("Case collision in release members")
            if index < len(parts.parts) and component in entries:
                raise ValueError("Release file is also a directory")
            spellings[folded] = component
        if any(existing.startswith(name + "/") for existing in entries):
            raise ValueError("Release file is also a directory")

    if path.suffix == ".zip":
        zip_directory(snapshot)
        with zipfile.ZipFile(io.BytesIO(snapshot)) as stream:
            for member in stream.infolist():
                mode = member.external_attr >> 16
                if (member.orig_filename != member.filename or member.is_dir()
                        or stat.S_IFMT(mode) not in (0, stat.S_IFREG)
                        or member.external_attr & 0x10 or mode & 0o7000
                        or member.flag_bits & 1 or member.volume
                        or member.extract_version >= 45
                        or member.compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED)):
                    raise ValueError("Archive contains a non-ordinary or unsafe member")
                accept(member.filename, member.file_size)
                with stream.open(member) as payload:
                    data = payload.read(member.file_size + 1)
                if len(data) != member.file_size:
                    raise ValueError("ZIP member differs from its declared size")
                entries[member.filename] = data
    else:
        # TAR parses extended headers before yielding members, so bound that input too.
        with gzip.GzipFile(fileobj=io.BytesIO(snapshot), mode="rb") as compressed:
            unpacked = compressed.read(MAX_UNPACKED_BYTES + 1)
        if len(unpacked) > MAX_UNPACKED_BYTES:
            raise ValueError("Unpacked release archive exceeds its resource budget")
        with tarfile.open(fileobj=io.BytesIO(unpacked), mode="r:") as stream:
            for member in stream:
                if not member.isfile() or member.issparse() or member.mode & 0o7000:
                    raise ValueError("Archive contains a non-ordinary or unsafe member")
                accept(member.name, member.size)
                entries[member.name] = stream.extractfile(member).read(member.size + 1)
    return entries, prefix


def verify(path, expected=None):
    snapshot = archive_snapshot(path)
    if expected is not None and hashlib.file_digest(io.BytesIO(snapshot), "sha256").hexdigest() != expected:
        raise ValueError("Archive does not match the expected trusted digest")
    entries, prefix = archive_entries(path, snapshot)
    manifest_name = prefix + "/build-manifest.json" if prefix else ""
    if manifest_name not in entries:
        raise ValueError("Missing build manifest")
    manifest = json.loads(entries.pop(manifest_name))
    if manifest.get("format") != 1 or manifest.get("target") not in TARGETS:
        raise ValueError("Unsupported build manifest")
    wanted = {prefix + "/" + name: value for name, value in manifest["files"].items()}
    if entries.keys() != wanted.keys() or any(digest(data) != wanted[name] for name, data in entries.items()):
        raise ValueError("Release contents differ from the build manifest")
    return {"ok": True, "version": manifest["version"], "target": manifest["target"], "files": len(entries),
            "authenticity": "authenticate the archive with its CI attestation or an independently trusted digest"}


def override_snapshot(directory, omitted=()):
    """Public imported source only; cache markers are not upstream source."""
    files, total = {}, 0
    for path in sorted(directory.rglob("*")):
        if path.is_symlink():
            raise ValueError("Symlink in imported source")
        if not path.is_file() or path.name == ".cargo-ok":
            continue
        relative = path.relative_to(directory).as_posix()
        member_path(relative)
        if relative in omitted:
            continue
        if len(files) >= 10000:
            raise ValueError("Imported source exceeds inventory budget")
        data = archive_snapshot(path, min(16 * 1024 * 1024, 64 * 1024 * 1024 - total))
        total += len(data)
        if total > 64 * 1024 * 1024:
            raise ValueError("Imported source exceeds inventory budget")
        files[relative] = digest(data)
    return files


def override_tree_digest(directory, omitted=()):
    return digest(json.dumps(override_snapshot(directory, omitted), sort_keys=True,
                             separators=(",", ":")).encode())


def check_override_sources():
    base = ROOT / "third_party/overrides"
    origins = json.loads((base / "sources.json").read_text())
    for relative, expected in origins["unchanged_primitives"].items():
        path = base / relative
        if path.is_symlink() or not path.resolve().is_relative_to(base.resolve()) or not path.is_file():
            raise ValueError("Unsafe upstream primitive path")
        with path.open("rb") as source:
            if hashlib.file_digest(source, "sha256").hexdigest() != expected:
                raise ValueError("Imported upstream primitive changed: " + relative)
    for key, origin in origins["packages"].items():
        if "directory" not in origin:
            continue
        member_path(origin["directory"])
        directory = base / origin["directory"]
        if directory.is_symlink() or not directory.resolve().is_relative_to(base.resolve()):
            raise ValueError("Unsafe recorded override directory")
        package = tomllib.loads((directory / "Cargo.toml").read_text())["package"]
        vcs = json.loads((directory / ".cargo_vcs_info.json").read_text())
        if (key != package["name"] + "@" + package["version"]
                or origin.get("registry_version", package["version"]) != package["version"]
                or package["repository"].rstrip("/") != origin["repository"].rstrip("/")
                or vcs["git"]["sha1"] != origin["revision"]
                or vcs.get("path_in_vcs", "") != origin["path_in_vcs"]):
            raise ValueError("Override identity/revision differs from source ledger")
        for relative, expected in origin["reviewed_files"].items():
            member_path(relative)
            path = directory / relative
            if path.is_symlink() or not path.resolve().is_relative_to(directory.resolve()) or digest(archive_snapshot(path, 16 * 1024 * 1024)) != expected:
                raise ValueError("Reviewed override file changed: " + key + "/" + relative)
        if override_tree_digest(directory, origin["reviewed_files"]) != origin["upstream_tree_sha256"]:
            raise ValueError("Unreviewed imported source changed: " + key)
        manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
        scope = manifest.get("target", {}).get(origin["sdk_target"], {}) if "sdk_target" in origin else manifest
        dependency = scope.get("dependencies", {}).get(origin["sdk_dependency"])
        if (not isinstance(dependency, dict) or dependency.get("path") != directory.relative_to(ROOT).as_posix()
                or dependency.get("package", origin["sdk_dependency"]) != package["name"]
                or dependency.get("version") != "=" + package["version"]
                or any(field in dependency for field in ("git", "registry"))):
            raise ValueError("SDK dependency must directly inherit the recorded override")
    return origins["packages"]


def check_override_source_files(source_files, notices):
    """Offline source must carry path-dependency code and its original legal data."""
    current = None
    for item in notices:
        origin = item.get("override") or {}
        if "directory" not in origin:
            continue
        if current is None:
            current = check_override_sources()
        if current.get(item["name"] + "@" + item["version"]) != origin:
            raise ValueError("Override provenance changed during source preparation")
        base = ROOT / "third_party/overrides" / origin["directory"]
        required = [base / name for name in override_snapshot(base)]
        required += [ROOT / name for name in ("Cargo.toml", "Cargo.lock", "third_party/overrides/sources.json", "third_party/notices.json")]
        for name in origin["required_notices"]:
            member_path(name)
            required.append(base / name if Path(name).name == name else ROOT / "third_party" / name)
        for path in required:
            name = path.relative_to(ROOT).as_posix()
            if name not in source_files or source_files[name][0] != path.read_bytes():
                raise ValueError("Offline SDK source missing or changed: " + name)


def licenses():
    overrides = check_override_sources()
    metadata = json.loads(command("cargo", "metadata", "--offline", "--locked", "--format-version", "1"))
    extra_path = ROOT / "third_party/notices.json"
    extra = json.loads(extra_path.read_text()) if extra_path.exists() else {"packages": {}, "sources": {}}
    notices, files = [], {}
    for package in sorted(metadata["packages"], key=lambda value: (value["name"], value["version"])):
        if package["id"] == metadata["resolve"]["root"]:
            continue
        key = package["name"] + "@" + package["version"]
        override = overrides.get(key) if package["source"] is None else None
        recorded = overrides.get(key, {})
        if "directory" in recorded:
            expected = ROOT / "third_party/overrides" / recorded["directory"] / "Cargo.toml"
            if package["source"] is not None or Path(package["manifest_path"]).absolute() != expected:
                raise ValueError("Resolved SDK dependency bypasses its recorded path")
        if package["source"] is None and override is None:
            raise ValueError("Unrecorded local dependency source: " + key)
        if not package.get("license"):
            raise ValueError("Review missing dependency license: " + package["name"])
        directory = Path(package["manifest_path"]).parent
        vcs_path = directory / ".cargo_vcs_info.json"
        vcs = json.loads(vcs_path.read_text()) if vcs_path.is_file() and not vcs_path.is_symlink() else {}
        revision = vcs.get("git", {}).get("sha1")
        included = []
        for candidate in sorted(directory.iterdir()):
            if candidate.is_symlink() or not candidate.is_file() or not candidate.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE")):
                continue
            if candidate.stat().st_size > 1024 * 1024:
                raise ValueError("Oversized dependency notice")
            name = f"licenses/{package['name']}-{package['version']}/{candidate.name}"
            data = candidate.read_bytes()
            if not data.strip():
                raise ValueError("Empty dependency notice")
            files[name] = (data, 0o644)
            included.append(name)
        # Curated nested notices supplement, never replace, package-root notices.
        for relative in extra["packages"].get(key, []):
            member_path(relative)
            candidate = ROOT / "third_party" / relative
            if candidate.is_symlink() or not candidate.resolve().is_relative_to((ROOT / "third_party").resolve()) or not candidate.is_file() or candidate.stat().st_size > 1024 * 1024:
                raise ValueError("Unsafe collected notice")
            data = candidate.read_bytes()
            if not data.strip():
                raise ValueError("Empty collected notice")
            if digest(data) != extra["sources"][relative]["sha256"]:
                raise ValueError("Collected upstream notice changed")
            if relative in files and files[relative][0] != data:
                raise ValueError("Conflicting collected notice")
            files[relative] = (data, 0o644)
            if relative not in included:
                included.append(relative)
        notices.append({"name": package["name"], "version": package["version"], "license": package["license"], "repository":package.get("repository"), "revision":revision, "path_in_vcs":vcs.get("path_in_vcs", ""), "override": override, "notices": included})
        if "directory" in recorded:
            if package["license"] != tomllib.loads(expected.read_text())["package"]["license"]:
                raise ValueError("Resolved override license differs from original manifest")
            for relative in recorded["required_notices"]:
                member_path(relative)
                if Path(relative).name == relative:
                    name = f"licenses/{package['name']}-{package['version']}/{relative}"
                    if name not in included or digest(files[name][0]) != recorded["reviewed_files"].get(relative):
                        raise ValueError("Missing or changed original local override notice")
                    data = files[name][0]
                    blob = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data, usedforsecurity=False).hexdigest()
                    if blob != recorded.get("license_git_blob"):
                        raise ValueError("Original local override notice differs from upstream Git blob")
                elif relative not in included or extra["sources"][relative]["revision"] != recorded["revision"]:
                    raise ValueError("Missing or mismatched original override notice")
    return notices, files


def publish_assets(assets, destination):
    """Exclusive complete-file publication with retained stages and matching-byte recovery.

    Multiple directory entries are NOT atomic. A failed link leaves full prior
    members plus a private complete stage; retrying identical inputs fills gaps.
    """
    paths = [destination / name for name in assets]
    for path in paths:
        if (path.exists() or path.is_symlink()) and (
            path.is_symlink() or not path.is_file() or archive_snapshot(path) != assets[path.name]
        ):
            raise FileExistsError("Different or unsafe release asset: " + str(path))
    if all(path.exists() for path in paths):
        return paths
    stage = Path(tempfile.mkdtemp(prefix=".kk-assets-", dir=destination))
    try:
        for name, data in assets.items():
            with (stage / name).open("xb") as output:
                output.write(data)
                output.flush()
                os.fsync(output.fileno())
    except OSError as error:
        raise RuntimeError(f"Incomplete private release stage retained at {stage}; "
                           "no publication attempted; retry identical inputs") from error
    try:
        for path in paths:
            try:
                os.link(stage / path.name, path)
            except FileExistsError:
                if path.is_symlink() or not path.is_file() or archive_snapshot(path) != assets[path.name]:
                    raise FileExistsError("Different or unsafe release asset: " + str(path)) from None
    except OSError as error:
        raise RuntimeError(f"Partial release asset publication; complete stage retained at {stage}; "
                           "retry identical inputs to recover, never overwrite foreign bytes") from error
    return paths


def bootstrap_assets(target, version, commit, files, destination):
    """Companion distribution, retaining every reviewed notice byte (not legal advice)."""
    contract = HELPERS[target]
    helper_name = f"kk-v{version}-{TARGETS[target]}-bootstrap{contract['suffix']}"
    notice_name = f"kk-v{version}-{TARGETS[target]}-bootstrap-notices.json"
    if not files.get("LICENSE", (b"",))[0].strip():
        raise ValueError("Bootstrap requires the project license")
    inventory = json.loads(files["THIRD-PARTY.json"][0])
    names = set(files) - {"kk"}  # Same-version build metadata and original notice inventory.
    for package in inventory:
        if not package["notices"]:
            raise ValueError("Bootstrap requires upstream notice inventory")
        names.update(package["notices"])
        if any(not files[name][0].strip() for name in package["notices"]):
            raise ValueError("Bootstrap refuses empty upstream notices")
    texts = {}
    for name in sorted(names):
        member_path(name)
        texts[name] = files[name][0].decode("utf-8")  # Never silently replace author bytes.
    helper = files["kk"][0]
    record = {"format": 1, "version": version, "commit": commit, "target": target,
              **contract, "helper": {"asset": helper_name, "sha256": digest(helper)},
              "notices_asset": notice_name, "files": texts}
    notices = (json.dumps(record, ensure_ascii=True, indent=2) + "\n").encode()
    if not helper or len(helper) > MAX_MEMBER_BYTES or len(notices) > MAX_MEMBER_BYTES:
        raise ValueError("Oversized or empty bootstrap distribution")
    return publish_assets({helper_name: helper, notice_name: notices}, destination)


def macos_minimum(binary):
    """Read Apple's parsed deployment metadata; never run the candidate binary."""
    if platform.system() != "Darwin":
        raise ValueError("macOS packaging requires native Apple otool/codesign")
    command("/usr/bin/codesign", "--verify", "--strict", str(binary))
    output = command("/usr/bin/otool", "-l", str(binary))
    versions = []
    for block in re.split(r"(?m)^Load command [0-9]+$", output):
        if re.search(r"(?m)^\s*cmd LC_BUILD_VERSION$", block):
            if not re.search(r"(?m)^\s*platform (?:1|macos)$", block):
                raise ValueError("Mach-O deployment platform is not macOS")
            match = re.search(r"(?m)^\s*minos ([0-9]+\.[0-9]+(?:\.[0-9]+)?)$", block)
        elif re.search(r"(?m)^\s*cmd LC_VERSION_MIN_MACOSX$", block):
            match = re.search(r"(?m)^\s*version ([0-9]+\.[0-9]+(?:\.[0-9]+)?)$", block)
        else:
            continue
        if match is None:
            raise ValueError("Missing Mach-O deployment version")
        components = [int(part) for part in match[1].split(".")]
        components += [0] * (3 - len(components))
        if components[0] < 10 or components[0] > 65535 or any(part > 255 for part in components[1:]):
            raise ValueError("Invalid Mach-O deployment version")
        versions.append(".".join(map(str, components)))
    if len(versions) != 1:
        raise ValueError("Expected exactly one thin Mach-O macOS deployment requirement")
    return versions[0]


def bundle(target, destination):
    if target not in TARGETS:
        raise ValueError("Unsupported native release target")
    if command("git", "status", "--porcelain"):
        raise ValueError("Release archives require a clean committed source tree")
    metadata = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]
    version = metadata["version"]
    tag = os.environ.get("CI_COMMIT_TAG") or (os.environ.get("GITHUB_REF_NAME") if os.environ.get("GITHUB_REF_TYPE") == "tag" else None)
    if tag is not None and tag != "v" + version:
        raise ValueError("Release tag differs from the package version")
    epoch = int(command("git", "show", "-s", "--format=%ct", "HEAD"))
    commit = command("git", "rev-parse", "HEAD")
    binary = ROOT / "target" / target / "release/kk"
    if not binary.is_file() or binary.is_symlink():
        raise ValueError("Build the exact release target first")
    notices, files = licenses()
    missing = [item["name"] for item in notices if not item["notices"]]
    if missing:
        raise ValueError("Collect and review missing upstream notices before release: " + ", ".join(missing))
    files["kk"] = (binary.read_bytes(), 0o755)
    for name in ["LICENSE", "README.md", "CONTRACT.md", "Cargo.lock"]:
        files[name] = ((ROOT / name).read_bytes(), 0o644)
    files["THIRD-PARTY.json"] = ((json.dumps(notices, indent=2) + "\n").encode(), 0o644)
    apple = target.endswith("-apple-darwin")
    minimum_os = macos_minimum(binary) if apple else None
    required = set() if apple else set(re.findall(r"GLIBC_(\d+\.\d+(?:\.\d+)?)", command("readelf", "--version-info", str(binary))))
    manifest = {"format": 1, "version": version, "target": target, "commit": commit,
                "rustc": command("rustc", "--version"), "build_python_libc": platform.libc_ver(),
                "build_system_libc": None if apple else command("ldd", "--version").splitlines()[0],
                "binary_glibc_required": max(required, key=lambda value:tuple(map(int,value.split(".")))) if required else None,
                "runtime_dependencies": ["GStreamer tools/base/good with Opus/VP8", "util-linux prlimit/setpriv for bounded Linux native capture"],
                "files": {name: digest(data) for name, (data, _) in sorted(files.items())}}
    if apple:
        manifest["binary_macos_required"] = minimum_os
        manifest["runtime_dependencies"] = ["Apple system libraries in Mach-O load commands", "GStreamer tools/base/good with Opus/VP8 for native media"]
    files["build-manifest.json"] = ((json.dumps(manifest, indent=2) + "\n").encode(), 0o644)
    destination.mkdir(parents=True, exist_ok=True)
    # Regenerate the complete expected set privately, including source/checksums.
    # Never create exclusive archives directly in a partially published release:
    # retry must compare ALL expected bytes before it fills any missing member.
    stage = Path(tempfile.mkdtemp(prefix=".kk-bundle-", dir=destination))
    stem = f"kk-v{version}-{TARGETS[target]}"
    output = stage / (stem + ".tar.gz")
    archive(output, stem, files, epoch)
    verify(output)
    zip_output = stage / (stem + ".zip")
    archive(zip_output, stem, files, epoch)
    verify(zip_output)
    manifest_path = stage / (stem + ".build.json")
    with manifest_path.open("xb") as record:
        record.write(files["build-manifest.json"][0])
    helper_path, notices_path = bootstrap_assets(target, version, commit, files, stage)
    entrypoints = publish_assets({name: (ROOT / "scripts" / name).read_bytes()
                                  for name in ("install.sh", "install.ps1")}, stage)
    # Vendor every locked target dependency so source can rebuild without a registry.
    work = Path(tempfile.mkdtemp(prefix="kk-source-", dir=ROOT / "target"))
    vendor = work / "vendor"
    subprocess.run(["cargo", "vendor", "--offline", "--locked", "--versioned-dirs", str(vendor)], cwd=ROOT, check=True, stdout=subprocess.PIPE)
    source_files = {}
    listing = subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).decode().split("\0")
    for name in filter(None, listing):
        path = ROOT / name
        if path.is_symlink() or not path.is_file():
            raise ValueError("Release source must contain only ordinary tracked files")
        source_files[name] = (path.read_bytes(), 0o755 if path.stat().st_mode & 0o111 else 0o644)
    for path in sorted(vendor.rglob("*")):
        if path.is_symlink():
            raise ValueError("Vendored source contains a symlink")
        if path.is_file():
            source_files["vendor/" + path.relative_to(vendor).as_posix()] = (path.read_bytes(), 0o755 if path.stat().st_mode & 0o111 else 0o644)
    check_override_source_files(source_files, notices)
    config = '.cargo/config.toml'
    if config in source_files:
        raise ValueError("Review existing Cargo configuration before vendoring")
    source_files[config] = (b'[source.crates-io]\nreplace-with = "vendored-sources"\n[source.vendored-sources]\ndirectory = "vendor"\n', 0o644)
    source = stage / f"kk-v{version}-source-offline.tar.gz"
    archive(source, f"kk-v{version}-source", source_files, epoch)
    sums = stage / (stem + ".sha256")
    with sums.open("x") as record:
        for path in [output, zip_output, manifest_path, source, helper_path, notices_path, *entrypoints]:
            record.write(f"{digest(path.read_bytes())}  {path.name}\n")
    try:
        publish_assets({path.name: path.read_bytes() for path in
                        [output, zip_output, manifest_path, helper_path, notices_path, *entrypoints, source, sums]}, destination)
    except RuntimeError as error:
        raise RuntimeError(f"Bundle publication incomplete; expected assets retained at {stage}; "
                           "rerun the same bundle command with identical inputs to recover; "
                           f"existing files are never overwritten. {error}") from error
    print(json.dumps({"archive": str(destination / output.name), "zip_archive": str(destination / zip_output.name),
                      "offline_source": str(destination / source.name), "checksums": str(destination / sums.name), "commit": commit,
                      "bootstrap_helper": str(destination / helper_path.name), "bootstrap_notices": str(destination / notices_path.name),
                      "bootstrap_entrypoints": [str(destination / path.name) for path in entrypoints]}))


def collect(source, destination):
    """Merge matrix artifacts only when equal-named files have equal bytes."""
    destination.mkdir(parents=True, exist_ok=True)
    for path in sorted(source.rglob("*")):
        if path.is_symlink():
            raise ValueError("Artifact links are not release files")
        if not path.is_file():
            continue
        target = destination / path.name
        data = path.read_bytes()
        if target.exists():
            if target.is_symlink() or digest(target.read_bytes()) != digest(data):
                raise ValueError("Matrix artifact conflict: " + path.name)
        else:
            with target.open("xb") as output:
                output.write(data)


def install_binary(binary, prefix, version):
    """Keep versioned binaries immutable and never replace an unrelated CLI."""
    prefix = prefix.resolve()
    managed = prefix / "lib/kk"
    link = prefix / "bin/kk"
    if (link.exists() or link.is_symlink()) and (not link.is_symlink() or not link.resolve().is_relative_to(managed)):
        raise ValueError("Refusing to replace an unrelated kk executable")
    directory = managed / version
    directory.mkdir(parents=True, exist_ok=True)
    target = directory / "kk"
    data = binary.read_bytes()
    if target.exists():
        if target.is_symlink() or digest(target.read_bytes()) != digest(data):
            raise ValueError("This version is already installed with different bytes")
    else:
        with target.open("xb") as output:
            output.write(data)
        target.chmod(0o755)
    link.parent.mkdir(parents=True, exist_ok=True)
    temporary = link.with_name(".kk-link-" + str(os.getpid()))
    temporary.symlink_to(target)
    os.replace(temporary, link)
    return link


def release_tag(push=False, remotes=("github", "gitlab")):
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    tag = "v" + version
    if push and (not remotes or any(remote not in {"github", "gitlab"} for remote in remotes)):
        raise ValueError("Release publication requires selected authorized remotes")
    if command("git", "status", "--porcelain"):
        raise ValueError("Release tags require a clean committed tree")
    if push:
        if command("git", "rev-list", "-n", "1", tag) != command("git", "rev-parse", "HEAD"):
            raise ValueError("Release tag does not identify this exact committed source")
        for remote in remotes:
            if not command("git", "remote", "get-url", remote).startswith("git@"):
                raise ValueError("Release publication requires SSH remotes")
        for remote in remotes:
            subprocess.run(["git", "push", remote, "refs/tags/" + tag], cwd=ROOT, check=True)
    else:
        subprocess.run(["git", "tag", "-a", tag, "-m", "K&K " + version], cwd=ROOT, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest="action", required=True)
    pack = actions.add_parser("bundle")
    pack.add_argument("--target", required=True, choices=TARGETS)
    pack.add_argument("--output", type=Path, default=ROOT / "target/dist")
    check = actions.add_parser("verify")
    check.add_argument("archive", type=Path)
    check.add_argument("--sha256")
    licensing = actions.add_parser("licenses")
    licensing.add_argument("--output", type=Path)
    merge = actions.add_parser("collect")
    merge.add_argument("--input", type=Path, required=True)
    merge.add_argument("--output", type=Path, required=True)
    local = actions.add_parser("install")
    local.add_argument("--prefix", type=Path, default=Path.home() / ".local")
    actions.add_parser("tag")
    push_tag = actions.add_parser("push-tag")
    push_tag.add_argument("--remotes", nargs="+", default=["github", "gitlab"])
    args = parser.parse_args()
    if args.action == "bundle":
        bundle(args.target, args.output)
    elif args.action == "verify":
        print(json.dumps(verify(args.archive, args.sha256)))
    elif args.action == "collect":
        collect(args.input, args.output)
    elif args.action == "install":
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
        binary = ROOT / "target/release/kk"
        if subprocess.check_output([str(binary), "--version"], text=True).strip() != "kk " + version:
            raise ValueError("Local binary differs from the selected package version")
        print(install_binary(binary, args.prefix, version))
    elif args.action in {"tag", "push-tag"}:
        release_tag(push=args.action == "push-tag", remotes=getattr(args, "remotes", ("github", "gitlab")))
    else:
        notices, _ = licenses()
        text = json.dumps(notices, indent=2) + "\n"
        if args.output:
            with args.output.open("x") as output:
                output.write(text)
            print(json.dumps({"packages":len(notices),"report":str(args.output),"licenses":sorted({item["license"] for item in notices}),"without_root_notices":[{"package":item["name"]+"@"+item["version"],"repository":item["repository"],"revision":item["revision"]} for item in notices if not item["notices"]]}))
        else:
            print(text)


if __name__ == "__main__":
    main()
