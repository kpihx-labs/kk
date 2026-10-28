#!/usr/bin/env -S uv run --script
"""Create explicit public test fixtures without replacing existing files."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import stat


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    parent = args.directory.parent.lstat()
    if not stat.S_ISDIR(parent.st_mode) or parent.st_uid != os.getuid():
        parser.error("fixture parent must already be an owned directory")
    args.directory.mkdir(mode=0o700, exist_ok=True)
    mode = args.directory.lstat()
    if not stat.S_ISDIR(mode.st_mode) or mode.st_uid != os.getuid() or mode.st_mode & 0o777 != 0o700:
        parser.error("fixture directory must be owned, private and non-symlink")
    fixtures = {"kk-ui-pattern.bin": bytes(range(256)) * 256,
                "kk-ui-empty.bin": b"", "kk-ui-note.txt": b"Study group notes\nAlex and Morgan: test fixture only.\n"}
    results = []
    for name, data in fixtures.items():
        path = args.directory / name
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, "wb") as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        results.append({"path": str(path), "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
    print(json.dumps(results, sort_keys=True))


if __name__ == "__main__":
    main()
