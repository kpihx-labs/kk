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

    def fake(self, argv, **options):
        output = self.arguments.output
        if argv[1:3] == ["release", "download"]:
            (output / self.arguments.asset).write_bytes(b"MOCK archive")
        elif argv[1:3] == ["attestation", "download"]:
            digest = hashlib.sha256(b"MOCK archive").hexdigest()
            (output / f"sha256:{digest}.jsonl").write_bytes(b"MOCK bundle")
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

    def test_invalid_selector_fails_before_network_or_directory_creation(self):
        for key, value in (("commit", "short"), ("version", "../escape"),
                           ("asset", "../archive.zip"), ("asset", "*.zip")):
            arguments = SimpleNamespace(**{**vars(self.arguments), key: value})
            with self.subTest(key=key, value=value), \
                    mock.patch.object(KIT.subprocess, "run") as run, self.assertRaises(ValueError):
                KIT.prepare(arguments)
            run.assert_not_called()
            self.assertFalse(arguments.output.exists())

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
