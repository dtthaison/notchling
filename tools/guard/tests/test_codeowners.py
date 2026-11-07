import io
import os
import unittest
from contextlib import redirect_stdout
from pathlib import Path

from guard.__main__ import main
from guard.codeowners import render
from guard.zones import glob_regex, load
from tests.helpers import make_repo

ROOT = Path(__file__).resolve().parents[3]
Z = load(ROOT / "tools/guard/zones.json")


def owner_of(text, path):
    """Last matching line wins, the way GitHub reads CODEOWNERS (gitignore patterns). None = no owner."""
    owner = None
    for line in text.splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        pattern, *who = line.split()
        if "/" not in pattern:  # no slash: matches at any depth
            pattern = "**/" + pattern
        pattern = pattern.lstrip("/")
        if pattern.endswith("/"):
            pattern += "**"
        if glob_regex(pattern).match(path):
            owner = who[0] if who else None
    return owner


class CodeownersTests(unittest.TestCase):
    def test_codeowners_starts_with_owner(self):
        lines = [l for l in render(Z).splitlines() if l and not l.startswith("#")]
        self.assertEqual(lines[0], "* @dtthaison")

    def test_zone_lines_have_no_owner(self):
        lines = [l for l in render(Z).splitlines() if l.startswith("/legacy/macos/Sources/Notchling/Chat/Views/")]
        self.assertEqual(lines, ["/legacy/macos/Sources/Notchling/Chat/Views/"])

    def test_exclusions_reowned_after_zone(self):
        lines = render(Z).splitlines()
        self.assertGreater(lines.index("/docs/rules.md @dtthaison"), lines.index("/docs/"))

    def test_real_file_up_to_date(self):
        self.assertEqual((ROOT / ".github/CODEOWNERS").read_text(), render(Z))

    def test_codeowners_check_fails_on_stale(self):
        repo = make_repo({"tools/guard/zones.json": (ROOT / "tools/guard/zones.json").read_text(),
                          ".github/CODEOWNERS": "* @someone\n"})
        cwd = os.getcwd()
        try:
            os.chdir(repo)
            out = io.StringIO()
            with redirect_stdout(out):
                code = main(["codeowners", "--check"])
        finally:
            os.chdir(cwd)
        self.assertEqual(code, 1)
        self.assertIn("GUARD-CODEOWNERS", out.getvalue())

    # I4 and C2: AGENTS.md, CLAUDE.md, .claude/, .gitattributes and CODEOWNERS are re-owned everywhere.
    def test_spec_protected_files_reowned_after_the_zone(self):
        lines = render(Z).splitlines()
        for pattern in ["**/AGENTS.md", "**/CLAUDE.md", "**/.claude/", "**/.gitattributes", "**/CODEOWNERS"]:
            line = f"{pattern} @dtthaison"
            self.assertIn(line, lines)
            self.assertGreater(lines.index(line), lines.index("/docs/"), pattern)
            self.assertGreater(lines.index(line), lines.index("/legacy/macos/Tests/"), pattern)

    def test_test_config_files_reowned_after_the_phone_tests(self):
        lines = render(Z).splitlines()
        for line in ["/apps/flutter/**/flutter_test_config.dart @dtthaison", "/apps/flutter/**/dart_test.yaml @dtthaison"]:
            self.assertIn(line, lines)
            self.assertGreater(lines.index(line), lines.index("/apps/flutter/test/"))

    def test_no_negation_left_in_the_file(self):
        self.assertNotIn("!", render(Z))

    def test_owner_resolution_matches_the_zone(self):
        text = render(Z)
        owned = ["docs/AGENTS.md", "docs/CLAUDE.md", "AGENTS.md", "legacy/macos/Sources/Notchling/Search/CLAUDE.md",
                 "legacy/macos/Sources/Notchling/Search/.claude/skills/x/SKILL.md", "legacy/macos/Sources/Notchling/Search/.gitattributes",
                 "legacy/macos/Tests/NotchlingTests/.gitattributes", "docs/CODEOWNERS", "docs/rules.md",
                 "apps/flutter/lib/features/pairing/pair.dart", "apps/flutter/test/flutter_test_config.dart",
                 "apps/flutter/test/chat/dart_test.yaml", "build.sh", "legacy/macos/Sources/Notchling/Chat/Model/ChatModel.swift"]
        for p in owned:
            self.assertEqual(owner_of(text, p), "@dtthaison", p)
        free = ["legacy/macos/Sources/Notchling/Search/SearchPanel.swift", "docs/chat-design.md", "legacy/macos/Tests/NotchlingTests/ChatTests.swift",
                "apps/flutter/test/chat/chat_test.dart", "apps/flutter/lib/features/chat/chat_page.dart"]
        for p in free:
            self.assertIsNone(owner_of(text, p), p)


if __name__ == "__main__":
    unittest.main()
