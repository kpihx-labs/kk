"""Explicit Windows lab execution over the existing local virtio agent channel."""
import argparse
import base64
import json
import time
import re
import tomllib
import subprocess
from pathlib import Path, PureWindowsPath

from kk_guest import Guest


class WindowsGuest(Guest):
    def powershell(self, script, timeout=60):
        if "@KK_RUST_CHANNEL@" in script:
            pin = Path(__file__).resolve().parents[2] / "rust-toolchain.toml"
            channel = tomllib.loads(pin.read_text())["toolchain"]["channel"]
            if not re.fullmatch(r"[A-Za-z0-9.-]+", channel):
                raise ValueError("Invalid repository Rust channel")
            script = script.replace("@KK_RUST_CHANNEL@", channel)
        if len(script.encode("utf-16-le")) > 20_000:
            raise ValueError("Lab PowerShell exceeds its command-line bound")
        encoded = base64.b64encode(script.encode("utf-16-le")).decode("ascii")
        pid = self.start(
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            "-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded,
        )
        print(f"Guest process {pid}; required completion API; deadline {timeout}s", flush=True)
        result = self.wait(pid, timeout=timeout)
        print("HOST MEMORY AFTER GUEST STAGE:", flush=True)
        subprocess.run(["free", "-m"], check=True)
        return result

    def upload(self, destination, data, timeout=300):
        """Write only explicit public qualification inputs into an owned run area."""
        path = PureWindowsPath(destination)
        if (not path.is_absolute() or path.drive.upper() != "C:"
                or len(path.parts) < 3 or not path.parts[1].startswith("kk-l4-")):
            raise ValueError("Upload must stay inside an explicit C:\\kk-l4-* run area")
        if len(data) > 128 * 1024 * 1024:
            raise ValueError("Qualification upload exceeds its bound")
        handle = self.agent("guest-file-open", path=str(path), mode="wb")
        deadline = time.monotonic() + timeout
        offset = 0
        try:
            while offset < len(data):
                if time.monotonic() >= deadline:
                    raise TimeoutError("Guest input transfer exceeded its deadline")
                chunk = data[offset:offset + 48_000]
                result = self.agent("guest-file-write", handle=handle, **{
                    "buf-b64": base64.b64encode(chunk).decode("ascii"),
                })
                count = result.get("count", 0)
                if not 0 < count <= len(chunk):
                    raise OSError("Guest input transfer made invalid progress")
                offset += count
        finally:
            self.agent("guest-file-close", handle=handle)

    def start(self, *argv, input_data=None):
        if not argv or not PureWindowsPath(argv[0]).is_absolute():
            raise ValueError("Windows execution requires an explicit absolute executable")
        options = {"capture-output": True}
        if input_data is not None:
            options["input-data"] = base64.b64encode(input_data).decode("ascii")
        return self.agent("guest-exec", path=argv[0], arg=list(argv[1:]), **options)["pid"]

    def agent(self, operation, **arguments):
        status = super().agent(operation, **arguments)
        if operation == "guest-exec-status" and (
            status.get("out-truncated") or status.get("err-truncated")
        ):
            raise RuntimeError("Guest output was truncated; refusing incomplete evidence")
        return status


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--domain", required=True)
    parser.add_argument("--script", type=Path, required=True)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--seconds", type=int, default=60)
    options = parser.parse_args()
    if not 1 <= options.seconds <= 7200:
        parser.error("seconds must be between 1 and 7200")
    guest = WindowsGuest(options.domain)
    script = options.script.read_text(encoding="utf-8-sig")
    result = guest.powershell(script, timeout=options.seconds)
    if options.report is not None:
        options.report.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(result["out"], end="", flush=True)
    if result["err"]:
        print("GUEST STDERR:\n" + result["err"], flush=True)
    print(f"Guest exit: {result['code']}", flush=True)
    return 0 if result["code"] == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
