"""Out-of-band lab control over virtio, never the network under test."""

import base64
import json
import os
import subprocess
import time
from pathlib import Path, PurePosixPath
import xml.etree.ElementTree as ET

URI = "qemu:///system"


def selected_binary():
    """Replay an explicitly selected archive binary without replacing a build."""
    default = Path(__file__).resolve().parents[2] / "target/release/kk"
    return Path(os.environ.get("KK_TEST_BINARY", default)).resolve()


def virsh(*args, input=None):
    result = subprocess.run(["virsh", "-c", URI, *map(str, args)], input=input,
                            capture_output=True, text=True, check=False)
    if result.returncode:
        raise RuntimeError(f"virsh {' '.join(map(str, args))}: {result.stderr.strip()}")
    return result.stdout


def shutdown_guests(names):
    """An asynchronous shutdown can close its RPC before the reply arrives."""
    errors = []
    for name in names:
        if virsh("domstate", name).strip() == "running":
            try:
                print(virsh("shutdown", name, "--mode", "agent"), flush=True)
            except RuntimeError as error:
                errors.append(str(error))
    deadline = time.monotonic() + 60
    while any(virsh("domstate", name).strip() != "shut off" for name in names):
        if time.monotonic() >= deadline:
            raise TimeoutError("Guests did not shut down; refusing forced destruction. " + "; ".join(errors))
        time.sleep(0.2)
    for error in errors:
        print("Shutdown RPC error resolved by confirmed domain shut-off: " + error, flush=True)


def declared_peers():
    definition = Path(__file__).resolve().parents[1] / "xml/kk-isolated.xml"
    return [dict(host.attrib) for host in ET.parse(definition).findall("./ip/dhcp/host")]


class Guest:
    def __init__(self, name):
        self.name = name

    def agent(self, operation, **arguments):
        request = {"execute": operation}
        if arguments:
            request["arguments"] = arguments
        result = subprocess.run(
            ["virsh", "-c", URI, "qemu-agent-command", self.name, "--timeout", "5", json.dumps(request)],
            capture_output=True, text=True, timeout=10, check=False,
        )
        if result.returncode:
            raise RuntimeError(f"{self.name}: {operation}: {result.stderr.strip()}")
        response = json.loads(result.stdout)
        if "error" in response:
            raise RuntimeError(f"{self.name}: {operation}: {response['error']}")
        return response["return"]

    def start(self, *argv, input_data=None):
        # Every lab guest has the same explicit entrypoint; Fedora additionally
        # labels it for the supported SELinux domain transition. No name heuristics.
        path = "/usr/local/libexec/kk-lab-control"
        arguments = list(argv)
        options = {"capture-output": True}
        if input_data is not None:
            options["input-data"] = base64.b64encode(input_data).decode()
        response = self.agent("guest-exec", path=path, arg=arguments, **options)
        return response["pid"]

    def wait(self, pid, timeout=60):
        deadline = time.monotonic() + timeout
        # guest-exec is asynchronous: guest-exec-status is its required completion API.
        while time.monotonic() < deadline:
            status = self.agent("guest-exec-status", pid=pid)
            if status.get("exited"):
                return {
                    "code": status.get("exitcode", 128 + status.get("signal", 0)),
                    "out": base64.b64decode(status.get("out-data", "")).decode(errors="replace"),
                    "err": base64.b64decode(status.get("err-data", "")).decode(errors="replace"),
                }
            time.sleep(0.1)
        raise TimeoutError(f"{self.name}: guest process {pid} exceeded {timeout}s")

    def run(self, *argv, timeout=60, check=True):
        result = self.wait(self.start(*argv), timeout=timeout)
        if check and result["code"]:
            raise RuntimeError(f"{self.name}: {argv[0]}: {result}")
        return result

    def upload(self, destination, data):
        self.run("/usr/bin/truncate", "--size", "0", destination)
        for start in range(0, len(data), 48_000):
            pid = self.start("/usr/bin/dd", "of=" + destination, "oflag=append",
                             "conv=notrunc", "status=none", input_data=data[start:start + 48_000])
            result = self.wait(pid)
            if result["code"]:
                raise OSError(f"{self.name}: guest file upload failed: {result['err']}")

    def public_certificate(self, identity_directory):
        # Deliberately no arbitrary read/private-key export operation.
        path = str(PurePosixPath(identity_directory) / "certificate.der")
        encoded = self.run("/usr/bin/base64", "--wrap", "0", path)["out"]
        certificate = base64.b64decode(encoded)
        if len(certificate) > 16_384:
            raise ValueError("public certificate exceeds the K&K bound")
        return certificate

    def room_invitation(self, source):
        """Opaque intended delivery of one ephemeral TEST invite, never keys."""
        parts = PurePosixPath(source).parts
        run = parts[3].removeprefix("kk-room-") if len(parts) == 5 else ""
        if (len(parts) != 5 or parts[:3] != ("/", "var", "tmp")
                or not parts[3].startswith("kk-room-") or len(run) != 12
                or any(char not in "0123456789abcdef" for char in run)
                or parts[4] != "scoped-invitation.json"):
            raise ValueError("Only the explicit ephemeral room-test invitation is exportable")
        if self.run("test", "-L", source, check=False)["code"] == 0:
            raise ValueError("Invitation symlinks are not exportable")
        # Bound on the guest BEFORE output allocation. No identity/MLS key path
        # is accepted, and the opaque code bundle is never logged.
        result = self.run("/usr/bin/dd", "if=" + source, "bs=4097", "count=1", "status=none", check=False)
        if result["code"]:
            raise RuntimeError("Private invitation transfer failed (exit " + str(result["code"]) + ")")
        data = result["out"].encode()
        if len(data) > 4096:
            raise ValueError("Room invitation exceeds the private bundle bound")
        return data

    def interfaces(self):
        return self.agent("guest-network-get-interfaces")

    def configure_isolated_network(self, peer):
        """Restore the lab's volatile address after every boot, via virtio only."""
        if peer["name"] != self.name:
            raise ValueError("Guest/network roster identity mismatch")
        # Virtio-agent readiness precedes the networking service on some boots.
        # Starting the existing guest unit waits for its service activation;
        # it does not enable/install a host or guest service.
        self.run("systemctl", "start", "NetworkManager")
        matches = [item["name"] for item in self.interfaces()
                   if item.get("hardware-address") == peer["mac"]]
        if len(matches) != 1:
            raise RuntimeError(f"{self.name}: expected exactly one interface with MAC {peer['mac']}")
        device = matches[0]
        self.run("/usr/bin/nmcli", "device", "set", device, "managed", "no")
        self.run("ip", "address", "flush", "dev", device)
        self.run("ip", "link", "set", device, "up")
        self.run("ip", "address", "replace", peer["ip"] + "/24", "dev", device)
        for family in ["-4", "-6"]:
            self.run("ip", family, "route", "flush", "default")
            if json.loads(self.run("ip", family, "-j", "route", "show", "default")["out"]):
                raise RuntimeError(f"{self.name}: {family} default Internet route remains")
        peer["device"] = device
