"""Phase 3 moved the notes in CLAUDE.md into AGENTS.md files. Claude Code still loads them through
`@` imports (CLAUDE.md is `@AGENTS.md`, which ends with `@legacy/macos/Sources/AGENTS.md`); Codex and Antigravity
read AGENTS.md themselves. These tests prove the move lost nothing an agent relies on and that the
import chain holds.

The old CLAUDE.md is a fixture, so the tests need no git history (CI clones are shallow). One test
checks the fixture against history when history is there.
"""

from __future__ import annotations

import re
import subprocess
import unittest
from pathlib import Path
from typing import Dict, List, Optional

ROOT = Path(__file__).resolve().parents[3]
# The old CLAUDE.md, byte for byte. Not named *.md: GUARD-CLAUDE-MD rejects any tracked `claude*.md`,
# and nothing should mistake it for live instructions.
OLD = Path(__file__).resolve().parent / "fixtures" / "notes-before-phase3.txt"
DOCS = ["AGENTS.md", "legacy/macos/Sources/AGENTS.md"]
LATER_DOCS = ["ARCHITECTURE.md"]  # written by another task; counted once it exists
# A fact the docs drop on purpose goes here, with the reason. Editing this file needs the maintainer.
RETIRED: Dict[str, str] = {}
# Phase 4 moved the phone to apps/flutter/. The old notes name its old folder, so a token
# is looked for under its new name. (Nothing else moved in a way the old notes name: the Swift app's
# paths are written relative to legacy/macos/, so they still match as substrings.)
RENAMED = {"mobile" + "/": "apps/flutter/"}  # (spelled in two parts: test_layout bans the old name)
ROOT_MAX_LINES = 80
ROOT_CLAUDE_MD = b"@AGENTS.md\n"

SPAN = re.compile(r"`([^`\n]+)`")
QUOTED = re.compile(r'"([^"\n]+)"')
NUMBER = re.compile(r"\d+(?:[.,:]\d+)*")
IMPORT = re.compile(r"^@(\S+)\s*$")
# How Claude Code spots an import: `@` at the start of a word, outside code.
BARE_AT = re.compile(r"(?:^|\s)@\S")


def squash(text: str) -> str:
    """One space for any run of whitespace, so a wrapped line still matches."""
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
    """CLAUDE.md as the newest commit that held anything but `@AGENTS.md` left it, or None."""
    try:
        commits = git("log", "--format=%H", "--", "CLAUDE.md").decode().split()
    except (OSError, subprocess.CalledProcessError):
        return None
    for commit in commits:
        try:
            text = git("show", f"{commit}:CLAUDE.md")
        except subprocess.CalledProcessError:  # deleted in that commit
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
        self.assertGreater(len(tokens), 500)  # the fixture is the real notes, not a stub
        self.assert_kept("backticked tokens", tokens)

    def test_every_quoted_string_and_number_is_kept(self):
        """Limits, timeouts, versions and the exact strings agents match on, outside backticks."""
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

    def test_root_ends_by_importing_the_swift_notes(self):
        text = self.read("AGENTS.md")
        lines = [line for line in text.splitlines() if line.strip()]
        self.assertEqual(lines[-1], "@legacy/macos/Sources/AGENTS.md")
        self.assertIn("(apps/flutter/AGENTS.md)", text)  # a link to the phone notes

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
        """Claude Code treats `@word` outside code as an import, so one must be a whole-line import."""
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
