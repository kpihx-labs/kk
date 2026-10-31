"""Mocked bootstrap-boundary tests, not installer or provenance qualification."""
import importlib.util
import json
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
spec = importlib.util.spec_from_file_location("kk_release", ROOT / "scripts/release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


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
        self.environment["TMPDIR"] = str(self.directory)
        for tool in ("mktemp", "timeout", "grep", "wc", "chmod", "cp", "jq", "sha256sum"):
            (self.tools / tool).symlink_to(shutil.which(tool))

    def bootstrap_tools(self, reject="", download_failure=False):
        # MOCKED HTTPS/provenance. This fixture is not published-release evidence.
        self.log = self.directory / "events"
        self.payload = self.directory / "payload"
        self.payload.write_text('#!/bin/sh\nprintf "EXEC %s\\n" "$*" >> "$MOCK_LOG"\nexit 37\n')
        self.fixtures = self.directory / "fixtures"
        self.fixtures.mkdir(exist_ok=True)
        files = {"kk": (self.payload.read_bytes(), 0o755), "LICENSE": (b"fixture license", 0o644),
                 "THIRD-PARTY.json": (b'[]', 0o644)}
        for target in release.TARGETS:
            release.bootstrap_assets(target, "0.8.2", COMMIT, files, self.fixtures)
        self.environment.update(MOCK_LOG=str(self.log), MOCK_PAYLOAD=str(self.payload),
                                MOCK_FIXTURES=str(self.fixtures), MOCK_REDIRECT="",
                                MOCK_REJECT=reject, MOCK_DOWNLOAD_FAIL=str(int(download_failure)))
        self.tool("gh", '''
if [ "$3" = --help ]; then
    printf '%s\\n' --repo --signer-workflow --source-ref --source-digest --deny-self-hosted-runners --bundle --custom-trusted-root
    exit 0
fi
printf 'VERIFY %s\\n' "$*" >> "$MOCK_LOG"
[ "$GH_HOST" = github.com ] || exit 88
case "$3" in *"$MOCK_REJECT"*) [ -z "$MOCK_REJECT" ] || exit 9;; esac
exit 0
''')
        self.tool("curl", '''
printf 'DOWNLOAD %s\\n' "$*" >> "$MOCK_LOG"
[ "$MOCK_DOWNLOAD_FAIL" = 0 ] || exit 22
while [ "$#" -gt 0 ]; do
    if [ "$1" = --output ]; then output=$2; shift; fi
    url=$1
    shift
done
if [ -n "$MOCK_REDIRECT" ]; then
    case "$url" in https://github.com/*)
        printf '302\n%s\n' "$MOCK_REDIRECT"
        exit 0 ;;
    esac
fi
case "$output" in
    *.bin) cp "$MOCK_PAYLOAD" "$output" ;;
    *.json) cp "$MOCK_FIXTURES/${output##*/}" "$output" ;;
    *) exit 98 ;;
esac
printf '%s\n\n' "${MOCK_STATUS:-200}"
''')

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
        self.assertIn("verifies", result.stdout)

    def test_valid_request_downloads_both_subjects_before_helper_execution(self):
        self.bootstrap_tools()
        result = self.request("--prefix", str(self.directory / "space é prefix"))
        self.assertEqual(result.returncode, 37, result.stderr)
        events = self.log.read_text().splitlines()
        self.assertEqual([event.split()[0] for event in events], ["DOWNLOAD", "VERIFY", "DOWNLOAD", "VERIFY", "EXEC"])
        for event in (events[1], events[3]):
            for expected in ("--repo kpihx-labs/kk", "--signer-workflow kpihx-labs/kk/.github/workflows/check.yml",
                             "--source-ref refs/tags/v0.8.2", "--source-digest " + COMMIT, "--deny-self-hosted-runners"):
                self.assertIn(expected, event)
        self.assertIn("kk-v0.8.2-linux-x86_64-bootstrap.bin", events[0])
        self.assertIn("kk-v0.8.2-linux-x86_64-bootstrap-notices.json", events[2])
        self.assertIn("--proto =https --proto-redir =https", events[0])
        self.assertIn("--max-filesize 134217728", events[0])
        self.assertIn("Verified bound notice inventory:", result.stderr)
        self.assertFalse((self.directory / "space é prefix").exists())

    def test_authenticated_but_unbound_subjects_never_execute(self):
        self.bootstrap_tools()
        companion = self.fixtures / "kk-v0.8.2-linux-x86_64-bootstrap-notices.json"
        original = json.loads(companion.read_bytes())
        for field, value in (("version", "0.8.3"), ("commit", "b" * 40),
                             ("target", "aarch64-unknown-linux-gnu"), ("architecture", "aarch64"),
                             ("platform", "windows"), ("format", 2), ("notices_asset", "foreign.json"),
                             ("helper", {"asset": "other.bin", "sha256": original["helper"]["sha256"]}),
                             ("helper", {"asset": original["helper"]["asset"], "sha256": "0" * 64})):
            with self.subTest(field=field, value=value):
                companion.write_text(json.dumps({**original, field: value}))
                self.log.write_text("")
                result = self.request()
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(self.log.read_text().count("VERIFY"), 2)
                self.assertNotIn("EXEC", self.log.read_text())
        companion.write_text(json.dumps(original))
        self.payload.write_bytes(b"swapped but gh mock approves")
        self.assertEqual(self.request().returncode, 1)
        for helper in self.directory.glob("kk-bootstrap.*/*.bin"):
            self.assertEqual(helper.stat().st_mode & 0o111, 0)

    def test_malformed_authenticated_json_and_multiple_documents_are_refused(self):
        self.bootstrap_tools()
        companion = self.fixtures / "kk-v0.8.2-linux-x86_64-bootstrap-notices.json"
        original = companion.read_text()
        for data in ("{", "[]", original + original):
            companion.write_text(data)
            self.assertEqual(self.request().returncode, 1)
            self.assertNotIn("EXEC", self.log.read_text())

    def test_manual_redirect_allowlist_before_follow(self):
        self.bootstrap_tools()
        for host in ("release-assets.githubusercontent.com", "objects.githubusercontent.com"):
            self.environment["MOCK_REDIRECT"] = f"https://{host}/asset?fixture=not-a-secret"
            self.log.write_text("")
            result = self.request()
            self.assertEqual(result.returncode, 37, result.stderr)
            self.assertEqual(self.log.read_text().count("DOWNLOAD"), 4)
            self.assertNotIn("--location", self.log.read_text())
            self.assertNotIn("not-a-secret", result.stderr)

    def test_rejected_redirects_never_connect_to_next_host(self):
        self.bootstrap_tools()
        for url in ("http://github.com/a", "https://evil.example/a", "https://github.com.evil/a",
                    "https://github.com@evil.example/a", "https://user@github.com/a",
                    "https://github.com:443/a", "https://release-assets.githubusercontent.com:443/a",
                    "https://objects.githubusercontent.com.evil/a", "https://github.com/a\nsecret",
                    "https://github.com/" + "a" * 8192):
            with self.subTest(url=url[:100]):
                self.environment["MOCK_REDIRECT"] = url
                self.log.write_text("")
                result = self.request()
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(self.log.read_text().count("DOWNLOAD"), 1)
                self.assertNotIn("VERIFY", self.log.read_text())

    def test_redirect_loop_is_bounded(self):
        self.bootstrap_tools()
        self.environment["MOCK_REDIRECT"] = "https://github.com/loop"
        result = self.request()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(self.log.read_text().count("DOWNLOAD"), 6)

    def test_unexpected_http_status_never_verifies_or_executes(self):
        self.bootstrap_tools()
        for status in ("000", "204", "206", "304", "404", "500", "200 extra"):
            with self.subTest(status=status):
                self.environment["MOCK_STATUS"] = status
                self.log.write_text("")
                result = self.request()
                self.assertEqual(result.returncode, 1)
                self.assertEqual(self.log.read_text().count("DOWNLOAD"), 1)
                self.assertNotIn("VERIFY", self.log.read_text())

    def test_empty_redirect_is_refused(self):
        self.bootstrap_tools()
        self.environment["MOCK_STATUS"] = "302"
        result = self.request()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(self.log.read_text().count("DOWNLOAD"), 1)

    def test_missing_jq_is_explicit_before_network(self):
        self.bootstrap_tools()
        (self.tools / "jq").unlink()
        result = self.request()
        self.assertEqual(result.returncode, 1)
        self.assertIn("install jq separately", result.stderr)
        self.assertFalse(self.log.exists())

    def test_verification_failure_never_executes_helper(self):
        for reject in (".bin", "-notices.json"):
            with self.subTest(reject=reject):
                self.bootstrap_tools(reject=reject)
                self.log.write_text("")
                result = self.request()
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("provenance verification failed", result.stderr)
                self.assertNotIn("EXEC", self.log.read_text())
                for helper in self.directory.glob("kk-bootstrap.*/*.bin"):
                    self.assertEqual(helper.stat().st_mode & 0o111, 0)

    def test_download_failure_never_verifies_or_executes(self):
        self.bootstrap_tools(download_failure=True)
        self.assertEqual(self.request().returncode, 1)
        self.assertNotIn("VERIFY", self.log.read_text())
        self.assertNotIn("EXEC", self.log.read_text())

    def test_empty_download_never_verifies_or_executes(self):
        self.bootstrap_tools()
        self.payload.write_bytes(b"")
        self.assertEqual(self.request().returncode, 1)
        self.assertNotIn("VERIFY", self.log.read_text())
        self.assertNotIn("EXEC", self.log.read_text())

    def test_incapable_gh_fails_before_download(self):
        self.bootstrap_tools()
        self.tool("gh", "exit 0\n")
        result = self.request()
        self.assertEqual(result.returncode, 1)
        self.assertIn("gh lacks required policy flag", result.stderr)
        self.assertFalse(self.log.exists())

    def test_local_archive_and_bundle_are_not_opened_or_executed(self):
        self.bootstrap_tools()
        result = self.request("--archive", str(self.directory / "USB é/archive.zip"),
                              "--bundle", str(self.directory / "USB é/provenance.jsonl"))
        self.assertEqual(result.returncode, 37, result.stderr)
        self.assertIn("--archive " + str(self.directory / "USB é/archive.zip"), self.log.read_text())
        self.assertFalse((self.directory / "USB é").exists())

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

    def test_supported_cpu_selects_native_asset(self):
        self.bootstrap_tools()
        for machine in ("aarch64", "arm64"):
            self.host("Linux", machine)
            self.assertEqual(self.request().returncode, 37)
        self.assertIn("linux-aarch64-bootstrap.bin", self.log.read_text())

    def test_missing_gh_is_explicit(self):
        (self.tools / "gh").unlink()
        result = self.request()
        self.assertEqual(result.returncode, 1)
        self.assertIn("install gh", result.stderr)

    def test_missing_curl_only_matters_for_online_path(self):
        (self.tools / "curl").unlink()
        self.assertIn("install curl", self.request().stderr)
        self.assertIn("install curl", self.request("--archive", "/USB/archive.zip").stderr)

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

    def test_trusted_root_is_forwarded_opaquely_for_install_and_rollback(self):
        helper = self.directory / "trusted helper é"
        helper.write_text('#!/bin/sh\nprintf "%s\\n" "$@"\nexit 38\n')
        helper.chmod(0o755)
        root = "/USB space é/root;touch NOT_EXECUTED.jsonl"
        for rollback in (False, True):
            arguments = ["--version", "0.8.2", "--helper", str(helper),
                         "--bundle", "/USB/bundle", "--trusted-root", root]
            arguments += ["--rollback"] if rollback else ["--commit", COMMIT, "--archive", "/USB/archive.zip"]
            result = self.run_shell(*arguments)
            self.assertEqual(result.returncode, 38, result.stderr)
            self.assertEqual(result.stdout.splitlines()[-2:], ["--trusted-root", root])
        self.assertFalse((self.directory / "NOT_EXECUTED.jsonl").exists())

    def test_trusted_root_requires_bundle_and_pretrusted_helper_before_download(self):
        result = self.request("--trusted-root", "/USB/root.jsonl")
        self.assertIn("requires --bundle", result.stderr)
        result = self.request("--archive", "/USB/archive.zip", "--bundle", "/USB/bundle",
                              "--trusted-root", "/USB/root.jsonl")
        self.assertIn("explicitly trusted --helper", result.stderr)
        self.assertEqual(result.returncode, 1)

    def local_bootstrap(self):
        self.bootstrap_tools()
        for asset in list(self.fixtures.iterdir()):
            asset.chmod(0o400)
            (self.fixtures / (asset.name + ".jsonl")).write_bytes(b"mock signed subject bundle")
        roots = self.directory / "public roots.jsonl"
        roots.write_bytes(b"mock separately provisioned roots")
        return ["--bootstrap-dir", str(self.fixtures), "--archive", "/USB/archive.zip",
                "--bundle", "/USB/archive.jsonl", "--trusted-root", str(roots)]

    def test_local_signed_bootstrap_authenticates_snapshots_without_curl(self):
        arguments = self.local_bootstrap()
        (self.tools / "curl").unlink()
        result = self.request(*arguments)
        self.assertEqual(result.returncode, 37, result.stderr)
        events = self.log.read_text().splitlines()
        self.assertEqual([event.split()[0] for event in events], ["VERIFY", "VERIFY", "EXEC"])
        for event in events[:2]:
            self.assertIn("--bundle ", event)
            self.assertIn("--custom-trusted-root ", event)
            self.assertIn("kk-bootstrap.", event)
            self.assertNotIn(str(self.fixtures), event)
        self.assertIn("--archive /USB/archive.zip", events[-1])
        self.assertIn("--bundle /USB/archive.jsonl", events[-1])
        self.assertIn("--trusted-root ", events[-1])
        self.assertEqual((self.fixtures / "kk-v0.8.2-linux-x86_64-bootstrap.bin").stat().st_mode & 0o777, 0o400)

    def test_macos_bootstrap_binds_native_platform_and_cpu_without_linux_tools(self):
        self.bootstrap_tools()
        self.host("Darwin", "arm64")
        result = self.request()
        self.assertEqual(result.returncode, 37, result.stderr)
        self.assertIn("macos-aarch64-bootstrap.bin", self.log.read_text())
        # Authenticated Linux notices are not interchangeable with Mach-O notices.
        notices = self.fixtures / "kk-v0.8.2-macos-aarch64-bootstrap-notices.json"
        record = json.loads(notices.read_text())
        record["executable_format"] = "elf"
        notices.write_text(json.dumps(record))
        self.log.write_text("")
        result = self.request()
        self.assertEqual(result.returncode, 1)
        self.assertIn("binding mismatch", result.stderr)
        self.assertNotIn("EXEC", self.log.read_text())

    def test_local_signed_bootstrap_missing_bundle_never_verifies_or_executes(self):
        arguments = self.local_bootstrap()
        (self.fixtures / "kk-v0.8.2-linux-x86_64-bootstrap-notices.json.jsonl").unlink()
        result = self.request(*arguments)
        self.assertEqual(result.returncode, 1)
        self.assertFalse(self.log.exists())

    def test_local_signed_bootstrap_rejected_provenance_never_executes(self):
        arguments = self.local_bootstrap()
        subject = self.fixtures / "kk-v0.8.2-linux-x86_64-bootstrap.bin"
        subject.chmod(0o700)
        self.environment["MOCK_REJECT"] = ".bin"
        result = self.request(*arguments)
        self.assertEqual(result.returncode, 1)
        self.assertIn("provenance verification failed", result.stderr)
        self.assertNotIn("EXEC", self.log.read_text())
        self.assertEqual(subject.stat().st_mode & 0o777, 0o700)
        for copied in self.directory.glob("kk-bootstrap.*/*.bin"):
            self.assertEqual(copied.stat().st_mode & 0o111, 0)

    def test_local_signed_bootstrap_still_rejects_unbound_notices(self):
        arguments = self.local_bootstrap()
        notices = self.fixtures / "kk-v0.8.2-linux-x86_64-bootstrap-notices.json"
        notices.chmod(0o600)
        record = json.loads(notices.read_text())
        record["helper"]["sha256"] = "f" * 64
        notices.write_text(json.dumps(record))
        result = self.request(*arguments)
        self.assertEqual(result.returncode, 1)
        self.assertIn("binding mismatch", result.stderr)
        self.assertNotIn("EXEC", self.log.read_text())

    def test_local_signed_bootstrap_rejects_linked_subject_before_verification(self):
        arguments = self.local_bootstrap()
        subject = self.fixtures / "kk-v0.8.2-linux-x86_64-bootstrap.bin"
        subject.unlink()
        subject.symlink_to(self.payload)
        result = self.request(*arguments)
        self.assertEqual(result.returncode, 1)
        self.assertIn("ordinary files", result.stderr)
        self.assertFalse(self.log.exists())

    def test_local_signed_bootstrap_rejects_oversized_roots_before_verification(self):
        arguments = self.local_bootstrap()
        roots = Path(arguments[-1])
        roots.write_bytes(b"x" * (1024 * 1024 + 1))
        result = self.request(*arguments)
        self.assertEqual(result.returncode, 1)
        self.assertFalse(self.log.exists())

    def test_local_signed_bootstrap_requires_offline_verifier_flags(self):
        arguments = self.local_bootstrap()
        self.tool("gh", 'printf "%s\\n" --repo --signer-workflow --source-ref --source-digest --deny-self-hosted-runners\n')
        result = self.request(*arguments)
        self.assertEqual(result.returncode, 1)
        self.assertIn("gh lacks required policy flag: --bundle", result.stderr)
        self.assertFalse(self.log.exists())

    def test_local_signed_bootstrap_requires_complete_explicit_trust(self):
        for extra in ([], ["--helper", "/trusted/kk"], ["--rollback"]):
            arguments = ["--version", "0.8.2", "--commit", COMMIT, "--bootstrap-dir", str(self.directory), *extra]
            result = self.run_shell(*arguments)
            self.assertEqual(result.returncode, 1)
            self.assertIn("--bootstrap-dir", result.stderr)

    def test_local_signed_bootstrap_rollback_authenticates_helper_without_download(self):
        arguments = self.local_bootstrap()
        position = arguments.index("--archive")
        del arguments[position:position + 2]
        (self.tools / "curl").unlink()
        result = self.request(*arguments, "--rollback")
        self.assertEqual(result.returncode, 37, result.stderr)
        events = self.log.read_text().splitlines()
        self.assertEqual([line.split()[0] for line in events], ["VERIFY", "VERIFY", "EXEC"])
        self.assertIn("EXEC rollback --version 0.8.2", events[-1])
        self.assertNotIn("--archive", events[-1])
        self.assertNotIn("--commit", events[-1])
        self.assertIn("--bundle /USB/archive.jsonl", events[-1])
        self.assertIn("--trusted-root ", events[-1])

    def test_signed_bootstrap_verify_only_forwards_native_flag_after_both_subjects(self):
        arguments = self.local_bootstrap()
        (self.tools / "curl").unlink()
        result = self.request(*arguments, "--verify-only")
        self.assertEqual(result.returncode, 37, result.stderr)
        events = self.log.read_text().splitlines()
        self.assertEqual([line.split()[0] for line in events], ["VERIFY", "VERIFY", "EXEC"])
        self.assertIn("EXEC install --version 0.8.2", events[-1])
        self.assertIn("--verify-only", events[-1])
        self.assertIn("--archive /USB/archive.zip", events[-1])

    def test_online_verify_only_preserves_authentication_and_rejected_helper_has_no_fallback(self):
        self.bootstrap_tools()
        self.environment["MOCK_REJECT"] = ".bin"
        result = self.request("--verify-only")
        self.assertEqual(result.returncode, 1)
        self.assertNotIn("EXEC", self.log.read_text())
        self.environment.pop("MOCK_REJECT")
        self.log.write_text("")
        result = self.request("--verify-only")
        self.assertEqual(result.returncode, 37, result.stderr)
        events = self.log.read_text().splitlines()
        self.assertEqual(sum(line.startswith("EXEC") for line in events), 1)
        self.assertIn("--verify-only", events[-1])

    def test_verify_only_cannot_skip_bootstrap_authentication_or_select_rollback(self):
        self.bootstrap_tools()
        for extra in (("--helper", str(self.payload)), ("--rollback",)):
            result = self.request("--verify-only", *extra)
            self.assertEqual(result.returncode, 1)
            self.assertIn("--verify-only excludes" if "--rollback" in extra else "authenticates helper/notices", result.stderr)
        self.assertFalse(self.log.exists())


class EntrypointContractTests(unittest.TestCase):
    def test_shell_syntax(self):
        subprocess.run(["/bin/sh", "-n", str(SHELL)], check=True, timeout=10)

    def test_no_extraction_or_activation_commands(self):
        for path in (SHELL, POWERSHELL):
            text = path.read_text()
            for forbidden in ("Expand-Archive", "ExtractToDirectory", "Invoke-Expression",
                              "Start-Process", "tar -x", "unzip ", "ln -s", "Move-Item",
                              "Remove-Item"):
                self.assertNotIn(forbidden, text)

    def test_guide_is_bounded_and_explicit(self):
        text = (ROOT / "docs/installers.md").read_text()
        self.assertLessEqual(len(text.splitlines()), 120)
        self.assertLessEqual(len((ROOT / "docs/distribution.md").read_text().splitlines()), 120)
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
