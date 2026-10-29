"""Executable byte binding over the trusted guest-agent control channel."""

import ast
from pathlib import Path
import sys
import unittest
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
with mock.patch.object(sys, "path", [str(SCRIPTS), *sys.path]):
    from kk_guest import Guest


class GuestExecutableTests(unittest.TestCase):
    def test_guest_digest_and_public_executable_path_must_both_match(self):
        guest = Guest("fixture")
        expected = "a" * 64
        with mock.patch.object(guest, "run", return_value={"out": expected + "  /usr/local/bin/kk\n"}) as run:
            self.assertEqual(guest.verify_executable(expected), expected)
        run.assert_called_once_with("/usr/bin/sha256sum", "/usr/local/bin/kk")

    def test_mismatch_missing_or_wrong_path_refuses(self):
        guest = Guest("fixture")
        expected = "a" * 64
        for output in ("", "b" * 64 + "  /usr/local/bin/kk", expected,
                       expected + "  /another/file", expected + "  /usr/local/bin/kk extra"):
            with self.subTest(output=output), mock.patch.object(guest, "run", return_value={"out": output}):
                with self.assertRaisesRegex(RuntimeError, "executable digest"):
                    guest.verify_executable(expected)

    def test_invalid_expected_digest_never_contacts_the_guest(self):
        guest = Guest("fixture")
        with mock.patch.object(guest, "run") as run:
            for expected in ("", "A" * 64, "a" * 63, "x" * 64):
                with self.assertRaises(ValueError):
                    guest.verify_executable(expected)
            run.assert_not_called()

    def test_deployment_checks_guest_bytes_before_chmod_and_execution(self):
        source = (SCRIPTS / "06-kk-test.py").read_text()
        tree = ast.parse(source)
        setup = ast.get_source_segment(source, next(node for node in ast.walk(tree)
                                                   if isinstance(node, ast.FunctionDef) and node.name == "setup"))
        self.assertIn("verify_executable", setup)
        self.assertLess(setup.index("verify_executable"), setup.index('guest.run("/usr/bin/chmod"'))
        self.assertLess(setup.index("verify_executable"), setup.index('guest.run(CLI, "--version")'))


if __name__ == "__main__":
    unittest.main()
