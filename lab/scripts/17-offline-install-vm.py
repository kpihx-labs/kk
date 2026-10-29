#!/usr/bin/env python3
"""Exercise a trusted development installer with a public kit in one isolated guest."""

import argparse
import hashlib
import io
import json
from pathlib import Path
import re
import tarfile
import xml.etree.ElementTree as ET

from kk_guest import Guest, declared_peers, virsh

LAB = Path(__file__).resolve().parents[1]
MAX_PAYLOAD = 256 * 1024 * 1024


def checked_file(path, record=None):
    if not path.is_absolute() or path.is_symlink() or not path.is_file():
        raise ValueError("expected absolute ordinary trusted fixture input")
    if path.stat().st_size > MAX_PAYLOAD:
        raise ValueError("fixture input too large")
    data = path.read_bytes()
    if record is not None and (len(data) != record["bytes"]
                               or hashlib.sha256(data).hexdigest() != record["sha256"]):
        raise ValueError("prepared kit input changed")
    return data


def exercise(arguments):
    if not re.fullmatch(r"[0-9a-f]{64}", arguments.expected_binary_sha256):
        raise ValueError("expected binary SHA-256 must be explicit lowercase hex")
    peers = {peer["name"]: peer for peer in declared_peers()}
    if arguments.guest not in peers:
        raise ValueError("only a declared disposable guest may be used")
    definition = LAB / "xml" / (arguments.guest + ".xml")
    expected_uuid = ET.parse(definition).findtext("uuid")
    if not expected_uuid or virsh("domuuid", arguments.guest).strip() != expected_uuid:
        raise ValueError("guest UUID differs from its project definition")
    if virsh("domstate", arguments.guest).strip() != "running":
        raise ValueError("start the declared guest explicitly before this gate")
    record = json.loads((arguments.kit / "kit.json").read_text())
    if record["repository"] != "kpihx-labs/kk":
        raise ValueError("unexpected kit repository")
    files = {"tools/helper": (checked_file(arguments.helper), 0o500),
             "tools/gh": (checked_file(arguments.gh), 0o500)}
    for key in ("archive", "bundle", "trusted_root"):
        item = record[key]
        if Path(item["name"]).name != item["name"]:
            raise ValueError("non-basename kit member")
        files["inputs/" + key] = (checked_file(arguments.kit / item["name"], item), 0o400)
    if sum(len(data) for data, _ in files.values()) > MAX_PAYLOAD:
        raise ValueError("fixture payload too large")
    payload = io.BytesIO()
    with tarfile.open(fileobj=payload, mode="w:gz") as archive:
        for name, (data, mode) in files.items():
            entry = tarfile.TarInfo(name)
            entry.size, entry.mode = len(data), mode
            archive.addfile(entry, io.BytesIO(data))
    guest = Guest(arguments.guest)
    interfaces = [item for item in guest.interfaces() if item["name"] != "lo"]
    if len(interfaces) != 1 or interfaces[0]["hardware-address"].lower() != peers[arguments.guest]["mac"]:
        raise ValueError("guest network differs from its dedicated roster")
    interface = interfaces[0]["name"]
    before = json.loads(guest.run("/usr/sbin/ip", "-j", "link", "show", "dev", interface)["out"])[0]
    uid = int(guest.run("/usr/bin/id", "-u", "nobody")["out"])
    gid = int(guest.run("/usr/bin/id", "-g", "nobody")["out"])
    if uid == 0:
        raise ValueError("installer test must be unprivileged")
    directory = guest.run("/usr/bin/mktemp", "-d", "/tmp/kk-offline.XXXXXXXX")["out"].strip()
    if not re.fullmatch(r"/tmp/kk-offline\.[A-Za-z0-9]+", directory):
        raise ValueError("unexpected guest fixture directory")
    print("Uploading trusted fixture tools and still-packed public archive", flush=True)
    guest.upload(directory + "/payload.tgz", payload.getvalue())
    guest.run("/usr/bin/tar", "-xzf", directory + "/payload.tgz", "-C", directory)
    guest.run("/usr/bin/mkdir", "-m", "700", directory + "/home", directory + "/gh-config", directory + "/tmp")
    guest.run("/usr/bin/chown", "-R", f"{uid}:{gid}", directory)
    prefix = directory + "/prefix"
    environment = ["/usr/sbin/runuser", "-u", "nobody", "--", "/usr/bin/env",
                   "-u", "GH_TOKEN", "-u", "GITHUB_TOKEN", "-u", "GH_DEBUG",
                   "HOME=" + directory + "/home", "GH_CONFIG_DIR=" + directory + "/gh-config",
                   "TMPDIR=" + directory + "/tmp",
                   "GH_NO_UPDATE_NOTIFIER=1", "GH_PAGER=cat", "PAGER=cat",
                   "PATH=" + directory + "/tools:/usr/sbin:/usr/bin:/sbin:/bin"]
    installer = directory + "/tools/helper"
    trust = ["--bundle", directory + "/inputs/bundle", "--trusted-root", directory + "/inputs/trusted_root"]
    report = {"guest": arguments.guest, "uid": uid, "fixture": directory,
              "kit_source_commit": record["commit"], "operations": []}
    mac = peers[arguments.guest]["mac"]
    physical = virsh("domif-getlink", arguments.guest, mac).split()
    if len(physical) != 2 or physical[0].lower() != mac or physical[1] not in ("up", "down"):
        raise ValueError("unexpected hypervisor link state")
    try:
        virsh("domif-setlink", arguments.guest, mac, "down")
        if virsh("domif-getlink", arguments.guest, mac).split() != [mac, "down"]:
            raise RuntimeError("hypervisor link did not go down")
        guest.run("/usr/sbin/ip", "link", "set", "dev", interface, "down")
        isolated = json.loads(guest.run("/usr/sbin/ip", "-j", "link", "show", "dev", interface)["out"])[0]
        if "UP" in isolated["flags"]:
            raise RuntimeError("guest NIC did not go down")
        report["network_isolation"] = {"interface": interface, "flags": isolated["flags"],
                                       "other_interfaces": ["lo"], "hypervisor_link": "down"}
        install = ["install", "--version", record["version"], "--commit", record["commit"],
                   "--prefix", prefix, "--archive", directory + "/inputs/archive", *trust]
        rollback = ["rollback", "--version", record["version"], "--prefix", prefix, *trust]
        for name, command in (("fresh", install), ("reinstall", install), ("same-version rollback", rollback)):
            print("Offline operation: " + name, flush=True)
            result = guest.run(*environment, installer, *command, timeout=300, check=False)
            report["operations"].append({"operation": name, **result})
            if result["code"]:
                raise RuntimeError("offline operation failed: " + name)
        report["installed_sha256"] = guest.run("/usr/bin/sha256sum", prefix + "/bin/kk")["out"].split()[0]
        report["installed_version"] = guest.run(*environment, prefix + "/bin/kk", "--version")["out"].strip()
        if report["installed_sha256"] != arguments.expected_binary_sha256:
            raise RuntimeError("installed binary differs from the authenticated expected bytes")
        if report["installed_version"] != "kk " + record["version"]:
            raise RuntimeError("installed binary reports a different version")
        report["success"] = True
    finally:
        # A timeout/agent failure is not proof that the installer or its children
        # ended. Keep the virtual cable disconnected until external shutdown.
        if report.get("success"):
            if "UP" in before["flags"]:
                guest.run("/usr/sbin/ip", "link", "set", "dev", interface, "up")
            if physical[1] == "up":
                virsh("domif-setlink", arguments.guest, mac, "up")
        else:
            report["cleanup_required"] = "confirm guest shutdown before restoring connectivity"
        print(json.dumps(report, indent=2), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--guest", required=True)
    parser.add_argument("--kit", type=Path, required=True)
    parser.add_argument("--helper", type=Path, required=True)
    parser.add_argument("--gh", type=Path, required=True)
    parser.add_argument("--expected-binary-sha256", required=True)
    exercise(parser.parse_args())


if __name__ == "__main__":
    main()
