"""Transfer only the explicitly named public source delta, then one native gate."""
import argparse
import hashlib
import json
from pathlib import Path

from kk_windows_guest import WindowsGuest


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("paths", nargs="+")
    options = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    guest = WindowsGuest("win11")
    expected = {}
    for name in options.paths:
        path = Path(name)
        if path.is_absolute() or ".." in path.parts or path.parts[0] not in {"src", "tests", "third_party"}:
            raise ValueError("Only an explicit public project source delta is accepted")
        source = root / path
        if source.is_symlink() or not source.is_file():
            raise ValueError("Source delta must be a regular file")
        data = source.read_bytes()
        expected[name] = hashlib.sha256(data).hexdigest()
        guest.upload("C:\\kk-l4-native\\repo\\" + name.replace("/", "\\"), data)
    print("EXACT SOURCE DELTA: " + json.dumps(expected, sort_keys=True), flush=True)
    checks = ["$ErrorActionPreference='Stop'"]
    for name, digest in expected.items():
        target = "C:\\kk-l4-native\\repo\\" + name.replace("/", "\\")
        checks.append(f"if ((Get-FileHash -LiteralPath '{target}' -Algorithm SHA256).Hash.ToLowerInvariant() -cne '{digest}') {{ throw 'Source delta mismatch' }}")
    checks.append("Write-Output 'SOURCE DELTA VERIFIED IN GUEST'")
    result = guest.powershell("\n".join(checks))
    print(result["out"], flush=True)
    if result["code"]:
        raise RuntimeError(result)
    result = guest.powershell((root / "lab/scripts/windows_native_targeted.ps1").read_text(), timeout=7200)
    print(result["out"], flush=True)
    print(result["err"], flush=True)
    raise SystemExit(result["code"])


if __name__ == "__main__":
    main()
