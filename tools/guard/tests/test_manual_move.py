from __future__ import annotations

import re
import subprocess
import unittest
from pathlib import Path
from typing import Dict, List, Optional

ROOT = Path(__file__).resolve().parents[3]
OLD = Path(__file__).resolve().parent / "fixtures" / "notes-before-phase3.txt"
DOCS = ["AGENTS.md", "legacy/macos/Sources/AGENTS.md", "crates/AGENTS.md"]
LATER_DOCS = ["ARCHITECTURE.md"]
RETIRED: Dict[str, str] = {}
RENAMED = {"mobile" + "/": "apps/flutter/"}
ROOT_MAX_LINES = 80
ROOT_CLAUDE_MD = b"@AGENTS.md\n"

SPAN = re.compile(r"`([^`\n]+)`")
QUOTED = re.compile(r'"([^"\n]+)"')
NUMBER = re.compile(r"\d+(?:[.,:]\d+)*")
IMPORT = re.compile(r"^@(\S+)\s*$")
BARE_AT = re.compile(r"(?:^|\s)@\S")


def squash(text: str) -> str:
    return re.sub(r"\s+", " ", text)


def renamed(token: str) -> str:
    for old, new in RENAMED.items():
        if token.startswith(old):
            return new + token[len(old):]
    return token


def unique(items: List[str]) -> List[str]:
    return list(dict.fromkeys(items))


def git(*args: str) -> bytes:
    return subprocess.run(["git", "-C", str(ROOT), *args], check=True, capture_output=True).stdout


def old_from_history() -> Optional[bytes]:
    try:
        commits = git("log", "--format=%H", "--", "CLAUDE.md").decode().split()
    except (OSError, subprocess.CalledProcessError):
        return None
    for commit in commits:
        try:
            text = git("show", f"{commit}:CLAUDE.md")
        except subprocess.CalledProcessError:
            continue
        if text != ROOT_CLAUDE_MD:
            return text
    return None


class ManualMoveTests(unittest.TestCase):
    def read(self, name: str) -> str:
        path = ROOT / name
        self.assertTrue(path.is_file(), f"{name} is missing")
        return path.read_text(encoding="utf-8")

    def new_text(self) -> str:
        parts = [self.read(name) for name in DOCS]
        parts += [(ROOT / n).read_text(encoding="utf-8") for n in LATER_DOCS if (ROOT / n).is_file()]
        return squash("\n".join(parts))

    def old_text(self) -> str:
        return OLD.read_text(encoding="utf-8")

    def assert_kept(self, kind: str, items: List[str]) -> None:
        text = self.new_text()
        missing = [i for i in unique(items) if i not in RETIRED and squash(renamed(i)) not in text]
        self.assertEqual(missing, [], f"{kind} from the old CLAUDE.md missing from {DOCS + LATER_DOCS}")

    def test_every_backticked_token_is_kept(self):
        tokens = SPAN.findall(self.old_text())
        self.assertGreater(len(tokens), 500)
        self.assert_kept("backticked tokens", tokens)

    def test_every_quoted_string_and_number_is_kept(self):
        prose = SPAN.sub(" ", self.old_text())
        self.assert_kept("quoted strings", QUOTED.findall(prose))
        self.assert_kept("numbers", NUMBER.findall(prose))

    def test_fixture_matches_history(self):
        old = old_from_history()
        if old is None:
            self.skipTest("no commit with the old CLAUDE.md in this clone (shallow, or not a git checkout)")
        self.assertEqual(OLD.read_bytes(), old)

    def test_claude_md_is_one_import(self):
        self.assertEqual((ROOT / "CLAUDE.md").read_bytes(), ROOT_CLAUDE_MD)

    def test_root_ends_by_importing_the_rust_notes(self):
        text = self.read("AGENTS.md")
        lines = [line for line in text.splitlines() if line.strip()]
        self.assertEqual(lines[-1], "@crates/AGENTS.md")
        self.assertIn("(legacy/macos/Sources/AGENTS.md)", text)
        self.assertIn("(apps/flutter/AGENTS.md)", text)

    def test_root_is_short(self):
        count = len(self.read("AGENTS.md").splitlines())
        self.assertLessEqual(count, ROOT_MAX_LINES, "the root AGENTS.md points to the layers; details go there")

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
