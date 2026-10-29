"""Keep the resource-contained UI workflow mandatory in every release check."""

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CASE = "ui::tests::ui_http_pairs_independent_devices_owns_foreground_rooms_and_redacts_sensitive_events"


def recipe(source, target):
    match = re.search(rf"^{re.escape(target)}:.*\n((?:\t.*\n)+)", source, re.MULTILINE)
    if match is None:
        raise AssertionError(f"missing executable target: {target}")
    return [line.strip() for line in match[1].splitlines()]


class WorkflowFixtureTests(unittest.TestCase):
    def test_exact_case_is_real_and_not_ignored(self):
        source = (ROOT / "src/ui/tests.rs").read_text()
        name = CASE.rsplit("::", 1)[1]
        pattern = rf"((?:#\[[^\n]*\]\n)+)async fn {re.escape(name)}\(\)"
        match = re.search(pattern, source)
        self.assertIsNotNone(match, "exact UI case or its test attributes are missing")
        self.assertIn("#[tokio::test", match[1])
        self.assertNotIn("#[ignore", match[1])

    def test_concurrent_batch_and_exact_workflow_each_run_once(self):
        source = (ROOT / "Makefile").read_text()
        self.assertIn(f"UI_WORKFLOW_TEST := {CASE}\n", source)
        self.assertEqual(recipe(source, "rust-test"), [
            "cargo test --offline --locked --all-targets -- --skip $(UI_WORKFLOW_TEST)",
            "$(MAKE) ui-workflow-check",
        ])
        self.assertEqual(recipe(source, "ui-workflow-check"), [
            "cargo test --offline --locked --lib $(UI_WORKFLOW_TEST) -- --exact --nocapture",
        ])
        self.assertIn("$(MAKE) rust-test", recipe(source, "check"))

    def test_both_ci_paths_preserve_the_mandatory_gate(self):
        github = (ROOT / ".github/workflows/check.yml").read_text()
        self.assertRegex(github, r"(?m)^\s*run: make check\s*$")
        self.assertIn("needs: [linux, lab-static, dependency-audit, sdk-boundaries, workflow-syntax]", github)
        gitlab = (ROOT / ".gitlab-ci.yml").read_text()
        self.assertIn("runuser -u kk-ci -- bash -o pipefail -c 'make rust-test 2>&1 | cat'", gitlab)

    def test_image_jobs_require_the_verified_hosted_container_runner(self):
        gitlab = (ROOT / ".gitlab-ci.yml").read_text()
        self.assertRegex(gitlab, r"(?m)^default:\n  tags:\n    - saas-linux-small-amd64\n")
        for job in ("rust-foundation", "browser-adapter", "release-integrity"):
            body = re.search(rf"(?ms)^{job}:\n(.*?)(?=^[A-Za-z][\w-]*:|\Z)", gitlab)
            self.assertIsNotNone(body, f"missing required job: {job}")
            self.assertIn("  image:", body[1])
            self.assertNotIn("  tags:", body[1], f"{job} overrides the container-runner contract")
            self.assertNotIn("  inherit:", body[1], f"{job} disables the runner contract")


if __name__ == "__main__":
    unittest.main()
