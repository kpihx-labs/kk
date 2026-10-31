"""Mocked VM gate boundaries; these tests do not qualify an offline install."""

import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tarfile
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
        self.verifications = []
        self.entrypoint_verifications = []
        self.fail_entrypoint = False
        self.observed_binary_hashes = {"0.8.2": "a" * 64, "0.8.1": "d" * 64}
        self.fail_verification = False
        self.verification_creates_prefix = False
        self.verification_done = False
        self.bad_verification_receipt = False
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
            out = self.observed_binary_hashes[self.installed_version] + "  kk"
        elif argv[0] == "/usr/bin/test":
            code = 0 if self.verification_creates_prefix and self.verification_done else 1
        elif argv[-1] == "--version":
            out = "kk " + self.installed_version
        elif argv[0] == "/usr/sbin/runuser":
            self.assertFalse(self.up, "installer must run with NIC down")
            self.assertFalse(self.host_up, "hypervisor must disconnect the virtual cable")
            if "attestation" in argv:
                self.entrypoint_verifications.append(argv)
                return {"code": int(self.fail_entrypoint), "out": "MOCK entrypoint attestation", "err": ""}
            if "--verify-only" in argv:
                self.verifications.append(argv)
                self.verification_done = True
                version = argv[argv.index("--version") + 1]
                directory = self.root / "previous" if version == "0.8.1" else self.root
                source = json.loads((directory / "kit.json").read_text())
                return {"code": int(self.fail_verification), "err": "", "out": json.dumps({
                    "activated": self.bad_verification_receipt, "version": version,
                    "commit": source["commit"], "target": "x86_64-unknown-linux-gnu",
                    "archive_sha256": source["archive"]["sha256"]})}
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

    def enable_signed_bootstrap(self, directory):
        record = json.loads((directory / "kit.json").read_text())
        stem = f"kk-v{record['version']}-linux-x86_64-bootstrap"
        record["bootstrap"] = {}
        for key, name in (("helper", stem + ".bin"), ("notices", stem + "-notices.json"), ("entrypoint", "install.sh")):
            for member, basename in ((key, name), (key + "_bundle", name + ".jsonl")):
                data = (record["version"] + member).encode()
                (directory / basename).write_bytes(data)
                record["bootstrap"][member] = {"name": basename, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
        (directory / "kit.json").write_text(json.dumps(record))
        expected = record["bootstrap"]["helper"]["sha256"]
        self.observed_binary_hashes[record["version"]] = expected
        if directory == self.root:
            self.arguments.expected_binary_sha256 = expected
        else:
            self.arguments.expected_previous_binary_sha256 = expected

    def assembled_signed_kit(self):
        self.enable_previous()
        self.enable_signed_bootstrap(self.root)
        self.enable_signed_bootstrap(self.root / "previous")
        previous = json.loads((self.root / "previous/kit.json").read_text())
        data = (self.root / "previous/kit.json").read_bytes()
        record = json.loads((self.root / "kit.json").read_text())
        record["previous_kit"] = {"directory": "previous", "version": previous["version"], "commit": previous["commit"],
                                  "manifest": {"name": "kit.json", "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}}
        (self.root / "kit.json").write_text(json.dumps(record))
        del self.arguments.previous_kit
        self.arguments.helper = None
        self.arguments.bootstrap = self.root / "install.sh"

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
        self.assertEqual(len(self.verifications), 1)
        self.assertTrue(self.up)
        self.assertTrue(self.host_up)
        self.assertIn('"success": true', output)

    def test_assembled_signed_kit_verifies_both_then_installs_updates_and_rolls_back(self):
        self.assembled_signed_kit()
        output = self.exercise()
        report = json.loads(output[output.index("{"):])
        self.assertEqual(report["entrypoint"], "signed local bootstrap")
        self.assertEqual(len(self.verifications), 2)
        self.assertEqual(len(self.entrypoint_verifications), 1)
        self.assertEqual(len(self.operations), 4)
        self.assertTrue(all(item["prefix_remained_absent"] for item in report["verifications"]))
        for index, command in enumerate(self.operations):
            namespace = "previous" if index in (0, 2) else "inputs"
            self.assertNotIn("--helper", command)
            self.assertEqual(command[command.index("--bootstrap-dir") + 1], f"/tmp/kk-offline.TEST/{namespace}/bootstrap")
            self.assertIn("--commit", command)
            self.assertEqual("--rollback" in command, index >= 2)
        for command in self.verifications:
            self.assertIn("--bootstrap-dir", command)
            self.assertNotIn("--helper", command)
        with tarfile.open(fileobj=io.BytesIO(self.guest.upload.call_args.args[1]), mode="r:gz") as archive:
            self.assertNotIn("tools/helper", archive.getnames())
            self.assertIn("previous/bootstrap/kk-v0.8.1-linux-x86_64-bootstrap.bin.jsonl", archive.getnames())

    def test_unbound_current_entrypoint_is_rejected_before_guest_access(self):
        self.assembled_signed_kit()
        alternate = self.root / "different-install.sh"
        alternate.write_bytes(b"different trusted script, not this kit's signed subject")
        self.arguments.bootstrap = alternate
        with self.assertRaisesRegex(ValueError, "prepared current entrypoint"):
            self.exercise()
        self.guest.interfaces.assert_not_called()

    def test_failed_entrypoint_attestation_precedes_all_helper_execution(self):
        self.assembled_signed_kit()
        self.fail_entrypoint = True
        with self.assertRaisesRegex(RuntimeError, "entrypoint attestation failed"):
            self.exercise()
        self.assertFalse(self.verifications)
        self.assertFalse(self.operations)
        self.assertFalse(self.host_up)

    def test_assembled_manifest_change_refuses_before_guest_access(self):
        self.assembled_signed_kit()
        path = self.root / "previous/kit.json"
        path.write_bytes(path.read_bytes() + b" ")
        with self.assertRaisesRegex(ValueError, "prepared kit input changed"):
            self.exercise()
        self.guest.interfaces.assert_not_called()

    def test_bootstrap_payload_change_refuses_before_guest_access(self):
        self.assembled_signed_kit()
        (self.root / "kk-v0.8.2-linux-x86_64-bootstrap.bin").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "prepared kit input changed"):
            self.exercise()
        self.guest.interfaces.assert_not_called()

    def test_verify_only_failure_does_not_install_or_restore_connectivity(self):
        self.fail_verification = True
        with self.assertRaisesRegex(RuntimeError, "verify-only failed"):
            self.exercise()
        self.assertFalse(self.operations)
        self.assertFalse(self.up)
        self.assertFalse(self.host_up)

    def test_verify_only_prefix_creation_cannot_be_mistaken_for_nonactivation(self):
        self.verification_creates_prefix = True
        with self.assertRaisesRegex(RuntimeError, "created or modified"):
            self.exercise()
        self.assertFalse(self.operations)
        self.assertFalse(self.host_up)

    def test_verify_only_must_return_an_exact_nonactivated_receipt(self):
        self.bad_verification_receipt = True
        with self.assertRaisesRegex(RuntimeError, "result differs"):
            self.exercise()
        self.assertFalse(self.operations)
        self.assertFalse(self.host_up)

    def test_shell_bootstrap_receives_all_local_inputs_for_each_version(self):
        self.enable_previous()
        self.arguments.bootstrap = self.root / "install.sh"
        self.arguments.bootstrap.write_bytes(b"mock shell bootstrap")
        output = self.exercise()
        report = json.loads(output[output.index("{"):])
        self.assertEqual(report["entrypoint"], "shell bootstrap")
        self.assertEqual(report["trusted_tool_sha256"]["tools/install.sh"],
                         hashlib.sha256(b"mock shell bootstrap").hexdigest())
        with tarfile.open(fileobj=io.BytesIO(self.guest.upload.call_args.args[1]), mode="r:gz") as archive:
            self.assertEqual(archive.extractfile("tools/install.sh").read(), b"mock shell bootstrap")
            self.assertEqual(archive.getmember("tools/install.sh").mode, 0o400)
        for index, command in enumerate(self.operations):
            self.assertIn("/bin/sh", command)
            self.assertIn("/tmp/kk-offline.TEST/tools/install.sh", command)
            self.assertEqual(command[command.index("--helper") + 1],
                             "/tmp/kk-offline.TEST/tools/helper")
            namespace = "previous" if index in (0, 2) else "inputs"
            for flag, member in (("--bundle", "bundle"), ("--trusted-root", "trusted_root")):
                self.assertEqual(command[command.index(flag) + 1],
                                 f"/tmp/kk-offline.TEST/{namespace}/{member}")
            self.assertEqual("--rollback" in command, index >= 2)
            self.assertEqual("--archive" in command, index < 2)

    def test_invalid_bootstrap_fails_before_guest_access(self):
        self.arguments.bootstrap = self.root / "missing.sh"
        with self.assertRaises(ValueError):
            self.exercise()
        self.guest.interfaces.assert_not_called()

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
        self.assertFalse(any(call.args[-1] == "--version" for call in self.guest.run.call_args_list))

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
