from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
NOTES = sorted(f"docs/notes/{p.name}" for p in (ROOT / "docs" / "notes").glob("*.md"))
DOCS = ["AGENTS.md", "crates/AGENTS.md"] + NOTES
ROOT_MAX_LINES = 80
ROOT_MAX_BYTES = 6 * 1024
ROOT_CLAUDE_MD = b"@AGENTS.md\n"

SPAN = re.compile(r"`([^`\n]+)`")
IMPORT = re.compile(r"^@(\S+)\s*$")
BARE_AT = re.compile(r"(?:^|\s)@\S")


class ManualMoveTests(unittest.TestCase):
    def read(self, name: str) -> str:
        path = ROOT / name
        self.assertTrue(path.is_file(), f"{name} is missing")
        return path.read_text(encoding="utf-8")

    def test_claude_md_is_one_import(self):
        self.assertEqual((ROOT / "CLAUDE.md").read_bytes(), ROOT_CLAUDE_MD)

    def test_root_imports_nothing_and_indexes_every_note(self):
        text = self.read("AGENTS.md")
        self.assertEqual([line for line in text.splitlines() if IMPORT.match(line)], [])
        self.assertIn("(apps/flutter/AGENTS.md)", text)
        self.assertIn("crates/AGENTS.md", text)
        self.assertGreater(len(NOTES), 0)
        self.assertEqual([n for n in NOTES if n not in text], [], "every docs/notes file has a line in the root index")

    def test_root_is_short(self):
        count = len(self.read("AGENTS.md").splitlines())
        self.assertLessEqual(count, ROOT_MAX_LINES, "the root AGENTS.md points to the layers; details go there")

    def test_root_is_small(self):
        size = len((ROOT / "AGENTS.md").read_bytes())
        self.assertLessEqual(size, ROOT_MAX_BYTES, "every Claude Code session loads the root AGENTS.md; details go in docs/notes/")

    def test_imports_resolve(self):
        for name in DOCS:
            for line in self.read(name).splitlines():
                m = IMPORT.match(line)
                if m:
                    target = (ROOT / name).parent / m.group(1)
                    self.assertTrue(target.is_file(), f"{name} imports {m.group(1)}, which doesn't exist")

    def test_no_stray_imports(self):
        stray = []
        for name in DOCS:
            fenced = False
            for no, line in enumerate(self.read(name).splitlines(), 1):
                if line.lstrip().startswith("```"):
                    fenced = not fenced
                    continue
                if fenced or IMPORT.match(line):
                    continue
                if BARE_AT.search(SPAN.sub(" ", line)):
                    stray.append(f"{name}:{no}")
        self.assertEqual(stray, [], "put the name in backticks, or make it a whole-line import")


if __name__ == "__main__":
    unittest.main()
