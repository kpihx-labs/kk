"""Out-of-band lab control over virtio, never the network under test."""

import base64
import json
import subprocess
import time
from pathlib import Path, PurePosixPath
import xml.etree.ElementTree as ET

URI = "qemu:///system"


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

    def interfaces(self):
        return self.agent("guest-network-get-interfaces")
