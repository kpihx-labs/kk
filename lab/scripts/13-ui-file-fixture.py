#!/usr/bin/env -S uv run --script
"""Assign an explicit disposable fixture to an owned K&K browser file input.

This exercises the real import handler, not the operating-system file picker.
Opaque fixture bytes stay in a private temporary payload, never argv or output.
"""

import argparse
import base64
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tempfile
from urllib.parse import urlsplit


def disposable_ceremony(path: Path, root: Path) -> bool:
    """Recognize only a private app-generated invitation in this repo's QA tree."""
    try:
        root = root.resolve(strict=True)
        if not root.is_relative_to(Path(__file__).resolve().parents[2] / "target"):
            return False
        parts = path.relative_to(root).parts
        if (len(parts) != 6 or parts[:2] != ("kk", "profiles")
                or not re.fullmatch(r"[a-z0-9-]{1,40}", parts[2])
                or parts[3] != "ui" or not re.fullmatch(r"ceremony-[A-Za-z0-9]{6,16}", parts[4])
                or parts[5] != "invitation.json"):
            return False
        for directory in (root, *[root.joinpath(*parts[:i]) for i in range(1, 6)]):
            metadata = directory.lstat()
            if (not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid()
                    or metadata.st_mode & 0o7777 != 0o700):
                return False
        metadata = path.lstat()
        return (stat.S_ISREG(metadata.st_mode) and metadata.st_uid == os.getuid()
                and metadata.st_mode & 0o7777 == 0o600 and metadata.st_nlink == 1
                and metadata.st_size <= 4096)
    except (OSError, ValueError):
        return False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--origin", required=True)
    parser.add_argument("--file", type=Path, required=True)
    parser.add_argument("--fixture-root", type=Path, required=True)
    parser.add_argument("--payload-directory", type=Path, required=True)
    parser.add_argument("--selector", default="dialog input[name=invitation-file]")
    parser.add_argument("--mime", default="application/json")
    parser.add_argument("--ceremony-invitation", action="store_true",
                        help="explicit private app-generated invitation under this repo's disposable target tree")
    args = parser.parse_args()
    origin = urlsplit(args.origin)
    if origin.scheme != "http" or origin.hostname != "127.0.0.1" or not origin.port:
        parser.error("an exact owned loopback origin is required")
    if args.origin != f"http://127.0.0.1:{origin.port}":
        parser.error("the origin must not contain credentials, paths or fragments")
    path = args.file.absolute()
    scoped = path.parent.resolve().is_relative_to(args.fixture_root.resolve())
    named = path.name.startswith("kk-")
    if args.ceremony_invitation:
        named = disposable_ceremony(path, args.fixture_root.resolve())
    if not named or not scoped:
        parser.error("only explicitly scoped kk-prefixed disposable fixtures are accepted")
    directory = args.payload_directory
    metadata = directory.lstat()
    if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid() or metadata.st_mode & 0o777 != 0o700:
        parser.error("payload directory must already be owned, private and non-symlink")
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    maximum = 4096 if args.ceremony_invitation else 128 * 1024
    with os.fdopen(fd, "rb") as fixture:
        metadata = os.fstat(fixture.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.getuid() or metadata.st_nlink != 1 or metadata.st_size > maximum or (args.ceremony_invitation and metadata.st_mode & 0o7777 != 0o600):
            parser.error("fixture must be an owned bounded ordinary single-link file")
        data = fixture.read(maximum + 1)
    if len(data) > maximum:
        parser.error("fixture grew beyond its bound")
    expression = """(async () => {
      if (location.origin !== ORIGIN || !document.querySelector('#profile-name')) throw new Error('wrong fixture origin');
      const inputs = document.querySelectorAll(SELECTOR);
      if (inputs.length !== 1 || inputs[0].type !== 'file') throw new Error('ambiguous fixture input');
      const input = inputs[0], transfer = new DataTransfer();
      const bytes = Uint8Array.from(atob(DATA), c => c.charCodeAt(0));
      transfer.items.add(new File([bytes], FILENAME, {type: MIME}));
      input.files = transfer.files;
      const assigned = input.files.length === 1;
      input.dispatchEvent(new Event('change', {bubbles: true}));
      return {assigned, filename: FILENAME, bytes: bytes.length, method: 'synthetic-file-input'};
    })()"""
    replacements = {"ORIGIN": args.origin, "SELECTOR": args.selector, "DATA": base64.b64encode(data).decode("ascii"), "FILENAME": path.name, "MIME": args.mime}
    # Replace placeholders once, without interpreting replacement contents.
    expression = re.sub(r"\b(?:ORIGIN|SELECTOR|DATA|FILENAME|MIME)\b", lambda match: json.dumps(replacements[match.group()]), expression)
    payload = {"profile": "default", "target_id": args.target, "expression": expression, "await_promise": True}
    with tempfile.NamedTemporaryFile(mode="w", suffix=".json", dir=directory, encoding="utf-8") as request:
        json.dump(payload, request)
        request.flush()
        result = subprocess.run(["browser-proxy", "do", "page-evaluate", request.name], capture_output=True, text=True, check=False)
    try:
        envelope = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError("browser fixture action returned no structured result") from error
    if result.returncode or envelope.get("meta", {}).get("status") != "ok":
        raise RuntimeError("browser fixture assignment failed; no opaque payload is printed")
    print(json.dumps(envelope["data"]["result"], sort_keys=True))


if __name__ == "__main__":
    main()
