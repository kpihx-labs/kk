"""Native fixture profiles must not inherit a shared checkout ancestor."""

import importlib.util
import inspect
import os
from pathlib import Path
import stat
import sys
import tempfile
import unittest
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
with mock.patch.object(sys, "path", [str(SCRIPTS), *sys.path]):
    SPEC = importlib.util.spec_from_file_location("native_media", SCRIPTS / "12-native-media-test.py")
    NATIVE = importlib.util.module_from_spec(SPEC)
    SPEC.loader.exec_module(NATIVE)


class NativeWorkspaceTests(unittest.TestCase):
    def test_workspace_is_owned_private_physical_and_not_beneath_shared_lab(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary).resolve()
            shared = base / "shared-lab"
            shared.mkdir(mode=0o775)
            shared.chmod(0o775)
            real_mkdtemp = tempfile.mkdtemp
            with mock.patch.object(NATIVE, "ROOT", shared), mock.patch.object(
                NATIVE.tempfile, "mkdtemp",
                side_effect=lambda **options: real_mkdtemp(dir=base, **options),
            ) as create:
                workspace = NATIVE.private_workspace()
            create.assert_called_once_with(prefix="kk-native-media-")
            self.assertFalse(workspace.is_relative_to(shared))
            self.assertEqual(workspace, workspace.resolve(strict=True))
            self.assertEqual(workspace.stat().st_uid, os.getuid())
            self.assertEqual(stat.S_IMODE(workspace.stat().st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(shared.stat().st_mode), 0o775)

    def test_native_driver_actually_uses_the_private_workspace(self):
        self.assertIn("directory = private_workspace()", inspect.getsource(NATIVE.main))


if __name__ == "__main__":
    unittest.main()
