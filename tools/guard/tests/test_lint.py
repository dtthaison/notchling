import io
import json
import os
import unittest
from contextlib import redirect_stdout

from guard.__main__ import main
from guard.lint import RULES, Violation, fingerprint, new_violations, rule, run
from tests.helpers import commit, make_repo

APP_DELEGATE = """final class AppDelegate {
    static func migrateFromAwake() {
        guard !defaults.bool(forKey: "migratedFromAwake") else { return }
        for key in ["mode", "showPet"] {
            copy(key)
        }
    }
}
"""
FIXTURE_REPO = make_repo({"legacy/macos/Sources/Notchling/App/AppDelegate.swift": APP_DELEGATE})


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
            self.assertTrue(r.check(r.bad[0], r.bad[1].splitlines(), FIXTURE_REPO), r.id)

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

    def test_force_unwrap_not_inequality_or_not(self):
        self.assertEqual(rule("LINT-NO-FORCE-UNWRAP").check("legacy/macos/Sources/A.swift", ["if a != b {}", "if !ok {}", "f(!x)"],
                                                            FIXTURE_REPO), [])

    def test_force_unwrap_not_in_string(self):
        self.assertEqual(rule("LINT-NO-FORCE-UNWRAP").check("legacy/macos/Sources/A.swift", ['Text("Done!")'], FIXTURE_REPO), [])

    def test_force_try_and_cast(self):
        self.assertEqual(len(rule("LINT-NO-FORCE-UNWRAP").check("legacy/macos/Sources/A.swift", ["let a = try! f()", "b as! C"],
                                                                FIXTURE_REPO)), 2)

    def test_defaults_key_migrated_ok(self):
        self.assertEqual(rule("LINT-DEFAULTS-MIGRATED").check(
            "legacy/macos/Sources/Notchling/A.swift", ['UserDefaults.standard.bool(forKey: "showPet")'], FIXTURE_REPO), [])

    def test_defaults_ignores_other_forkey(self):
        self.assertEqual(rule("LINT-DEFAULTS-MIGRATED").check(
            "legacy/macos/Sources/Notchling/A.swift", ['coder.encode(x, forKey: "nope")'], FIXTURE_REPO), [])

    def test_applescript_rule_reads_only_code(self):
        r = rule("LINT-NO-APPLESCRIPT")
        self.assertFalse(r.applies("legacy/macos/Sources/AGENTS.md"))
        self.assertTrue(r.applies("legacy/macos/Sources/Notchling/Agents/AgentActions.swift"))
        self.assertTrue(r.applies("crates/engine/src/lib.rs"))

    def test_tests_and_demo_excluded(self):
        r = rule("LINT-NO-FORCE-UNWRAP")
        self.assertFalse(r.applies("legacy/macos/Tests/NotchlingTests/A.swift"))
        self.assertFalse(r.applies("legacy/macos/Demo/Render.swift"))


class BaselineTests(unittest.TestCase):
    def test_baseline_absorbs_existing(self):
        v = Violation("LINT-NO-APPLESCRIPT", "legacy/macos/Sources/Notchling/Agents/AgentActions.swift", 98, "NSAppleScript(source: s)")
        self.assertEqual(new_violations([v], [fingerprint(v)]), [])

    def test_fingerprint_ignores_line_number_and_spacing(self):
        a = Violation("R", "p", 1, "let  a = b!")
        b = Violation("R", "p", 9, "let a = b!")
        self.assertEqual(fingerprint(a), fingerprint(b))

    def test_baseline_counts_duplicates(self):
        v = Violation("LINT-NO-FORCE-UNWRAP", "legacy/macos/Sources/A.swift", 1, "x!")
        self.assertEqual(len(new_violations([v, v], [fingerprint(v)])), 1)

    def test_run_finds_violations_in_tracked_files(self):
        repo = make_repo({"legacy/macos/Sources/Notchling/App/AppDelegate.swift": APP_DELEGATE,
                          "legacy/macos/Sources/Notchling/Search/S.swift": "let a = b!\n"})
        self.assertEqual([(v.rule, v.path, v.line) for v in run(repo)],
                         [("LINT-NO-FORCE-UNWRAP", "legacy/macos/Sources/Notchling/Search/S.swift", 1)])

    def test_lint_cli_fails_on_new_and_names_rule(self):
        repo = make_repo({"legacy/macos/Sources/Notchling/App/AppDelegate.swift": APP_DELEGATE,
                          "tools/guard/lint-baseline.json": json.dumps({"entries": []}),
                          "legacy/macos/Sources/Notchling/Search/S.swift": "let a = b!\n"})
        code, out = in_repo(repo, ["lint"])
        self.assertEqual(code, 1)
        self.assertIn("legacy/macos/Sources/Notchling/Search/S.swift:1: LINT-NO-FORCE-UNWRAP", out)

    def test_write_baseline_then_clean(self):
        repo = make_repo({"legacy/macos/Sources/Notchling/App/AppDelegate.swift": APP_DELEGATE,
                          "legacy/macos/Sources/Notchling/Search/S.swift": "let a = b!\n"})
        self.assertEqual(in_repo(repo, ["lint", "--write-baseline"])[0], 0)
        self.assertEqual(in_repo(repo, ["lint"])[0], 0)
        self.assertEqual(in_repo(repo, ["lint", "--write-baseline"])[0], 1)

    def test_baseline_cannot_grow(self):
        repo = make_repo({"legacy/macos/Sources/Notchling/App/AppDelegate.swift": APP_DELEGATE,
                          "tools/guard/lint-baseline.json": json.dumps({"entries": []}),
                          "legacy/macos/Sources/Notchling/Search/S.swift": "let a = b!\n"})
        code, out = in_repo(repo, ["lint", "--update-baseline"])
        self.assertEqual(code, 1)
        self.assertIn("GUARD-BASELINE", out)

    def test_update_baseline_shrinks(self):
        repo = make_repo({"legacy/macos/Sources/Notchling/App/AppDelegate.swift": APP_DELEGATE,
                          "legacy/macos/Sources/Notchling/Search/S.swift": "let a = b!\n"})
        in_repo(repo, ["lint", "--write-baseline"])
        commit(repo, {"legacy/macos/Sources/Notchling/Search/S.swift": "let a = b\n"})
        self.assertEqual(in_repo(repo, ["lint", "--update-baseline"])[0], 0)
        data = json.loads((repo / "tools/guard/lint-baseline.json").read_text())
        self.assertEqual(data["entries"], [])


if __name__ == "__main__":
    unittest.main()
