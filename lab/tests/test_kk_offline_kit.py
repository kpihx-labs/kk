"""Mocked kit preparation boundaries, not real provenance or offline evidence."""

import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/16-offline-kit.py"
SPEC = importlib.util.spec_from_file_location("offline_kit", SCRIPT)
KIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(KIT)


class OfflineKitTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.gh = self.root / "trusted gh"
        self.gh.write_text("synthetic executable fixture")
        self.gh.chmod(0o700)
        self.arguments = SimpleNamespace(
            gh=self.gh, output=self.root / "kit", version="0.8.2", commit="a" * 40,
            asset="kk-v0.8.2-linux-x86_64.tar.gz",
        )
        for owner, name in ((KIT.resource, "setrlimit"), (KIT.os, "umask")):
            patch = mock.patch.object(owner, name)
            patch.start()
            self.addCleanup(patch.stop)
        self.staging = self.root / "local-staging"
        self.staging.mkdir()
        patch = mock.patch.object(KIT.tempfile, "mkdtemp", return_value=str(self.staging))
        patch.start()
        self.addCleanup(patch.stop)

    def fake(self, argv, **options):
        output = self.arguments.output
        if argv[1:3] == ["release", "download"]:
            name = argv[argv.index("--pattern") + 1]
            output = Path(argv[argv.index("--dir") + 1])
            (output / name).write_bytes(b"MOCK archive" if name == self.arguments.asset else name.encode())
        elif argv[1:3] == ["attestation", "download"]:
            digest = hashlib.sha256(Path(argv[3]).read_bytes()).hexdigest()
            (Path(options["cwd"]) / f"sha256:{digest}.jsonl").write_bytes(b"MOCK bundle")
        elif argv[1:3] == ["attestation", "trusted-root"]:
            options["stdout"].write(b"MOCK public roots")
        return subprocess.CompletedProcess(argv, 0)

    def test_local_inputs_are_bound_and_never_claim_isolation_or_installation(self):
        with mock.patch.object(KIT.subprocess, "run", side_effect=self.fake) as run, \
                contextlib.redirect_stdout(io.StringIO()):
            KIT.prepare(self.arguments)
        record = json.loads((self.arguments.output / "kit.json").read_text())
        self.assertEqual(record["network_isolation"], "not tested")
        self.assertEqual(record["installation"], "not run")
        verify = run.call_args.args[0]
        for selector in ("--bundle", "--custom-trusted-root", "--source-digest",
                         "--source-ref", "--signer-workflow", "--deny-self-hosted-runners"):
            self.assertIn(selector, verify)
        self.assertIn("refs/tags/v0.8.2", verify)
        self.assertIn("a" * 40, verify)
        self.assertEqual(record["bundle"]["name"], "attestation.jsonl")
        self.assertEqual((self.arguments.output / "attestation.jsonl").read_bytes(), b"MOCK bundle")
        self.assertTrue(all(":" not in path.name for path in self.arguments.output.iterdir()))
        download = next(call for call in run.call_args_list
                        if call.args[0][1:3] == ["attestation", "download"])
        self.assertEqual(download.kwargs["cwd"], self.staging)

    def test_bundle_export_does_not_overwrite_an_unexpected_file(self):
        def collide(argv, **options):
            result = self.fake(argv, **options)
            if argv[1:3] == ["attestation", "download"]:
                (self.arguments.output / "attestation.jsonl").write_bytes(b"keep")
            return result

        with mock.patch.object(KIT.subprocess, "run", side_effect=collide), \
                self.assertRaises(FileExistsError):
            KIT.prepare(self.arguments)
        self.assertEqual((self.arguments.output / "attestation.jsonl").read_bytes(), b"keep")
        self.assertFalse((self.arguments.output / "kit.json").exists())

    def test_invalid_selector_fails_before_network_or_directory_creation(self):
        for key, value in (("commit", "short"), ("version", "../escape"),
                           ("asset", "../archive.zip"), ("asset", "*.zip")):
            arguments = SimpleNamespace(**{**vars(self.arguments), key: value})
            with self.subTest(key=key, value=value), \
                    mock.patch.object(KIT.subprocess, "run") as run, self.assertRaises(ValueError):
                KIT.prepare(arguments)
            run.assert_not_called()
            self.assertFalse(arguments.output.exists())

    def test_complete_bootstrap_kit_authenticates_every_subject_with_portable_sidecars(self):
        self.arguments.with_bootstrap = True
        with mock.patch.object(KIT.subprocess, "run", side_effect=self.fake) as run, \
                contextlib.redirect_stdout(io.StringIO()):
            KIT.prepare(self.arguments)
        record = json.loads((self.arguments.output / "kit.json").read_text())
        bootstrap = record["bootstrap"]
        for key in ("helper", "notices", "entrypoint"):
            subject = bootstrap[key]
            bundle = bootstrap[key + "_bundle"]
            self.assertEqual(bundle["name"], subject["name"] + ".jsonl")
            self.assertEqual(hashlib.sha256((self.arguments.output / subject["name"]).read_bytes()).hexdigest(),
                             subject["sha256"])
        verifies = [call.args[0] for call in run.call_args_list
                    if call.args[0][1:3] == ["attestation", "verify"]]
        self.assertEqual(len(verifies), 4)
        for argv in verifies:
            self.assertIn("--deny-self-hosted-runners", argv)
            self.assertIn("--bundle", argv)
            self.assertEqual(argv[argv.index("--custom-trusted-root") + 1],
                             str(self.arguments.output / "trusted-root.jsonl"))
        self.assertTrue(all(":" not in path.name for path in self.arguments.output.iterdir()))
        self.assertEqual(record["installation"], "not run")

    def test_bootstrap_requires_a_qualified_binary_basename_before_download(self):
        self.arguments.with_bootstrap = True
        self.arguments.asset = "kk-v0.8.2-source-offline.tar.gz"
        with mock.patch.object(KIT.subprocess, "run") as run, self.assertRaises(ValueError):
            KIT.prepare(self.arguments)
        run.assert_not_called()
        self.assertFalse(self.arguments.output.exists())

    def test_two_version_assembly_binds_both_exact_selectors_and_child_manifest(self):
        self.arguments.with_bootstrap = True
        self.arguments.previous_version = "0.8.1"
        self.arguments.previous_commit = "b" * 40
        with mock.patch.object(KIT.subprocess, "run", side_effect=self.fake) as run, \
                contextlib.redirect_stdout(io.StringIO()) as output:
            KIT.prepare(self.arguments)
        record = json.loads(output.getvalue())
        previous = record["previous_kit"]
        child = self.arguments.output / previous["directory"]
        manifest = (child / "kit.json").read_bytes()
        self.assertEqual(hashlib.sha256(manifest).hexdigest(), previous["manifest"]["sha256"])
        old = json.loads(manifest)
        self.assertEqual((old["version"], old["commit"]), ("0.8.1", "b" * 40))
        self.assertNotIn("previous_kit", old)
        self.assertEqual(set(old["bootstrap"]), {"helper", "helper_bundle", "notices", "notices_bundle", "entrypoint", "entrypoint_bundle"})
        from test_kk_offline_vm import VM
        loaded, current_files = VM.kit_inputs(self.arguments.output, "inputs")
        prior, previous_files = VM.kit_inputs(child, "previous", previous["manifest"])
        self.assertEqual(loaded["previous_kit"], previous)
        self.assertEqual(prior["commit"], old["commit"])
        self.assertIn("inputs/bootstrap/install.sh.jsonl", current_files)
        self.assertIn("previous/bootstrap/kk-v0.8.1-linux-x86_64-bootstrap.bin", previous_files)
        verifies = [call.args[0] for call in run.call_args_list if call.args[0][1:3] == ["attestation", "verify"]]
        self.assertEqual(len(verifies), 8)
        for command in verifies:
            commit = "b" * 40 if str(child) in command[3] else "a" * 40
            self.assertEqual(command[command.index("--source-digest") + 1], commit)
            self.assertIn("--bundle", command)
            self.assertIn("--custom-trusted-root", command)

    def test_invalid_previous_selection_never_starts_network_or_creates_output(self):
        self.arguments.with_bootstrap = True
        for version, commit in (("0.8.1", None), ("0.8.2", "b" * 40), ("bad/version", "b" * 40), ("0.8.1", "bad")):
            self.arguments.previous_version, self.arguments.previous_commit = version, commit
            with self.subTest(version=version, commit=commit), mock.patch.object(KIT.subprocess, "run") as run, self.assertRaises(ValueError):
                KIT.prepare(self.arguments)
            run.assert_not_called()
            self.assertFalse(self.arguments.output.exists())

    def test_existing_kit_is_not_overwritten(self):
        self.arguments.output.mkdir()
        sentinel = self.arguments.output / "keep"
        sentinel.write_bytes(b"existing")
        with mock.patch.object(KIT.subprocess, "run") as run, self.assertRaises(FileExistsError):
            KIT.prepare(self.arguments)
        run.assert_not_called()
        self.assertEqual(sentinel.read_bytes(), b"existing")

    def test_failed_verification_retains_inputs_without_success_record(self):
        def fail(argv, **options):
            if argv[1:3] == ["attestation", "verify"]:
                raise subprocess.CalledProcessError(1, argv)
            return self.fake(argv, **options)

        with mock.patch.object(KIT.subprocess, "run", side_effect=fail), \
                self.assertRaises(subprocess.CalledProcessError):
            KIT.prepare(self.arguments)
        self.assertTrue((self.arguments.output / self.arguments.asset).is_file())
        self.assertTrue((self.arguments.output / "trusted-root.jsonl").is_file())
        self.assertFalse((self.arguments.output / "kit.json").exists())


if __name__ == "__main__":
    unittest.main()
