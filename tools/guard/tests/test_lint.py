import io
import json
import os
import unittest
from contextlib import redirect_stdout

from guard.__main__ import main
from guard.lint import RULES, Violation, fingerprint, new_violations, rule, run
from tests.helpers import commit, make_repo

FIXTURE_REPO = make_repo({"README.md": "x\n"})
BAD_PATH = "crates/app/src/x.rs"
BAD_LINE = 'fn f() { eprintln!("x"); }\n'


def in_repo(repo, argv):
    cwd = os.getcwd()
    out = io.StringIO()
    try:
        os.chdir(repo)
        with redirect_stdout(out):
            code = main(argv)
    finally:
        os.chdir(cwd)
    return code, out.getvalue()


class RuleTests(unittest.TestCase):
    def test_every_rule_fails_on_its_bad_example(self):
        for r in RULES:
            repo = make_repo(dict(r.probe_files)) if r.probe_files else FIXTURE_REPO
            self.assertTrue(r.check(r.bad[0], r.bad[1].splitlines(), repo), r.id)

    def test_every_rule_passes_its_good_example(self):
        for r in RULES:
            self.assertEqual(r.check(r.good[0], r.good[1].splitlines(), FIXTURE_REPO), [], r.id)

    def test_rule_ids_unique_and_prefixed(self):
        ids = [r.id for r in RULES]
        self.assertEqual(len(ids), len(set(ids)))
        self.assertTrue(all(i.startswith("LINT-") for i in ids))

    def test_rule_examples_are_in_its_paths(self):
        for r in RULES:
            self.assertTrue(r.applies(r.bad[0]), r.id)

    def test_log_writer_not_in_string(self):
        self.assertEqual(rule("LINT-LOG-ONE-WRITER").check(BAD_PATH, ['let s = "eprintln!(x)";'], FIXTURE_REPO), [])

    def test_log_writer_not_in_the_test_module(self):
        lines = ["fn f() {}", "#[cfg(test)]", "mod tests {", '    fn t() { eprintln!("x"); }', "}"]
        self.assertEqual(rule("LINT-LOG-ONE-WRITER").check(BAD_PATH, lines, FIXTURE_REPO), [])

    def test_log_writer_finds_print_macros(self):
        found = rule("LINT-LOG-ONE-WRITER").check(BAD_PATH, ['println!("a");', 'eprint!("b");', "tracing::info!(x);"], FIXTURE_REPO)
        self.assertEqual(len(found), 3)

    def test_applescript_rule_reads_only_code(self):
        r = rule("LINT-NO-APPLESCRIPT")
        self.assertFalse(r.applies("crates/engine/AGENTS.md"))
        self.assertTrue(r.applies("crates/engine/src/lib.rs"))

    def test_tools_and_phone_tests_excluded(self):
        r = rule("LINT-FILE-SIZE")
        self.assertFalse(r.applies("tools/guard/guard/lint.py"))
        self.assertFalse(r.applies("apps/flutter/test/a_test.dart"))
        self.assertTrue(r.applies("crates/app/src/a.rs"))


class BaselineTests(unittest.TestCase):
    def test_baseline_absorbs_existing(self):
        v = Violation("LINT-NO-APPLESCRIPT", "crates/app/src/a.rs", 98, "NSAppleScript::new(s)")
        self.assertEqual(new_violations([v], [fingerprint(v)]), [])

    def test_fingerprint_ignores_line_number_and_spacing(self):
        a = Violation("R", "p", 1, "let  a = b")
        b = Violation("R", "p", 9, "let a = b")
        self.assertEqual(fingerprint(a), fingerprint(b))

    def test_baseline_counts_duplicates(self):
        v = Violation("LINT-LOG-ONE-WRITER", BAD_PATH, 1, "x")
        self.assertEqual(len(new_violations([v, v], [fingerprint(v)])), 1)

    def test_run_finds_violations_in_tracked_files(self):
        repo = make_repo({BAD_PATH: BAD_LINE})
        self.assertEqual([(v.rule, v.path, v.line) for v in run(repo)],
                         [("LINT-LOG-ONE-WRITER", BAD_PATH, 1)])

    def test_lint_cli_fails_on_new_and_names_rule(self):
        repo = make_repo({"tools/guard/lint-baseline.json": json.dumps({"entries": []}), BAD_PATH: BAD_LINE})
        code, out = in_repo(repo, ["lint"])
        self.assertEqual(code, 1)
        self.assertIn("crates/app/src/x.rs:1: LINT-LOG-ONE-WRITER", out)

    def test_write_baseline_then_clean(self):
        repo = make_repo({BAD_PATH: BAD_LINE})
        self.assertEqual(in_repo(repo, ["lint", "--write-baseline"])[0], 0)
        self.assertEqual(in_repo(repo, ["lint"])[0], 0)
        self.assertEqual(in_repo(repo, ["lint", "--write-baseline"])[0], 1)

    def test_baseline_cannot_grow(self):
        repo = make_repo({"tools/guard/lint-baseline.json": json.dumps({"entries": []}), BAD_PATH: BAD_LINE})
        code, out = in_repo(repo, ["lint", "--update-baseline"])
        self.assertEqual(code, 1)
        self.assertIn("GUARD-BASELINE", out)

    def test_update_baseline_shrinks(self):
        repo = make_repo({BAD_PATH: BAD_LINE})
        in_repo(repo, ["lint", "--write-baseline"])
        commit(repo, {BAD_PATH: "fn f() {}\n"})
        self.assertEqual(in_repo(repo, ["lint", "--update-baseline"])[0], 0)
        data = json.loads((repo / "tools/guard/lint-baseline.json").read_text())
        self.assertEqual(data["entries"], [])


if __name__ == "__main__":
    unittest.main()
