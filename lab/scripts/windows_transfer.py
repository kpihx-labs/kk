"""In-memory public source snapshot; transfer via virtio only, no host SDK."""
import hashlib
import io
import json
from pathlib import Path
import tarfile

from kk_windows_guest import WindowsGuest


def main():
    root = Path(__file__).resolve().parents[2]
    buffer = io.BytesIO()
    count = 0

    def accept(info):
        nonlocal count
        parts = Path(info.name).parts
        if any(part in {".git", "target", "__pycache__", ".ruff_cache"} for part in parts):
            return None
        if info.issym() or info.islnk() or not (info.isfile() or info.isdir()):
            raise ValueError("Nonordinary public source member refused")
        if any(part == ".env" or part.startswith(".env.") for part in parts):
            raise ValueError("Environment files are not qualification inputs")
        info.uid = info.gid = 0
        info.uname = info.gname = ""
        if info.isfile():
            count += 1
        return info

    members = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "LICENSE", "README.md", "CONTRACT.md", "src", "tests", "third_party/overrides", "scripts/install.ps1"]
    with tarfile.open(fileobj=buffer, mode="w:gz", compresslevel=3) as archive:
        for member in members:
            archive.add(root / member, arcname="kk/" + member, filter=accept)
    data = buffer.getvalue()
    digest = hashlib.sha256(data).hexdigest()
    print(json.dumps({"qualification_source_sha256": digest, "bytes": len(data), "files": count, "release_claim": False}), flush=True)
    guest = WindowsGuest("win11")
    setup = guest.powershell(r"""
$ErrorActionPreference='Stop'
$root='C:\kk-l4-native'
if (-not (Test-Path "$root\qualification-owner.txt")) { throw 'Owned workspace missing' }
if (Test-Path "$root\repo") {
    if (-not (Test-Path "$root\source.tar.gz")) { throw 'Previous source archive missing' }
    $previous=(Get-FileHash "$root\source.tar.gz" -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($previous -cnotmatch '^[0-9a-f]{64}$') { throw 'Previous source digest invalid' }
    $archive="$root\repo-$previous"
    if (Test-Path $archive) { throw 'Previous source snapshot already archived' }
    Move-Item -LiteralPath "$root\repo" -Destination $archive
    Move-Item -LiteralPath "$root\source.tar.gz" -Destination "$root\source-$previous.tar.gz"
}
""")
    if setup["code"]:
        raise RuntimeError(setup)
    guest.upload(r"C:\kk-l4-native\source.tar.gz", data, timeout=600)
    script = r"""
$ErrorActionPreference='Stop'
$root='C:\kk-l4-native'
$actual=(Get-FileHash "$root\source.tar.gz" -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -cne '@DIGEST@') { throw 'Source transfer digest mismatch' }
New-Item -ItemType Directory -Path "$root\repo" | Out-Null
& "$env:SystemRoot\System32\tar.exe" -xzf "$root\source.tar.gz" --strip-components 1 -C "$root\repo"
if ($LASTEXITCODE -ne 0) { throw 'Source extraction failed' }
Write-Output "SOURCE TRANSFER VERIFIED: $actual"
""".replace("@DIGEST@", digest)
    result = guest.powershell(script, timeout=180)
    print(result["out"], flush=True)
    if result["code"]:
        raise RuntimeError(result)


if __name__ == "__main__":
    main()
