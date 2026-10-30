"""GitHub-active delivery keeps an explicit, validated GitLab synchronization path."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class DeliveryTests(unittest.TestCase):
    def invoke(self, *options, https=False):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            git = directory / "git"
            git.write_text('''#!/bin/sh
printf '%s\\n' "$*" >> "$PUSH_TRACE"
case "$1" in
  branch) printf 'main\\n' ;;
  remote) if [ "$TEST_HTTPS" = yes ] && [ "$3" = gitlab ]; then printf 'https://example.invalid/repo\\n'; else printf 'git@%s.com:owner/kk.git\\n' "$3"; fi ;;
  push) exit 0 ;;
  *) exit 4 ;;
esac
''')
            git.chmod(0o700)
            trace = directory / "trace"
            environment = {**os.environ, "PATH": str(directory) + os.pathsep + os.environ["PATH"],
                           "PUSH_TRACE": str(trace), "TEST_HTTPS": "yes" if https else "no"}
            environment.pop("PUSH_REMOTES", None)
            result = subprocess.run(["make", "--no-print-directory", "-f", str(ROOT / "Makefile"), "push", *options],
                                    cwd=directory, env=environment, capture_output=True, text=True, timeout=10, check=False)
            return result, trace.read_text().splitlines() if trace.exists() else []

    def test_default_push_neither_probes_nor_pushes_gitlab(self):
        result, calls = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, ["branch --show-current", "remote get-url github", "push github main"])

    def test_explicit_later_synchronization_uses_both_existing_ssh_remotes(self):
        result, calls = self.invoke("PUSH_REMOTES=github gitlab")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([call for call in calls if call.startswith("push ")], ["push github main", "push gitlab main"])

    def test_invalid_or_non_ssh_destinations_fail_before_any_push(self):
        for options, https in [(("PUSH_REMOTES=github origin",), False), (("PUSH_REMOTES=github gitlab",), True), (("PUSH_REMOTES=",), False)]:
            with self.subTest(options=options):
                result, calls = self.invoke(*options, https=https)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(any(call.startswith("push ") for call in calls), calls)


if __name__ == "__main__":
    unittest.main()
