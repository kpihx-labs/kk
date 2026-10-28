"""Offline guards for the explicitly scoped disposable ceremony importer."""

import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("file_fixture", ROOT / "lab/scripts/13-ui-file-fixture.py")
FIXTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURE)


class CeremonyFixtureTests(unittest.TestCase):
    def setUp(self):
        (ROOT / "target").mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="kk-file-fixture-", dir=ROOT / "target")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.path = self.root / "kk/profiles/fixture-user/ui/ceremony-Test12/invitation.json"
        current = self.root
        for part in self.path.relative_to(self.root).parts[:-1]:
            current /= part
            current.mkdir(mode=0o700)
        self.path.write_bytes(b"synthetic fixture")
        self.path.chmod(0o600)

    def accepted(self):
        return FIXTURE.disposable_ceremony(self.path, self.root)

    def test_exact_owned_private_generated_file_and_size_boundary(self):
        self.assertTrue(self.accepted())
        self.path.write_bytes(b"x" * 4096)
        self.assertTrue(self.accepted())
        self.path.write_bytes(b"x" * 4097)
        self.assertFalse(self.accepted())

    def test_unknown_names_and_private_key_paths_are_not_allowed(self):
        for candidate in [self.path.with_name("private-key.der"), self.root / "kk/profiles/fixture-user/identity/private-key.der", self.path.parent.with_name("unknown") / "invitation.json"]:
            self.assertFalse(FIXTURE.disposable_ceremony(candidate, self.root))

    def test_readable_file_or_parent_modes_are_not_normalized(self):
        self.path.chmod(0o640)
        self.assertFalse(self.accepted())
        self.assertEqual(self.path.stat().st_mode & 0o7777, 0o640)
        self.path.chmod(0o600)
        self.path.parent.chmod(0o750)
        self.assertFalse(self.accepted())

    def test_hardlinks_and_foreign_ownership_are_refused(self):
        alias = self.root / "retained"
        os.link(self.path, alias)
        self.assertFalse(self.accepted())
        alias.unlink()
        with patch.object(FIXTURE.os, "getuid", return_value=os.getuid() + 1000):
            self.assertFalse(self.accepted())

    def test_symlink_parent_or_file_is_refused(self):
        directory = self.path.parent
        retained = directory.with_name("retained")
        directory.rename(retained)
        directory.symlink_to(retained, target_is_directory=True)
        self.assertFalse(self.accepted())
        directory.unlink()
        retained.rename(directory)
        file = self.path.with_name("retained.json")
        self.path.rename(file)
        self.path.symlink_to(file)
        self.assertFalse(self.accepted())

    def test_other_trees_are_not_disposable_project_ceremonies(self):
        with tempfile.TemporaryDirectory() as outside:
            self.assertFalse(FIXTURE.disposable_ceremony(self.path, Path(outside)))
        self.assertFalse(FIXTURE.disposable_ceremony(self.root / "../outside", self.root))


if __name__ == "__main__":
    unittest.main()
