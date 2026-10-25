#!/usr/bin/env -S uv run --script
"""Execute real native MLS credential tests on each declared OS, not a room-network gate."""

import hashlib
import json
import re
import subprocess
import sys
import time
from pathlib import Path

from kk_guest import Guest, declared_peers

ROOT = Path(__file__).resolve().parents[1]


def build():
    command = ["cargo", "test", "--offline", "--locked", "--release", "--jobs", "2", "--lib", "--no-run",
               "--message-format=json", "--manifest-path", str(ROOT.parent / "Cargo.toml")]
    result = subprocess.run(command, stdout=subprocess.PIPE, text=True, check=False)
    artifacts = []
    for line in result.stdout.splitlines():
        message = json.loads(line)
        if message.get("reason") == "compiler-message":
            print(message["message"].get("rendered", ""), file=sys.stderr, end="")
        if (message.get("reason") == "compiler-artifact" and message.get("profile", {}).get("test")
                and "lib" in message["target"]["kind"] and message.get("executable")):
            artifacts.append(Path(message["executable"]))
    if result.returncode:
        raise SystemExit(result.returncode)
    if len(artifacts) != 1:
        raise RuntimeError("Expected exactly one native library test executable")
    listing = subprocess.run([str(artifacts[0]), "rooms::", "--list"], stdout=subprocess.PIPE, text=True, check=True).stdout
    expected = {line.removesuffix(": test") for line in listing.splitlines() if line.endswith(": test")}
    if not expected:
        raise RuntimeError("No room crypto tests were discovered")
    return artifacts[0].read_bytes(), expected


def main():
    data, expected = build()
    digest = hashlib.sha256(data).hexdigest()
    results = []
    report = ROOT / "tmp" / f"native-crypto-{time.time_ns()}.json"
    report.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    try:
        for peer in declared_peers():
            guest = Guest(peer["name"])
            path = "/usr/local/bin/kk-crypto-tests"
            print("Native MLS tests: " + peer["name"], flush=True)
            guest.upload(path, data)
            guest.run("chmod", "0755", path)
            assert guest.run("sha256sum", path)["out"].split()[0] == digest
            outcome = guest.run(path, "rooms::", "--nocapture", timeout=90, check=False)
            record = {"guest": peer["name"], "kernel": guest.run("uname", "-sr")["out"].strip(),
                      "result": outcome}
            results.append(record)
            print(outcome["out"], flush=True)
            if outcome["err"]:
                print(outcome["err"], file=sys.stderr, flush=True)
            executed = set(re.findall(r"^test ([\w:]+) \.\.\. ok$", outcome["out"], re.MULTILINE))
            if outcome["code"] or executed != expected:
                raise RuntimeError("Native room crypto tests did not all pass")
    finally:
        report.write_text(json.dumps({"scope": "Native per-OS crypto tests; no room network admission claim",
                                      "test_binary_sha256": digest, "expected_tests": sorted(expected), "results": results}, indent=2) + "\n")
        print(f"Evidence: {report}", flush=True)
    print(f"PASS: native room crypto tests on {len(results)} guests", flush=True)


if __name__ == "__main__":
    main()
