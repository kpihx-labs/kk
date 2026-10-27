"""Current project guides, not imported upstream Markdown or historical logs."""
import re
import unittest
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
DOCS = [ROOT / name for name in ("README.md", "CONTRACT.md", "TODO.md", "CHANGELOG.md")]
DOCS += sorted((ROOT / "docs").glob("*.md"))
DOCS += [ROOT / name for name in (
    "lab/README.md", "lab/opaque-interop/README.md", "third_party/README.md",
    "third_party/overrides/README.md", "third_party/overrides/elliptic-curve-xmd/README.md",
)]
if (ROOT / "AGENTS.md").exists():
    DOCS.append(ROOT / "AGENTS.md")


def prose(text):
    fenced = False
    for line in text.splitlines():
        if line.startswith("```"):
            fenced = not fenced
        elif not fenced:
            yield line


def anchors(path):
    result = set()
    for line in prose(path.read_text()):
        match = re.match(r"^#{1,6} (.+)$", line)
        if match:
            result.add(re.sub(r"[^\w -]", "", match[1].lower()).replace(" ", "-"))
    return result


class DocumentationTests(unittest.TestCase):
    def test_relative_links_and_heading_targets_exist(self):
        for path in DOCS:
            for line in prose(path.read_text()):
                for target in re.findall(r"\[[^\]]*\]\(([^)]+)\)", line):
                    parsed = urlsplit(target)
                    if parsed.scheme:
                        continue
                    linked = (path.parent / unquote(parsed.path)).resolve() if parsed.path else path
                    with self.subTest(page=path.relative_to(ROOT), target=target):
                        self.assertTrue(linked.exists(), "broken relative link")
                        if parsed.fragment and linked.suffix == ".md":
                            self.assertIn(unquote(parsed.fragment), anchors(linked), "missing heading")

    def test_guides_have_one_title_and_stay_short(self):
        for path in DOCS:
            text = path.read_text()
            with self.subTest(page=path.relative_to(ROOT)):
                self.assertEqual(sum(line.startswith("# ") for line in prose(text)), 1)
                self.assertLessEqual(len(text.splitlines()), 120, "split purpose, not running updates")
                self.assertFalse(any(char in text for char in ("\u2013", "\u2014", "\u2026")))

    def test_table_blocks_have_headers_and_consistent_columns(self):
        for path in DOCS:
            block = []
            for line in [*prose(path.read_text()), ""]:
                if line.startswith("|"):
                    block.append(line)
                    continue
                if block:
                    with self.subTest(page=path.relative_to(ROOT), header=block[0]):
                        self.assertGreaterEqual(len(block), 2)
                        self.assertRegex(block[1], r"^\|[ :|-]+\|$")
                        sizes = {len(re.split(r"(?<!\\)\|", row)) for row in block}
                        self.assertEqual(len(sizes), 1, "malformed table columns")
                    block = []

    def test_current_wire_tokens_match_their_owners(self):
        contract = (ROOT / "CONTRACT.md").read_text()
        for source in ("src/config.rs", "src/files.rs"):
            token = re.search(r'const ALPN:.*?b"([^"]+)"', (ROOT / source).read_text())[1]
            self.assertIn(f"`{token}`", contract)
        for path in DOCS:
            self.assertNotRegex(path.read_text(), r"`kk(?:-file|-text)?/1`", str(path))

    def test_command_examples_do_not_restore_removed_surfaces(self):
        for path in DOCS:
            text = path.read_text()
            for block in re.findall(r"```sh\n(.*?)```", text, re.DOTALL):
                with self.subTest(page=path.relative_to(ROOT)):
                    self.assertNotRegex(block, r"\bkk\s+(?:listen|inspect)\b")
                    self.assertNotRegex(block, r"\bkk\s+file\s+(?:send|receive)\b")
                    self.assertNotIn("--message", block)


if __name__ == "__main__":
    unittest.main()
