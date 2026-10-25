"""Scoped environment for distro tools and the local libguestfs appliance."""

import os
import platform
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ASSET_ROOT = ROOT.parent.parent / "vm-lab"


def environment(assets=None):
    result = os.environ.copy()
    interpreter_bin = Path(sys.executable).parent.resolve()
    # Preserve the normal PATH (including trash-safe rm), excluding UV's Python.
    result["PATH"] = os.pathsep.join(
        item for item in result.get("PATH", os.defpath).split(os.pathsep)
        if item and Path(item).resolve() != interpreter_bin
    )
    result["LIBGUESTFS_BACKEND"] = "direct"
    for variable, folder in [("LIBGUESTFS_CACHEDIR", "guestfs-cache"),
                             ("LIBGUESTFS_TMPDIR", "guestfs-tmp"), ("TMPDIR", "guestfs-tmp")]:
        directory = ROOT / "tmp" / folder
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        result[variable] = str(directory)
    kernel = (assets or ASSET_ROOT) / "boot/host-kernel/boot" / f"vmlinuz-{platform.release()}"
    if kernel.is_file():
        result["SUPERMIN_KERNEL"] = str(kernel)
        result["SUPERMIN_MODULES"] = f"/lib/modules/{platform.release()}"
    return result
