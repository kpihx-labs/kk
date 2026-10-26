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
import subprocess
import tarfile
import tempfile
import tomllib
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {"x86_64-unknown-linux-gnu": "linux-x86_64", "aarch64-unknown-linux-gnu": "linux-aarch64"}
MAX_MEMBERS = 4096
MAX_UNPACKED_BYTES = 256 * 1024 * 1024
MAX_MANIFEST_BYTES = 4 * 1024 * 1024


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def archive(path, root, files, epoch):
    """A new deterministic archive of explicit ordinary files, never links."""
    with path.open("xb") as output, gzip.GzipFile(filename="", fileobj=output, mode="wb", mtime=epoch) as compressed, tarfile.open(fileobj=compressed, mode="w") as stream:
        for name, (data, mode) in sorted(files.items()):
            relative = PurePosixPath(name)
            if not relative.parts or relative.is_absolute() or ".." in relative.parts or relative.as_posix() != name:
                raise ValueError("Unsafe release member")
            info = tarfile.TarInfo(root + "/" + name)
            info.size, info.mode, info.mtime = len(data), mode, epoch
            stream.addfile(info, io.BytesIO(data))


def verify(path, expected=None):
    if not path.is_file() or path.stat().st_size > MAX_UNPACKED_BYTES:
        raise ValueError("Compressed release archive is non-ordinary or oversized")
    if expected is not None:
        with path.open("rb") as compressed:
            if hashlib.file_digest(compressed, "sha256").hexdigest() != expected:
                raise ValueError("Archive does not match the expected trusted digest")
    # TAR parses extended headers before yielding members, so bound that input too.
    with gzip.open(path, "rb") as compressed:
        unpacked = compressed.read(MAX_UNPACKED_BYTES + 1)
    if len(unpacked) > MAX_UNPACKED_BYTES:
        raise ValueError("Unpacked release archive exceeds its resource budget")
    entries, prefix, total = {}, None, 0
    with tarfile.open(fileobj=io.BytesIO(unpacked), mode="r:") as stream:
        for member in stream:
            parts = PurePosixPath(member.name)
            if not member.isfile() or parts.is_absolute() or ".." in parts.parts or len(parts.parts) < 2 or parts.as_posix() != member.name:
                raise ValueError("Archive contains a non-ordinary or unsafe member")
            if prefix is None:
                prefix = parts.parts[0]
            total += member.size
            if parts.parts[0] != prefix or member.name in entries or member.size > 128 * 1024 * 1024 or total > MAX_UNPACKED_BYTES or len(entries) >= MAX_MEMBERS or (parts.name == "build-manifest.json" and member.size > MAX_MANIFEST_BYTES):
                raise ValueError("Ambiguous or oversized release member")
            entries[member.name] = stream.extractfile(member).read()
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


def licenses():
    metadata = json.loads(command("cargo", "metadata", "--offline", "--locked", "--format-version", "1"))
    extra_path = ROOT / "third_party/notices.json"
    extra = json.loads(extra_path.read_text()) if extra_path.exists() else {"packages": {}, "sources": {}}
    notices, files = [], {}
    for package in sorted(metadata["packages"], key=lambda value: (value["name"], value["version"])):
        if package["source"] is None:
            continue
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
            files[name] = (candidate.read_bytes(), 0o644)
            included.append(name)
        if not included:
            for relative in extra["packages"].get(package["name"] + "@" + package["version"], []):
                candidate = ROOT / "third_party" / relative
                if candidate.is_symlink() or not candidate.resolve().is_relative_to((ROOT / "third_party").resolve()) or candidate.stat().st_size > 1024 * 1024:
                    raise ValueError("Unsafe collected notice")
                data = candidate.read_bytes()
                if digest(data) != extra["sources"][relative]["sha256"]:
                    raise ValueError("Collected upstream notice changed")
                files[relative] = (data, 0o644)
                included.append(relative)
        notices.append({"name": package["name"], "version": package["version"], "license": package["license"], "repository":package.get("repository"), "revision":revision, "path_in_vcs":vcs.get("path_in_vcs", ""), "notices": included})
    return notices, files


def bundle(target, destination):
    if target not in TARGETS:
        raise ValueError("Only qualified Linux release targets are supported")
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
    required = set(re.findall(r"GLIBC_(\d+\.\d+(?:\.\d+)?)", command("readelf", "--version-info", str(binary))))
    manifest = {"format": 1, "version": version, "target": target, "commit": commit,
                "rustc": command("rustc", "--version"), "build_python_libc": platform.libc_ver(),
                "build_system_libc": command("ldd", "--version").splitlines()[0],
                "binary_glibc_required": max(required, key=lambda value:tuple(map(int,value.split(".")))) if required else None,
                "runtime_dependencies": ["GStreamer tools/base/good with Opus/VP8", "util-linux prlimit/setpriv for bounded Linux native capture"],
                "files": {name: digest(data) for name, (data, _) in sorted(files.items())}}
    files["build-manifest.json"] = ((json.dumps(manifest, indent=2) + "\n").encode(), 0o644)
    destination.mkdir(parents=True, exist_ok=True)
    stem = f"kk-v{version}-{TARGETS[target]}"
    output = destination / (stem + ".tar.gz")
    archive(output, stem, files, epoch)
    verify(output)
    manifest_path = destination / (stem + ".build.json")
    with manifest_path.open("xb") as record:
        record.write(files["build-manifest.json"][0])
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
    config = '.cargo/config.toml'
    if config in source_files:
        raise ValueError("Review existing Cargo configuration before vendoring")
    source_files[config] = (b'[source.crates-io]\nreplace-with = "vendored-sources"\n[source.vendored-sources]\ndirectory = "vendor"\n', 0o644)
    source = destination / f"kk-v{version}-source-offline.tar.gz"
    archive(source, f"kk-v{version}-source", source_files, epoch)
    sums = destination / (stem + ".sha256")
    with sums.open("x") as record:
        for path in [output, manifest_path, source]:
            record.write(f"{digest(path.read_bytes())}  {path.name}\n")
    print(json.dumps({"archive": str(output), "offline_source": str(source), "checksums": str(sums), "commit": commit}))


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


def release_tag(push=False):
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    tag = "v" + version
    if command("git", "status", "--porcelain"):
        raise ValueError("Release tags require a clean committed tree")
    if push:
        if command("git", "rev-list", "-n", "1", tag) != command("git", "rev-parse", "HEAD"):
            raise ValueError("Release tag does not identify this exact committed source")
        for remote in ["github", "gitlab"]:
            if not command("git", "remote", "get-url", remote).startswith("git@"):
                raise ValueError("Release publication requires both SSH remotes")
        for remote in ["github", "gitlab"]:
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
    actions.add_parser("push-tag")
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
        release_tag(push=args.action == "push-tag")
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
