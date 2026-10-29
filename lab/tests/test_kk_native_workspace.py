"""Native fixture security and real TCP-listener allocation remain explicit."""

import ast
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

    def test_tcp_invitation_port_is_allocated_by_the_actual_listener(self):
        tree = ast.parse(inspect.getsource(NATIVE.main))
        invite = next(node.value for node in ast.walk(tree)
                      if isinstance(node, ast.Assign)
                      and any(isinstance(target, ast.Name) and target.id == "inviter"
                              for target in node.targets))
        position = next(index for index, argument in enumerate(invite.args)
                        if isinstance(argument, ast.Constant) and argument.value == "--bind")
        binding = invite.args[position + 1]
        self.assertIsInstance(binding, ast.JoinedStr)
        self.assertEqual([part.value for part in binding.values if isinstance(part, ast.Constant)], [":0"])
        self.assertEqual(sum(isinstance(part, ast.FormattedValue) for part in binding.values), 1)


if __name__ == "__main__":
    unittest.main()
