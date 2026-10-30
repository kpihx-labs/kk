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
import uuid

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
        self.lab = self.root / "lab"
        (self.lab / "xml").mkdir(parents=True)
        self.uuid = str(uuid.uuid4())
        self.definition = self.lab / "xml/fedora-44.xml"
        self.definition.write_text(f"<domain><uuid>{self.uuid}</uuid></domain>")
        lab = mock.patch.object(VM, "LAB", self.lab)
        lab.start()
        self.addCleanup(lab.stop)
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
        self.installed_version = "0.8.2"
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
            out = ("d" if self.installed_version == "0.8.1" else "a") * 64 + "  kk"
        elif argv[-1] == "--version":
            out = "kk " + self.installed_version
        elif argv[0] == "/usr/sbin/runuser":
            self.assertFalse(self.up, "installer must run with NIC down")
            self.assertFalse(self.host_up, "hypervisor must disconnect the virtual cable")
            self.operations.append(argv)
            if self.timeout_operation:
                raise TimeoutError("mock status deadline; process may remain alive")
            code = int(self.fail_operation)
            if not code:
                self.installed_version = argv[argv.index("--version") + 1]
        return {"out": out, "err": "", "code": code}

    def enable_previous(self):
        previous = self.root / "previous"
        previous.mkdir()
        record = json.loads((self.root / "kit.json").read_text())
        record.update(version="0.8.1", commit="c" * 40)
        for key in ("archive", "bundle", "trusted_root"):
            (previous / key).write_bytes((self.root / key).read_bytes())
        (previous / "kit.json").write_text(json.dumps(record))
        self.arguments.previous_kit = previous
        self.arguments.expected_previous_binary_sha256 = "d" * 64

    def exercise(self):
        def virsh(action, *_arguments):
            if action == "domuuid":
                return self.uuid
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

    def test_missing_definition_refuses_before_any_guest_access(self):
        with mock.patch.object(VM, "LAB", self.root / "unprovisioned"), \
                mock.patch.object(VM, "Guest") as guest, \
                mock.patch.object(VM, "virsh") as virsh, self.assertRaises(FileNotFoundError):
            VM.exercise(self.arguments)
        guest.assert_not_called()
        virsh.assert_not_called()

    def test_inter_version_update_and_both_rollbacks_verify_each_exact_binary(self):
        self.enable_previous()
        output = self.exercise()
        report = json.loads(output[output.index("{"):])
        self.assertEqual([command[command.index("--version") + 1] for command in self.operations],
                         ["0.8.1", "0.8.2", "0.8.1", "0.8.2"])
        self.assertEqual([entry["installed_sha256"] for entry in report["operations"]],
                         ["d" * 64, "a" * 64, "d" * 64, "a" * 64])
        for command, namespace in zip(self.operations, ["previous", "inputs", "previous", "inputs"]):
            self.assertTrue(command[command.index("--bundle") + 1].endswith("/" + namespace + "/bundle"))
        self.assertEqual(report["previous_source_commit"], "c" * 40)
        self.assertTrue(self.up and self.host_up)

    def test_incorrect_intermediate_update_stops_before_rollback_and_keeps_isolation(self):
        self.enable_previous()
        def wrong_update(*argv, **options):
            result = self.run_guest(*argv, **options)
            if argv[0] == "/usr/bin/sha256sum" and len(self.operations) == 2:
                result["out"] = "d" * 64 + "  kk"
            return result
        self.guest.run.side_effect = wrong_update
        with self.assertRaisesRegex(RuntimeError, "authenticated expected bytes"):
            self.exercise()
        self.assertEqual(len(self.operations), 2)
        self.assertFalse(self.up or self.host_up)

    def test_previous_kit_requires_explicit_independent_byte_oracle(self):
        self.enable_previous()
        self.arguments.expected_previous_binary_sha256 = None
        with self.assertRaisesRegex(ValueError, "supplied together"):
            self.exercise()
        self.guest.interfaces.assert_not_called()

    def test_same_version_cannot_be_labeled_inter_version_qualification(self):
        self.enable_previous()
        self.arguments.previous_kit = self.root
        with self.assertRaisesRegex(ValueError, "distinct versions"):
            self.exercise()
        self.guest.interfaces.assert_not_called()

    def test_changed_previous_kit_is_rejected_before_guest_access(self):
        self.enable_previous()
        (self.arguments.previous_kit / "archive").write_bytes(b"changed previous")
        with self.assertRaisesRegex(ValueError, "kit input changed"):
            self.exercise()
        self.guest.interfaces.assert_not_called()


if __name__ == "__main__":
    unittest.main()
