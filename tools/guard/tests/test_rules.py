import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from guard import capability, claudemd, lint, rules
from tests.helpers import git, make_repo

ROOT = Path(__file__).resolve().parents[3]
DOCS = ROOT / "docs/rules.md"
PKG = ROOT / "tools/guard"

TABLE = """# Rules

| ID | Rule | Why | Check |
| --- | --- | --- | --- |
{rows}
"""


def docs_with(ids):
    d = Path(tempfile.mkdtemp(prefix="rules-test-"))
    rows = "\n".join(f"| {i} | a rule | a reason | python3 -m guard {i.lower()} |" for i in ids)
    path = d / "rules.md"
    path.write_text(TABLE.format(rows=rows))
    return path


class DocsTests(unittest.TestCase):
    def test_real_docs_pass(self):
        self.assertEqual(rules.check(DOCS), [])

    def test_every_probe_passes_on_its_own(self):
        for rule_id, probe in sorted(rules.REGISTRY.items()):
            with self.subTest(rule=rule_id):
                self.assertTrue(probe(), f"{rule_id}: the check did not fire on its bad example")

    def test_docs_and_registry_list_the_same_ids(self):
        self.assertEqual(sorted(rules.listed_ids(DOCS.read_text())), sorted(rules.REGISTRY))

    def test_every_row_has_four_filled_cells(self):
        rows = 0
        for line in DOCS.read_text().splitlines():
            if line.startswith("| ") and not line.startswith("| ID") and not line.startswith("| ---"):
                rows += 1
                cells = [c.strip() for c in line.strip().strip("|").split("|")]
                with self.subTest(row=cells[0]):
                    self.assertEqual(len(cells), 4)
                    self.assertTrue(all(cells))
        self.assertEqual(rows, len(rules.REGISTRY))

    def test_no_id_is_listed_twice(self):
        ids = rules.listed_ids(DOCS.read_text())
        self.assertEqual(len(ids), len(set(ids)))

    def test_unknown_id_in_docs_fails(self):
        path = docs_with(list(rules.REGISTRY) + ["NOT-A-RULE"])
        problems = rules.check(path)
        self.assertTrue(any("NOT-A-RULE" in p and p.startswith("GUARD-RULES") for p in problems), problems)

    def test_registry_entry_missing_from_docs_fails(self):
        ids = list(rules.REGISTRY)
        path = docs_with(ids[1:])
        problems = rules.check(path)
        self.assertTrue(any(ids[0] in p and p.startswith("GUARD-RULES") for p in problems), problems)

    def test_listed_twice_fails(self):
        ids = list(rules.REGISTRY)
        problems = rules.check(docs_with(ids + [ids[0]]))
        self.assertTrue(any(ids[0] in p and "twice" in p for p in problems), problems)

    def test_probe_that_returns_false_fails(self):
        target = "LINT-NO-APPLESCRIPT"
        with mock.patch.dict(rules.REGISTRY, {target: lambda: False}):
            problems = rules.check(DOCS)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn(target, problems[0])
        self.assertTrue(problems[0].startswith("GUARD-RULES"))

    def test_probe_that_raises_fails(self):
        def boom():
            raise RuntimeError("probe broke")
        with mock.patch.dict(rules.REGISTRY, {"WEAK-SKIP": boom}):
            problems = rules.check(DOCS)
        self.assertTrue(any("WEAK-SKIP" in p and "probe broke" in p for p in problems), problems)

    def test_fake_registry_whose_check_never_fires_fails(self):
        path = docs_with(["FAKE-ONE", "FAKE-TWO"])
        reg = {"FAKE-ONE": lambda: True, "FAKE-TWO": lambda: False}
        problems = rules.check(path, reg)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("FAKE-TWO", problems[0])

    def test_missing_docs_file_fails(self):
        problems = rules.check(Path(tempfile.mkdtemp()) / "rules.md")
        self.assertTrue(problems and problems[0].startswith("GUARD-RULES"), problems)

    def test_main_prints_problems_and_exits_nonzero(self):
        path = docs_with(["NOT-A-RULE"])
        with mock.patch("builtins.print") as out:
            code = rules.main(path)
        self.assertEqual(code, 1)
        self.assertIn("NOT-A-RULE", " ".join(str(c.args[0]) for c in out.call_args_list))

    def test_cli_passes_on_the_real_repo(self):
        env = dict(os.environ, PYTHONPATH=str(PKG))
        for py in {sys.executable, "/usr/bin/python3"}:
            if not Path(py).exists():
                continue
            r = subprocess.run([py, "-m", "guard", "rules"], cwd=str(ROOT), env=env, capture_output=True,
                               text=True, timeout=120)
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr)


