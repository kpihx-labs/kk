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


def checked_file(path, record=None, limit=MAX_PAYLOAD):
    """Read a bounded ordinary fixture file and optionally verify its prepared digest.

    Args:
        path (Path): Absolute non-symlink path to an explicitly selected public input.
        record (dict | None): Expected byte count and SHA-256, or None for trusted tools.
        limit (int): Maximum input size in bytes, checked before reading.
    Returns:
        bytes: The verified input; invalid paths, sizes or digests raise ValueError.
    Examples:
        checked_file(Path('/tmp/kk-fixture/marker'), limit=4) -> b'data'
        checked_file(Path('/tmp/kk-fixture/empty'), limit=4) -> b''
    """
    if not path.is_absolute() or path.is_symlink() or not path.is_file():
        raise ValueError("expected absolute ordinary trusted fixture input")
    if path.stat().st_size > limit:
        raise ValueError("fixture input too large")
    data = path.read_bytes()
    if record is not None and (len(data) != record["bytes"]
                               or hashlib.sha256(data).hexdigest() != record["sha256"]):
        raise ValueError("prepared kit input changed")
    return data


def kit_inputs(directory, prefix):
    """Load public kit metadata and digest-bound inputs before accessing a guest.

    Args:
        directory (Path): Absolute directory containing an already prepared public kit.
        prefix (str): Owned relative namespace inside the generated fixture payload.
    Returns:
        tuple[dict, dict]: Source metadata and relative names mapped to (bytes, mode).
    Examples:
        sorted(kit_inputs(Path('/tmp/kk-kit'), 'inputs')[1])
        -> ['inputs/archive', 'inputs/bundle', 'inputs/trusted_root']
        kit_inputs(Path('/tmp/kk-kit'), 'previous')[0]['repository']
        -> 'kpihx-labs/kk'
    """
    record = json.loads(checked_file(directory / "kit.json", limit=1024 * 1024))
    if not isinstance(record, dict) or record.get("repository") != "kpihx-labs/kk":
        raise ValueError("unexpected kit repository")
    for key, pattern in (("version", r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?"),
                         ("commit", r"[0-9a-f]{40}")):
        if not isinstance(record.get(key), str) or not re.fullmatch(pattern, record[key]):
            raise ValueError("invalid kit " + key)
    files = {}
    for key in ("archive", "bundle", "trusted_root"):
        item = record[key]
        if not isinstance(item["name"], str) or Path(item["name"]).name != item["name"]:
            raise ValueError("non-basename kit member")
        files[prefix + "/" + key] = (checked_file(directory / item["name"], item), 0o400)
    return record, files


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
    record, inputs = kit_inputs(arguments.kit, "inputs")
    files = {"tools/helper": (checked_file(arguments.helper), 0o500),
              "tools/gh": (checked_file(arguments.gh), 0o500)}
    files.update(inputs)
    previous_path = getattr(arguments, "previous_kit", None)
    previous_sha = getattr(arguments, "expected_previous_binary_sha256", None)
    previous = None
    if previous_path is not None or previous_sha is not None:
        if previous_path is None or not isinstance(previous_sha, str) or not re.fullmatch(r"[0-9a-f]{64}", previous_sha):
            raise ValueError("previous kit and explicit previous binary SHA-256 must be supplied together")
        previous, previous_files = kit_inputs(previous_path, "previous")
        if previous["version"] == record["version"] or previous_sha == arguments.expected_binary_sha256:
            raise ValueError("inter-version qualification requires distinct versions and binary bytes")
        files.update(previous_files)
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
    report = {"guest": arguments.guest, "uid": uid, "fixture": directory,
               "kit_source_commit": record["commit"],
               "previous_source_commit": previous["commit"] if previous else None,
               "operations": []}
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
        plan = [("fresh", "install", record, "inputs", arguments.expected_binary_sha256),
                ("reinstall", "install", record, "inputs", arguments.expected_binary_sha256),
                ("same-version rollback", "rollback", record, "inputs", arguments.expected_binary_sha256)]
        if previous:
            plan = [("fresh previous", "install", previous, "previous", previous_sha),
                    ("update current", "install", record, "inputs", arguments.expected_binary_sha256),
                    ("rollback previous", "rollback", previous, "previous", previous_sha),
                    ("return current", "rollback", record, "inputs", arguments.expected_binary_sha256)]
        for name, operation, source, namespace, expected_sha in plan:
            command = [operation, "--version", source["version"], "--prefix", prefix,
                       "--bundle", directory + "/" + namespace + "/bundle",
                       "--trusted-root", directory + "/" + namespace + "/trusted_root"]
            if operation == "install":
                command.extend(["--commit", source["commit"], "--archive", directory + "/" + namespace + "/archive"])
            print("Offline operation: " + name, flush=True)
            result = guest.run(*environment, installer, *command, timeout=300, check=False)
            observed = {"operation": name, "expected_sha256": expected_sha, **result}
            report["operations"].append(observed)
            if result["code"]:
                raise RuntimeError("offline operation failed: " + name)
            observed["installed_sha256"] = guest.run("/usr/bin/sha256sum", prefix + "/bin/kk")["out"].split()[0]
            observed["installed_version"] = guest.run(*environment, prefix + "/bin/kk", "--version")["out"].strip()
            if observed["installed_sha256"] != expected_sha:
                raise RuntimeError("installed binary differs from the authenticated expected bytes")
            if observed["installed_version"] != "kk " + source["version"]:
                raise RuntimeError("installed binary reports a different version")
        report["installed_sha256"] = observed["installed_sha256"]
        report["installed_version"] = observed["installed_version"]
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
    parser.add_argument("--previous-kit", type=Path)
    parser.add_argument("--expected-previous-binary-sha256")
    exercise(parser.parse_args())


if __name__ == "__main__":
    main()
