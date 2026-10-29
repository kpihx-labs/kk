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


def prepare(arguments):
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

    def run(*argv, stdout=None):
        subprocess.run([str(gh), *map(str, argv)], cwd=output, env=environment,
                       stdout=stdout, check=True, timeout=180)

    tag = "v" + arguments.version
    run("release", "download", tag, "--repo", REPOSITORY,
        "--pattern", arguments.asset, "--dir", output)
    archive = output / arguments.asset
    archive_record = digest(archive, MAX_ARCHIVE)
    run("attestation", "download", archive, "--repo", REPOSITORY)
    bundle = output / ("sha256:" + archive_record["sha256"] + ".jsonl")
    bundle_record = digest(bundle, 16 * 1024 * 1024)
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
    with (output / "kit.json").open("x", encoding="utf-8") as destination:
        json.dump(record, destination, indent=2)
        destination.write("\n")
    print(json.dumps(record, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gh", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--asset", required=True)
    arguments = parser.parse_args()
    try:
        prepare(arguments)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Kit preparation failed; partial output is retained: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
