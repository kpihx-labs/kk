"""Mocked bootstrap-boundary tests, not installer or provenance qualification."""
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SHELL = ROOT / "scripts/install.sh"
POWERSHELL = ROOT / "scripts/install.ps1"
COMMIT = "a" * 40


class ShellBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="kk installer Unicode é ")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.tools = self.directory / "tools"
        self.tools.mkdir()
        self.host("Linux", "x86_64")
        for tool in ("gh", "curl"):
            self.tool(tool, "exit 99\n")  # Discovery only: executing either must not occur.
        self.environment = {**os.environ, "PATH": str(self.tools), "HOME": str(self.directory)}

    def tool(self, name, body):
        path = self.tools / name
        path.write_text("#!/bin/sh\n" + body)
        path.chmod(0o755)

    def host(self, system, machine):
        self.tool("uname", f'case "$1" in -s) printf "%s\\n" "{system}";; '
                  f'-m) printf "%s\\n" "{machine}";; *) exit 99;; esac\n')

    def run_shell(self, *arguments):
        return subprocess.run(["/bin/sh", str(SHELL), *arguments], env=self.environment,
                              capture_output=True, text=True, timeout=10, check=False)

    def request(self, *arguments):
        return self.run_shell("--version", "0.8.2", "--commit", COMMIT, *arguments)

    def test_help_without_tools(self):
        result = self.run_shell("--help")
        self.assertEqual(result.returncode, 0)
        self.assertIn("BLOCKED", result.stdout)

    def test_valid_request_is_blocked_without_writes_or_tool_execution(self):
        before = sorted(self.directory.rglob("*"))
        result = self.request("--prefix", str(self.directory / "space é prefix"))
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn("Nothing was downloaded", result.stderr)
        self.assertEqual(before, sorted(self.directory.rglob("*")))

    def test_local_archive_and_bundle_are_not_opened_or_executed(self):
        result = self.request("--archive", str(self.directory / "USB é/archive.zip"),
                              "--bundle", str(self.directory / "USB é/provenance.jsonl"))
        self.assertEqual(result.returncode, 3, result.stderr)

    def test_invalid_selectors(self):
        for version in ("", "v0.8.2", "0.8", "0..2", "00.8.2", "0.8.2;id", "0.8.2/xx"):
            with self.subTest(version=version):
                self.assertEqual(self.run_shell("--version", version, "--commit", COMMIT).returncode, 1)
        for commit in ("abc", "A" * 40, "g" * 40, "a" * 41):
            with self.subTest(commit=commit):
                self.assertEqual(self.run_shell("--version", "0.8.2", "--commit", commit).returncode, 1)

    def test_invalid_arguments(self):
        for arguments in (("--version",), ("--unknown",), ("--prefix", "relative"),
                          ("--bundle", "/bundle"), ("--version", "0.8.2"),
                          ("--archive", "--commit")):
            with self.subTest(arguments=arguments):
                self.assertEqual(self.request(*arguments).returncode, 1)

    def test_unsupported_hosts(self):
        for system, machine in (("Darwin", "arm64"), ("MINGW64_NT", "x86_64"),
                                ("Linux", "riscv64")):
            with self.subTest(system=system, machine=machine):
                self.host(system, machine)
                self.assertEqual(self.request().returncode, 1)

    def test_supported_cpu_detection_still_blocks(self):
        for machine in ("aarch64", "arm64"):
            self.host("Linux", machine)
            self.assertEqual(self.request().returncode, 3)

    def test_missing_gh_is_explicit(self):
        (self.tools / "gh").unlink()
        result = self.request()
        self.assertEqual(result.returncode, 1)
        self.assertIn("install gh", result.stderr)

    def test_missing_curl_only_matters_for_online_path(self):
        (self.tools / "curl").unlink()
        self.assertIn("install curl", self.request().stderr)
        self.assertEqual(self.request("--archive", "/USB/archive.zip").returncode, 3)

    def test_explicit_trusted_helper_argv_is_not_shell_evaluated(self):
        helper = self.directory / "trusted helper é"
        helper.write_text('#!/bin/sh\n[ "$#" -eq 11 ] || exit 91\n'
                          '[ "$1" = install ] && [ "$8" = --archive ] || exit 92\n'
                          '[ "$9" = "/USB space é/archive;touch NOT_EXECUTED.zip" ] || exit 93\n'
                          'exit 37\n')
        helper.chmod(0o755)
        result = self.request("--helper", str(helper), "--archive",
                              "/USB space é/archive;touch NOT_EXECUTED.zip", "--bundle", "/USB/bundle")
        self.assertEqual(result.returncode, 37, result.stderr)
        self.assertFalse((self.directory / "NOT_EXECUTED.zip").exists())

    def test_explicit_trusted_helper_rollback_dispatch(self):
        helper = self.directory / "trusted helper é"
        helper.write_text('#!/bin/sh\n[ "$#" -eq 5 ] && [ "$1" = rollback ] || exit 92\nexit 38\n')
        helper.chmod(0o755)
        result = self.run_shell("--version", "0.8.2", "--helper", str(helper), "--rollback")
        self.assertEqual(result.returncode, 38, result.stderr)

    def test_untrusted_implicit_and_link_helper_are_refused(self):
        self.assertEqual(self.request("--helper", "relative").returncode, 1)
        link = self.directory / "linked helper"
        link.symlink_to(self.tools / "gh")
        self.assertEqual(self.request("--helper", str(link)).returncode, 1)


class EntrypointContractTests(unittest.TestCase):
    def test_shell_syntax(self):
        subprocess.run(["/bin/sh", "-n", str(SHELL)], check=True, timeout=10)

    def test_no_extraction_or_activation_commands(self):
        for path in (SHELL, POWERSHELL):
            text = path.read_text()
            for forbidden in ("Expand-Archive", "ExtractToDirectory", "Invoke-Expression",
                              "Start-Process", "tar -x", "unzip ", "ln -s", "Move-Item",
                              "Remove-Item", "gh attestation verify"):
                self.assertNotIn(forbidden, text)

    def test_guide_is_bounded_and_explicit(self):
        text = (ROOT / "docs/installers.md").read_text()
        self.assertLessEqual(len(text.splitlines()), 120)
        self.assertIn("BEFORE chmod/execution", text)
        self.assertIn("MOCKED gh/curl", text)

    @unittest.skipUnless(shutil.which("pwsh"), "pwsh unavailable; no PowerShell execution evidence")
    def test_powershell_refuses_installation(self):
        result = subprocess.run([shutil.which("pwsh"), "-NoProfile", "-File", str(POWERSHELL),
                                 "-Version", "0.8.2", "-Commit", COMMIT],
                                capture_output=True, text=True, check=False, timeout=20)
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn("not shipped", result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
