#!/usr/bin/env python3
"""Exercise a trusted development installer with a public kit in one isolated guest."""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
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
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > limit:
            raise ValueError("fixture input too large or non-ordinary")
        with os.fdopen(descriptor, "rb", closefd=False) as source:
            data = source.read(limit + 1)
    finally:
        os.close(descriptor)
    if len(data) > limit:
        raise ValueError("fixture input too large")
    if record is not None and (len(data) != record["bytes"]
                               or hashlib.sha256(data).hexdigest() != record["sha256"]):
        raise ValueError("prepared kit input changed")
    return data


def kit_inputs(directory, prefix, manifest=None):
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
    record = json.loads(checked_file(directory / "kit.json", manifest, limit=1024 * 1024))
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
    if "bootstrap" in record:
        seen = set()
        for key in ("helper", "notices", "entrypoint"):
            for member in (key, key + "_bundle"):
                item = record["bootstrap"][member]
                name = item["name"]
                if (not isinstance(name, str) or Path(name).name != name
                        or name in ("", ".", "..") or name in seen):
                    raise ValueError("unsafe or duplicate bootstrap member")
                seen.add(name)
                limit = 16 * 1024 * 1024 if member.endswith("_bundle") else (
                    1024 * 1024 if member == "entrypoint" else 128 * 1024 * 1024)
                files[prefix + "/bootstrap/" + name] = (checked_file(directory / name, item, limit), 0o400)
            if record["bootstrap"][key + "_bundle"]["name"] != record["bootstrap"][key]["name"] + ".jsonl":
                raise ValueError("bootstrap bundle basename differs from prepared subject")
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
    files = {"tools/gh": (checked_file(arguments.gh), 0o500)}
    bootstrap = getattr(arguments, "bootstrap", None)
    if bootstrap is not None:
        files["tools/install.sh"] = (checked_file(bootstrap, limit=1024 * 1024), 0o400)
    files.update(inputs)
    previous_path = getattr(arguments, "previous_kit", None)
    previous_sha = getattr(arguments, "expected_previous_binary_sha256", None)
    previous = None
    previous_manifest = None
    link = record.get("previous_kit")
    if link is not None:
        if (not isinstance(link, dict) or link.get("directory") != "previous"
                or not isinstance(link.get("manifest"), dict) or link["manifest"].get("name") != "kit.json"):
            raise ValueError("invalid assembled previous-kit link")
        nested = arguments.kit / "previous"
        if nested.is_symlink() or (previous_path is not None and previous_path != nested):
            raise ValueError("previous kit differs from the assembled link")
        previous_path, previous_manifest = nested, link["manifest"]
    if previous_path is not None or previous_sha is not None:
        if previous_path is None or not isinstance(previous_sha, str) or not re.fullmatch(r"[0-9a-f]{64}", previous_sha):
            raise ValueError("previous kit and explicit previous binary SHA-256 must be supplied together")
        previous, previous_files = kit_inputs(previous_path, "previous", previous_manifest)
        if link is not None and (previous["version"] != link.get("version") or previous["commit"] != link.get("commit")):
            raise ValueError("previous kit selectors differ from the assembled link")
        if previous["version"] == record["version"] or previous_sha == arguments.expected_binary_sha256:
            raise ValueError("inter-version qualification requires distinct versions and binary bytes")
        files.update(previous_files)
    signed_bootstrap = bootstrap is not None and "bootstrap" in record and (previous is None or "bootstrap" in previous)
    authenticate_entrypoint = bootstrap is not None and "bootstrap" in record
    if authenticate_entrypoint and hashlib.sha256(files["tools/install.sh"][0]).hexdigest() != record["bootstrap"]["entrypoint"]["sha256"]:
        raise ValueError("selected bootstrap differs from the prepared current entrypoint")
    if not signed_bootstrap:
        helper = getattr(arguments, "helper", None)
        if helper is None:
            raise ValueError("a trusted --helper is required without complete signed bootstrap inputs")
        files["tools/helper"] = (checked_file(helper), 0o500)
    if bootstrap is not None:
        for source, expected in ((record, arguments.expected_binary_sha256), (previous, previous_sha)):
            if source and "bootstrap" in source and source["bootstrap"]["helper"]["sha256"] != expected:
                raise ValueError("bootstrap helper differs from the independently selected binary SHA-256")
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
                    "HOME=" + directory + "/home", "GH_CONFIG_DIR=" + directory + "/gh-config", "GH_HOST=github.com",
                   "TMPDIR=" + directory + "/tmp",
                   "GH_NO_UPDATE_NOTIFIER=1", "GH_PAGER=cat", "PAGER=cat",
                   "PATH=" + directory + "/tools:/usr/sbin:/usr/bin:/sbin:/bin"]
    installer = directory + "/tools/helper"
    report = {"guest": arguments.guest, "uid": uid, "fixture": directory,
                "kit_source_commit": record["commit"],
                "previous_source_commit": previous["commit"] if previous else None,
                "entrypoint": "signed local bootstrap" if signed_bootstrap else ("shell bootstrap" if bootstrap else "trusted helper"),
                "trusted_tool_sha256": {name: hashlib.sha256(data).hexdigest()
                                        for name, (data, _) in files.items() if name.startswith("tools/")},
                "verifications": [], "operations": []}
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
        if authenticate_entrypoint:
            result = guest.run(*environment, directory + "/tools/gh", "attestation", "verify",
                               directory + "/tools/install.sh", "--repo", "kpihx-labs/kk",
                               "--signer-workflow", "kpihx-labs/kk/.github/workflows/check.yml",
                               "--source-ref", "refs/tags/v" + record["version"], "--source-digest", record["commit"],
                               "--deny-self-hosted-runners", "--bundle", directory + "/inputs/bootstrap/install.sh.jsonl",
                               "--custom-trusted-root", directory + "/inputs/trusted_root", timeout=180, check=False)
            report["entrypoint_attestation"] = result
            if result["code"]:
                raise RuntimeError("current bootstrap entrypoint attestation failed")
        for source, namespace in ([(previous, "previous")] if previous else []) + [(record, "inputs")]:
            verification_prefix = directory + "/verify-" + namespace
            if guest.run("/usr/bin/test", "-e", verification_prefix, check=False)["code"] != 1:
                raise RuntimeError("verification prefix must initially be absent")
            command = ["--version", source["version"], "--commit", source["commit"],
                       "--prefix", verification_prefix, "--archive", directory + "/" + namespace + "/archive",
                       "--bundle", directory + "/" + namespace + "/bundle",
                       "--trusted-root", directory + "/" + namespace + "/trusted_root", "--verify-only"]
            if bootstrap is not None and "bootstrap" in source:
                executable = ["/bin/sh", directory + "/tools/install.sh"]
                command.extend(["--bootstrap-dir", directory + "/" + namespace + "/bootstrap"])
            else:
                executable = [installer, "install"]
            result = guest.run(*environment, *executable, *command, timeout=300, check=False)
            observed_verification = {"version": source["version"], **result}
            report["verifications"].append(observed_verification)
            if result["code"] or len(result["out"]) > 16384:
                raise RuntimeError("offline verify-only failed")
            verified = json.loads(result["out"])
            if (verified.get("activated") is not False or verified.get("version") != source["version"]
                    or verified.get("commit") != source["commit"]
                    or verified.get("archive_sha256") != source["archive"]["sha256"]):
                raise RuntimeError("verify-only result differs from the prepared archive selectors")
            if guest.run("/usr/bin/test", "-e", verification_prefix, check=False)["code"] != 1:
                raise RuntimeError("verify-only created or modified its absent prefix")
            observed_verification["result"] = verified
            observed_verification["prefix_remained_absent"] = True
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
            executable = [installer]
            if bootstrap is not None:
                # Exercise the actual shell entrypoint, not a reconstructed helper
                # invocation. Explicit local helper/archive/bundle/roots forbid
                # bootstrap downloads while the virtual cable is disconnected.
                command = command[1:]
                if operation == "rollback":
                    command.append("--rollback")
                if "bootstrap" in source:
                    command.extend(["--bootstrap-dir", directory + "/" + namespace + "/bootstrap"])
                    if operation == "rollback":
                        command.extend(["--commit", source["commit"]])
                else:
                    command.extend(["--helper", installer])
                executable = ["/bin/sh", directory + "/tools/install.sh"]
            print("Offline operation: " + name, flush=True)
            result = guest.run(*environment, *executable, *command, timeout=300, check=False)
            observed = {"operation": name, "expected_sha256": expected_sha, **result}
            report["operations"].append(observed)
            if result["code"]:
                raise RuntimeError("offline operation failed: " + name)
            observed["installed_sha256"] = guest.run("/usr/bin/sha256sum", prefix + "/bin/kk")["out"].split()[0]
            if observed["installed_sha256"] != expected_sha:
                raise RuntimeError("installed binary differs from the authenticated expected bytes")
            observed["installed_version"] = guest.run(*environment, prefix + "/bin/kk", "--version")["out"].strip()
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
    parser.add_argument("--helper", type=Path,
                        help="explicitly trusted native helper; optional with complete signed bootstrap kits and --bootstrap")
    parser.add_argument("--gh", type=Path, required=True)
    parser.add_argument("--bootstrap", type=Path,
                        help="optional explicitly trusted install.sh to exercise instead of calling the helper directly")
    parser.add_argument("--expected-binary-sha256", required=True)
    parser.add_argument("--previous-kit", type=Path)
    parser.add_argument("--expected-previous-binary-sha256")
    exercise(parser.parse_args())


if __name__ == "__main__":
    main()
