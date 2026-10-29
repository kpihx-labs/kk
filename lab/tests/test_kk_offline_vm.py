"""Mocked VM gate boundaries; these tests do not qualify an offline install."""

import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
with mock.patch.object(sys, "path", [str(SCRIPTS), *sys.path]):
    SPEC = importlib.util.spec_from_file_location("offline_vm", SCRIPTS / "17-offline-install-vm.py")
    VM = importlib.util.module_from_spec(SPEC)
    SPEC.loader.exec_module(VM)


class OfflineVmTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.arguments = SimpleNamespace(
            guest="fedora-44", kit=self.root, helper=self.root / "helper", gh=self.root / "gh",
            expected_binary_sha256="a" * 64,
        )
        self.arguments.helper.write_bytes(b"mock helper")
        self.arguments.gh.write_bytes(b"mock gh")
        record = {"repository": "kpihx-labs/kk", "version": "0.8.2", "commit": "b" * 40}
        for key in ("archive", "bundle", "trusted_root"):
            data = ("mock " + key).encode()
            (self.root / key).write_bytes(data)
            record[key] = {"name": key, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
        (self.root / "kit.json").write_text(json.dumps(record))
        self.guest = mock.Mock()
        peer = next(peer for peer in VM.declared_peers() if peer["name"] == "fedora-44")
        self.mac = peer["mac"]
        self.guest.interfaces.return_value = [{"name": "lo"}, {"name": "eth0", "hardware-address": peer["mac"]}]
        self.up = True
        self.host_up = True
        self.operations = []
        self.fail_operation = False
        self.timeout_operation = False
        self.guest.run.side_effect = self.run_guest

    def run_guest(self, *argv, **options):
        out, code = "", 0
        if argv[:2] == ("/usr/sbin/ip", "-j"):
            out = json.dumps([{"flags": ["UP"] if self.up else []}])
        elif argv[:2] == ("/usr/sbin/ip", "link"):
            self.up = argv[-1] == "up"
        elif argv[0] == "/usr/bin/id":
            out = "65534"
        elif argv[0] == "/usr/bin/mktemp":
            out = "/tmp/kk-offline.TEST"
        elif argv[0] == "/usr/bin/sha256sum":
            out = "a" * 64 + "  kk"
        elif argv[-1] == "--version":
            out = "kk 0.8.2"
        elif argv[0] == "/usr/sbin/runuser":
            self.assertFalse(self.up, "installer must run with NIC down")
            self.assertFalse(self.host_up, "hypervisor must disconnect the virtual cable")
            self.operations.append(argv)
            if self.timeout_operation:
                raise TimeoutError("mock status deadline; process may remain alive")
            code = int(self.fail_operation)
        return {"out": out, "err": "", "code": code}

    def exercise(self):
        uuid = VM.ET.parse(SCRIPTS.parent / "xml/fedora-44.xml").findtext("uuid")

        def virsh(action, *_arguments):
            if action == "domuuid":
                return uuid
            if action == "domstate":
                return "running"
            if action == "domif-getlink":
                return self.mac + (" up" if self.host_up else " down")
            if action == "domif-setlink":
                self.host_up = _arguments[-1] == "up"
                return ""
            raise AssertionError("unexpected hypervisor command")

        with mock.patch.object(VM, "Guest", return_value=self.guest), \
                mock.patch.object(VM, "virsh", side_effect=virsh), \
                contextlib.redirect_stdout(io.StringIO()) as output:
            VM.exercise(self.arguments)
        return output.getvalue()

    def test_three_operations_run_isolated_and_restore_interface(self):
        output = self.exercise()
        self.assertEqual(len(self.operations), 3)
        self.assertTrue(self.up)
        self.assertTrue(self.host_up)
        self.assertIn('"success": true', output)

    def test_operation_failure_keeps_isolation_and_stops(self):
        self.fail_operation = True
        with self.assertRaisesRegex(RuntimeError, "offline operation failed"):
            self.exercise()
        self.assertEqual(len(self.operations), 1)
        self.assertFalse(self.up)
        self.assertFalse(self.host_up)

    def test_timeout_never_reconnects_a_possibly_live_process(self):
        self.timeout_operation = True
        with self.assertRaises(TimeoutError):
            self.exercise()
        self.assertFalse(self.up)
        self.assertFalse(self.host_up)

    def test_wrong_installed_bytes_cannot_pass(self):
        self.arguments.expected_binary_sha256 = "c" * 64
        with self.assertRaisesRegex(RuntimeError, "authenticated expected bytes"):
            self.exercise()
        self.assertFalse(self.up)
        self.assertFalse(self.host_up)

    def test_wrong_domain_uuid_fails_before_guest_access(self):
        with mock.patch.object(VM, "virsh", return_value="wrong"), \
                mock.patch.object(VM, "Guest") as guest, self.assertRaisesRegex(ValueError, "UUID"):
            VM.exercise(self.arguments)
        guest.assert_not_called()

    def test_changed_kit_fails_before_guest_access(self):
        (self.root / "archive").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "kit input changed"):
            self.exercise()
        self.guest.interfaces.assert_not_called()


if __name__ == "__main__":
    unittest.main()
