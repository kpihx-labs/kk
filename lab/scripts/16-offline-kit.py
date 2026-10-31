#!/usr/bin/env python3
"""Prepare public inputs for a separate isolated test; never claim offline execution."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import resource
import stat
import subprocess
import sys
import tempfile

REPOSITORY = "kpihx-labs/kk"
WORKFLOW = REPOSITORY + "/.github/workflows/check.yml"
MAX_ARCHIVE = 256 * 1024 * 1024


def digest(path, limit):
    if path.is_symlink() or not stat.S_ISREG(path.stat().st_mode):
        raise ValueError("kit input must be an ordinary file")
    result = hashlib.sha256()
    count = 0
    with path.open("rb") as stream:
        while chunk := stream.read(65536):
            count += len(chunk)
            if count > limit:
                raise ValueError("kit input exceeds its byte budget")
            result.update(chunk)
    if not count:
        raise ValueError("empty kit input")
    return {"name": path.name, "bytes": count, "sha256": result.hexdigest()}


def prepare(arguments, *, announce=True):
    gh = arguments.gh
    if not gh.is_absolute() or gh.is_symlink() or not gh.is_file() or not os.access(gh, os.X_OK):
        raise ValueError("--gh must be an absolute, ordinary, explicitly trusted executable")
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?", arguments.version):
        raise ValueError("invalid explicit version")
    if not re.fullmatch(r"[0-9a-f]{40}", arguments.commit):
        raise ValueError("expected full lowercase source commit")
    if (len(arguments.asset) > 200 or not re.fullmatch(r"[0-9A-Za-z._-]+", arguments.asset)
            or not arguments.asset.endswith((".tar.gz", ".zip"))):
        raise ValueError("--asset must select one literal TAR gzip or ZIP basename")
    bootstrap = getattr(arguments, "with_bootstrap", False)
    architecture = re.fullmatch(r"kk-v" + re.escape(arguments.version)
                                + r"-(linux|macos)-(x86_64|aarch64)\.(?:tar\.gz|zip)", arguments.asset)
    if bootstrap and not architecture:
        raise ValueError("--with-bootstrap requires a canonical Linux/macOS binary asset")
    previous_version = getattr(arguments, "previous_version", None)
    previous_commit = getattr(arguments, "previous_commit", None)
    if bool(previous_version) != bool(previous_commit):
        raise ValueError("--previous-version and --previous-commit must be supplied together")
    if previous_version and (not bootstrap or previous_version == arguments.version):
        raise ValueError("a two-version kit requires --with-bootstrap and distinct versions")
    if previous_version and (not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?", previous_version)
                             or not re.fullmatch(r"[0-9a-f]{40}", previous_commit)):
        raise ValueError("invalid explicit previous release selectors")
    output = arguments.output
    if not output.is_absolute() or not output.parent.is_dir():
        raise ValueError("--output needs an absolute existing parent")
    output.mkdir(mode=0o700)  # Exclusive: retain partial kits, never overwrite one.
    os.umask(0o077)
    soft, hard = resource.getrlimit(resource.RLIMIT_FSIZE)
    ceiling = min(value for value in (MAX_ARCHIVE, soft, hard) if value != resource.RLIM_INFINITY)
    resource.setrlimit(resource.RLIMIT_FSIZE, (ceiling, ceiling))
    environment = {**os.environ, "GH_HOST": "github.com", "GH_PAGER": "cat", "PAGER": "cat"}
    environment.pop("GH_DEBUG", None)

    def run(*argv, stdout=None, cwd=output):
        subprocess.run([str(gh), *map(str, argv)], cwd=cwd, env=environment,
                       stdout=stdout, check=True, timeout=180)

    tag = "v" + arguments.version
    run("release", "download", tag, "--repo", REPOSITORY,
        "--pattern", arguments.asset, "--dir", output)
    archive = output / arguments.asset
    archive_record = digest(archive, MAX_ARCHIVE)
    # gh uses a colon-bearing filename on Unix, which FAT/exFAT USB media cannot
    # store. Download on the local temporary filesystem, then export only a
    # portable basename. Keep the staging evidence, including on failure.
    staging = Path(tempfile.mkdtemp(prefix="kk-kit-attestation-"))
    print(f"Attestation download staging retained at: {staging}", file=sys.stderr)

    def export_bundle(subject, subject_record, basename):
        run("attestation", "download", subject, "--repo", REPOSITORY, cwd=staging)
        downloaded = staging / ("sha256:" + subject_record["sha256"] + ".jsonl")
        digest(downloaded, 16 * 1024 * 1024)
        exported = output / basename
        with downloaded.open("rb") as source, exported.open("xb") as destination:
            data = source.read(16 * 1024 * 1024 + 1)
            if len(data) > 16 * 1024 * 1024:
                raise ValueError("kit bundle exceeds its byte budget")
            destination.write(data)
        return digest(exported, 16 * 1024 * 1024)

    bundle_record = export_bundle(archive, archive_record, "attestation.jsonl")
    bundle = output / bundle_record["name"]
    roots = output / "trusted-root.jsonl"
    with roots.open("xb") as destination:
        run("attestation", "trusted-root", stdout=destination)
    root_record = digest(roots, 1024 * 1024)
    run("attestation", "verify", archive, "--repo", REPOSITORY,
        "--signer-workflow", WORKFLOW, "--source-ref", "refs/tags/" + tag,
        "--source-digest", arguments.commit, "--deny-self-hosted-runners",
        "--bundle", bundle, "--custom-trusted-root", roots)
    record = {"repository": REPOSITORY, "version": arguments.version,
              "commit": arguments.commit, "archive": archive_record,
              "bundle": bundle_record, "trusted_root": root_record,
              "verification": "real gh with explicit local bundle and public roots",
               "network_isolation": "not tested", "installation": "not run"}
    if bootstrap:
        stem = f"kk-v{arguments.version}-{architecture[1]}-{architecture[2]}-bootstrap"
        record["bootstrap"] = {}
        for key, name in (("helper", stem + ".bin"), ("notices", stem + "-notices.json"),
                          ("entrypoint", "install.sh")):
            run("release", "download", tag, "--repo", REPOSITORY, "--pattern", name, "--dir", output)
            subject = output / name
            subject_record = digest(subject, MAX_ARCHIVE if key != "entrypoint" else 1024 * 1024)
            subject_bundle = export_bundle(subject, subject_record, name + ".jsonl")
            run("attestation", "verify", subject, "--repo", REPOSITORY,
                "--signer-workflow", WORKFLOW, "--source-ref", "refs/tags/" + tag,
                "--source-digest", arguments.commit, "--deny-self-hosted-runners",
                "--bundle", output / subject_bundle["name"], "--custom-trusted-root", roots)
            record["bootstrap"][key] = subject_record
            record["bootstrap"][key + "_bundle"] = subject_bundle
    if previous_version:
        previous = argparse.Namespace(**vars(arguments))
        previous.version, previous.commit = previous_version, previous_commit
        previous.asset = arguments.asset.replace("kk-v" + arguments.version + "-", "kk-v" + previous_version + "-", 1)
        previous.output = output / "previous"
        previous.previous_version = previous.previous_commit = None
        previous_record = prepare(previous, announce=False)
        record["previous_kit"] = {"directory": "previous", "version": previous_record["version"],
                                  "commit": previous_record["commit"],
                                  "manifest": digest(previous.output / "kit.json", 1024 * 1024)}
    with (output / "kit.json").open("x", encoding="utf-8") as destination:
        json.dump(record, destination, indent=2)
        destination.write("\n")
    if announce:
        print(json.dumps(record, indent=2))
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gh", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--asset", required=True)
    parser.add_argument("--with-bootstrap", action="store_true",
                        help="also authenticate/export Linux/macOS raw helper, notices and install.sh with portable sidecars")
    parser.add_argument("--previous-version", help="also assemble the exact previous release under previous/ for offline rollback")
    parser.add_argument("--previous-commit", help="independently selected full source commit of the previous release")
    arguments = parser.parse_args()
    try:
        prepare(arguments)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Kit preparation failed; partial output is retained: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