class CoverageTests(unittest.TestCase):

    def test_lint_rules(self):
        self.assertEqual({i for i in rules.REGISTRY if i.startswith("LINT-")}, {r.id for r in lint.RULES})

    def test_capability_rules(self):
        want = set(capability.RULES) | {capability.BINARY_RULE, capability.OBSCURE_RULE}
        self.assertEqual({i for i in rules.REGISTRY if i.startswith("CAP-")}, want)

    def test_weakening_zone_and_collision_rules(self):
        self.assertEqual({i for i in rules.REGISTRY if i.startswith("WEAK-")},
                         {"WEAK-TEST-EDIT", "WEAK-SKIP", "WEAK-UNICODE"})
        for i in ("ZONE", "CASE-COLLISION"):
            self.assertIn(i, rules.REGISTRY)

    def test_every_guard_id_in_the_source_is_registered(self):
        found = set()
        for f in (PKG / "guard").glob("*.py"):
            found |= set(re.findall(r"\bGUARD-[A-Z]+(?:-[A-Z]+)*\b", f.read_text()))
        self.assertTrue(found)
        self.assertEqual(found - set(rules.REGISTRY), set())
        self.assertEqual({i for i in rules.REGISTRY if i.startswith("GUARD-")} - found, set())

    def test_warning_rule_is_listed_and_fires(self):
        self.assertTrue(lint.rule("LINT-FILE-SIZE-WARN").warn)
        self.assertTrue(rules.REGISTRY["LINT-FILE-SIZE-WARN"]())


class ClaudeMdTests(unittest.TestCase):
    def test_exact_root_file_passes(self):
        repo = make_repo({"CLAUDE.md": "@AGENTS.md\n", "AGENTS.md": "x\n"})
        self.assertEqual(claudemd.check(repo), [])

    def test_stray_nested_file_fails(self):
        repo = make_repo({"CLAUDE.md": "@AGENTS.md\n", "legacy/macos/Sources/CLAUDE.md": "more rules\n"})
        problems = claudemd.check(repo)
        self.assertTrue(any(p.startswith("GUARD-CLAUDE-MD") and "legacy/macos/Sources/CLAUDE.md" in p for p in problems), problems)

    def test_other_case_and_suffix_fail(self):
        for name in ("docs/claude.md", "apps/flutter/Claude.local.md", "CLAUDE.local.md"):
            with self.subTest(name=name):
                repo = make_repo({"CLAUDE.md": "@AGENTS.md\n", name: "x\n"})
                self.assertTrue(any(name in p for p in claudemd.check(repo)))

    def test_root_with_other_text_fails(self):
        for text in ("# Notes\n", "@AGENTS.md", "@AGENTS.md\n\n", "@AGENTS.md\n@legacy/macos/Sources/AGENTS.md\n", ""):
            with self.subTest(text=text):
                repo = make_repo({"CLAUDE.md": text})
                problems = claudemd.check(repo)
                self.assertTrue(problems and all(p.startswith("GUARD-CLAUDE-MD") for p in problems), problems)

    def test_missing_root_fails(self):
        repo = make_repo({"AGENTS.md": "x\n"})
        self.assertTrue(any(p.startswith("GUARD-CLAUDE-MD") for p in claudemd.check(repo)))

    def test_untracked_file_is_ignored(self):
        repo = make_repo({"CLAUDE.md": "@AGENTS.md\n"})
        (repo / "legacy/macos/Sources").mkdir(parents=True)
        (repo / "legacy/macos/Sources/CLAUDE.md").write_text("untracked\n")
        self.assertEqual(claudemd.check(repo), [])

    def test_symlinked_root_fails(self):
        repo = make_repo({"AGENTS.md": "@AGENTS.md\n"})
        (repo / "CLAUDE.md").symlink_to("AGENTS.md")
        git(repo, "add", "CLAUDE.md")
        self.assertTrue(any(p.startswith("GUARD-CLAUDE-MD") for p in claudemd.check(repo)))

    def test_cli_exit_code(self):
        env = dict(os.environ, PYTHONPATH=str(PKG))
        bad = make_repo({"CLAUDE.md": "# Notes\n"})
        good = make_repo({"CLAUDE.md": "@AGENTS.md\n"})
        for repo, code in ((bad, 1), (good, 0)):
            r = subprocess.run([sys.executable, "-m", "guard", "claudemd"], cwd=str(repo), env=env,
                               capture_output=True, text=True, timeout=60)
            self.assertEqual(r.returncode, code, r.stdout + r.stderr)

    def test_real_tree(self):
        root_file = ROOT / "CLAUDE.md"
        if not root_file.exists() or root_file.read_bytes() != b"@AGENTS.md\n":
            self.skipTest("the root CLAUDE.md is not yet exactly '@AGENTS.md' + newline (Task A)")
        self.assertEqual(claudemd.check(ROOT), [])


if __name__ == "__main__":
    unittest.main()
