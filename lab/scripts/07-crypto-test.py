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
    command = ["cargo", "test", "--offline", "--locked", "--release", "--jobs", "2", "--lib", "--test", "room_crash", "--no-run",
               "--message-format=json", "--manifest-path", str(ROOT.parent / "Cargo.toml")]
    result = subprocess.run(command, stdout=subprocess.PIPE, text=True, check=False)
    artifacts = []
    for line in result.stdout.splitlines():
        message = json.loads(line)
        if message.get("reason") == "compiler-message":
            print(message["message"].get("rendered", ""), file=sys.stderr, end="")
        if (message.get("reason") == "compiler-artifact" and message.get("profile", {}).get("test") and message.get("executable")):
            target = message["target"]
            if "lib" in target["kind"]:
                artifacts.append(("library", Path(message["executable"]), ["rooms::"]))
            elif target["name"] == "room_crash" and "test" in target["kind"]:
                artifacts.append(("process-crash", Path(message["executable"]), []))
    if result.returncode:
        raise SystemExit(result.returncode)
    if len(artifacts) != 2 or {name for name, _, _ in artifacts} != {"library", "process-crash"}:
        raise RuntimeError("Expected separate library and process-crash executables")
    result = []
    for name, path, filters in artifacts:
        listing = subprocess.run([str(path), *filters, "--list"], stdout=subprocess.PIPE, text=True, check=True).stdout
        expected = {line.removesuffix(": test") for line in listing.splitlines() if line.endswith(": test")}
        if not expected:
            raise RuntimeError("No room tests discovered in " + name)
        data = path.read_bytes()
        result.append({"name": name, "data": data, "filters": filters, "expected": expected,
                       "sha256": hashlib.sha256(data).hexdigest()})
    return result


def main():
    artifacts = build()
    results = []
    report = ROOT / "tmp" / f"native-crypto-{time.time_ns()}.json"
    report.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    try:
        for peer in declared_peers():
            guest = Guest(peer["name"])
            for artifact in artifacts:
                path = "/usr/local/bin/kk-crypto-" + artifact["name"]
                print("Native room tests: " + peer["name"] + " / " + artifact["name"], flush=True)
                guest.upload(path, artifact["data"])
                guest.run("chmod", "0755", path)
                assert guest.run("sha256sum", path)["out"].split()[0] == artifact["sha256"]
                outcome = guest.run(path, *artifact["filters"], "--nocapture", timeout=90, check=False)
                record = {"guest": peer["name"], "artifact": artifact["name"], "kernel": guest.run("uname", "-sr")["out"].strip(), "result": outcome}
                results.append(record)
                print(outcome["out"], flush=True)
                if outcome["err"]:
                    print(outcome["err"], file=sys.stderr, flush=True)
                executed = set(re.findall(r"^test ([\w:]+) \.\.\. ok$", outcome["out"], re.MULTILINE))
                if outcome["code"] or executed != artifact["expected"]:
                    raise RuntimeError("Native room crypto tests did not all pass")
    finally:
        report.write_text(json.dumps({"format_version": 2, "scope": "Native per-OS tests; process crash isolated; no room network admission claim",
                                      "artifacts": [{"name": item["name"], "sha256": item["sha256"], "expected_tests": sorted(item["expected"])} for item in artifacts],
                                      "results": results}, indent=2) + "\n")
        print(f"Evidence: {report}", flush=True)
    print(f"PASS: native room crypto tests on {len(declared_peers())} guests", flush=True)


if __name__ == "__main__":
    main()
