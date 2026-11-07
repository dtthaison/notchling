import io
import json
import os
import subprocess
import tempfile
import unittest
from contextlib import redirect_stdout

from guard.__main__ import main
from guard.changes import Change, diff
from guard.gate import (MAINTAINERS, added_paths, approved_head, baseline_reasons, case_collisions,
                        collision_reasons, decide, needs_maintainer)
from guard.lint import BASELINE
from guard.zones import Zones
from tests.helpers import ENV, commit, git, head_of, make_repo

H = "a" * 40
R = ["ZONE: build.sh"]
Z = Zones(("legacy/macos/Sources/Notchling/Search/**",), ("legacy/macos/Tests/**",))


def rev(login, state, commit_id, t=1):
    return {"user": {"login": login}, "state": state, "commit_id": commit_id,
            "submitted_at": f"2026-11-07T10:00:0{t}Z"}


class DecideTests(unittest.TestCase):
    def test_decide_nothing_protected(self):
        d = decide([], [], H)
        self.assertTrue(d.passed)
        self.assertEqual(d.summary, "Nothing protected")

    def test_decide_needs_maintainer(self):
        d = decide(["ZONE: build.sh"], [], H)
        self.assertFalse(d.passed)
        self.assertEqual(d.summary, "Needs the maintainer: ZONE: build.sh")

    def test_decide_more_reasons_counted(self):
        self.assertEqual(decide(["ZONE: a", "ZONE: b", "CAP-PTY: c"], [], H).summary,
                         "Needs the maintainer: ZONE: a (+2 more)")

    def test_decide_approved_head(self):
        d = decide(R, [rev("dtthaison", "APPROVED", H)], H)
        self.assertTrue(d.passed)
        self.assertEqual(d.summary, "Approved by the maintainer at aaaaaaa")

    def test_decide_old_approval(self):
        self.assertFalse(decide(R, [rev("dtthaison", "APPROVED", "old")], H).passed)

    def test_decide_later_changes_requested(self):
        self.assertFalse(decide(R, [rev("dtthaison", "APPROVED", H, t=1),
                                    rev("dtthaison", "CHANGES_REQUESTED", H, t=2)], H).passed)

    def test_decide_reviews_out_of_order(self):
        self.assertFalse(decide(R, [rev("dtthaison", "CHANGES_REQUESTED", H, t=2),
                                    rev("dtthaison", "APPROVED", H, t=1)], H).passed)

    def test_decide_comment_after_approval_keeps_it(self):
        self.assertTrue(decide(R, [rev("dtthaison", "APPROVED", H, t=1),
                                   rev("dtthaison", "COMMENTED", H, t=2)], H).passed)

    def test_decide_other_approver(self):
        self.assertFalse(decide(R, [rev("TrietKun", "APPROVED", H)], H).passed)

    def test_decide_deleted_user(self):
        self.assertFalse(decide(R, [{"user": None, "state": "APPROVED", "commit_id": H,
                                     "submitted_at": "2026-11-07T10:00:01Z"}], H).passed)

    def test_approved_head_maintainers(self):
        self.assertEqual(MAINTAINERS, frozenset({"dtthaison"}))
        self.assertTrue(approved_head([rev("TrietKun", "APPROVED", H)], H, frozenset({"TrietKun"})))

    # I5: a dismissed review is still that maintainer's latest word.
    def test_decide_dismissed_changes_request_does_not_revive_older_approval(self):
        self.assertFalse(decide(R, [rev("dtthaison", "APPROVED", H, t=1),
                                    rev("dtthaison", "CHANGES_REQUESTED", H, t=2),
                                    rev("dtthaison", "DISMISSED", H, t=3)], H).passed)

    def test_decide_dismissal_after_approval_blocks(self):
        self.assertFalse(decide(R, [rev("dtthaison", "APPROVED", H, t=1),
                                    rev("dtthaison", "DISMISSED", H, t=2)], H).passed)

    def test_decide_new_approval_after_dismissal_passes(self):
        self.assertTrue(decide(R, [rev("dtthaison", "DISMISSED", H, t=1),
                                   rev("dtthaison", "APPROVED", H, t=2)], H).passed)

    def test_decide_dismissed_approval_alone_fails(self):
        self.assertFalse(decide(R, [rev("dtthaison", "DISMISSED", H)], H).passed)

    def test_decide_dismissed_by_one_maintainer_leaves_the_other(self):
        both = frozenset({"a", "b"})
        self.assertTrue(approved_head([rev("a", "APPROVED", H, t=1), rev("b", "DISMISSED", H, t=2)], H, both))
        self.assertFalse(approved_head([rev("a", "DISMISSED", H, t=2), rev("b", "DISMISSED", H, t=1)], H, both))

    def test_decide_comment_after_dismissal_changes_nothing(self):
        self.assertFalse(decide(R, [rev("dtthaison", "APPROVED", H, t=1),
                                    rev("dtthaison", "DISMISSED", H, t=2),
                                    rev("dtthaison", "COMMENTED", H, t=3)], H).passed)

    def test_summary_fits_status(self):
        self.assertLessEqual(len(decide(["ZONE: " + "x" * 300], [], H).summary), 140)


class NeedsMaintainerTests(unittest.TestCase):
    def test_needs_maintainer_combines(self):
        repo = make_repo({"build.sh": "echo 1\n", "legacy/macos/Sources/Notchling/Search/S.swift": "let a = 1\n"})
        base = head_of(repo)
        commit(repo, {"build.sh": "echo 2\n", "legacy/macos/Sources/Notchling/Search/S.swift": "let p = Process()\n"})
        changes = diff(repo, base, head_of(repo))
        self.assertEqual(sorted(r.split(":")[0] for r in needs_maintainer(changes, Z)), ["CAP-PROCESS", "ZONE"])

    def test_needs_maintainer_weakening(self):
        repo = make_repo({"legacy/macos/Tests/A.swift": "func testA() {}\n"})
        base = head_of(repo)
        commit(repo, {"legacy/macos/Tests/A.swift": "func testA() { throw XCTSkip(\"x\") }\n"})
        reasons = needs_maintainer(diff(repo, base, head_of(repo)), Z)
        self.assertIn("WEAK-TEST-EDIT", [r.split(":")[0] for r in reasons])

    def test_zone_only_change_is_clean(self):
        repo = make_repo({"legacy/macos/Sources/Notchling/Search/S.swift": "let a = 1\n"})
        base = head_of(repo)
        commit(repo, {"legacy/macos/Sources/Notchling/Search/S.swift": "let a = 2\n"})
        self.assertEqual(needs_maintainer(diff(repo, base, head_of(repo)), Z), [])


def commit_exact(repo, files):
    """Commit paths that differ only by case, through plumbing: a case-insensitive disk (macOS)
    can't hold both in a working tree."""
    git(repo, "config", "core.ignorecase", "false")
    for path, text in files.items():
        blob = subprocess.run(["git", "-C", str(repo), "hash-object", "-w", "--stdin"], input=text.encode(),
                              check=True, capture_output=True, env=ENV).stdout.decode().strip()
        git(repo, "update-index", "--add", "--cacheinfo", f"100644,{blob},{path}")
    git(repo, "commit", "-q", "-m", "case")
    return head_of(repo)


class CaseCollisionTests(unittest.TestCase):
    """I2: on a Mac a path that differs only by case (or normalization) is another file's path."""

    def test_file_beside_a_file_of_another_case(self):
        self.assertEqual(case_collisions(["legacy/macos/Tests/NotchlingTests/chattests.swift"],
                                         ["legacy/macos/Tests/NotchlingTests/ChatTests.swift"]),
                         ["CASE-COLLISION: legacy/macos/Tests/NotchlingTests/chattests.swift vs legacy/macos/Tests/NotchlingTests/ChatTests.swift"])

    def test_normalization_counts(self):
        self.assertEqual(case_collisions(["docs/cafe\u0301.md"], ["docs/caf\u00e9.md"]),
                         ["CASE-COLLISION: docs/cafe\u0301.md vs docs/caf\u00e9.md"])
        self.assertEqual(len(case_collisions(["docs/CAFE\u0301.md"], ["docs/caf\u00e9.md"])), 1)

    def test_protected_folder_of_another_case(self):
        self.assertEqual(case_collisions(["docs/Decisions/x.md"], ["docs/decisions/0001.md"]),
                         ["CASE-COLLISION: docs/Decisions/x.md vs docs/decisions/"])
        self.assertEqual(case_collisions(["apps/flutter/lib/features/Pairing/pair.dart"],
                                         ["apps/flutter/lib/features/pairing/keys.dart"]),
                         ["CASE-COLLISION: apps/flutter/lib/features/Pairing/pair.dart vs apps/flutter/lib/features/pairing/"])

    def test_file_named_like_a_folder(self):
        self.assertEqual(len(case_collisions(["Docs"], ["docs/a.md"])), 1)
        self.assertEqual(len(case_collisions(["docs/Sub/a.md"], ["docs/sub"])), 1)

    def test_two_new_paths_that_collide_with_each_other(self):
        tree = ["a/B.md", "a/b.md"]
        self.assertEqual(len(case_collisions(tree, tree)), 2)

    def test_unrelated_paths_are_fine(self):
        self.assertEqual(case_collisions(["docs/new.md", "docs/sub/new.md"],
                                         ["docs/old.md", "docs/sub/old.md", "legacy/macos/Tests/A.swift"]), [])

    def test_a_path_is_not_its_own_collision(self):
        self.assertEqual(case_collisions(["docs/a.md"], ["docs/a.md"]), [])

    def test_added_paths_are_new_names_only(self):
        changes = [Change("a.md", "A"), Change("b.md", "M"), Change("c.md", "D"), Change("e.md", "R", "d.md")]
        self.assertEqual(added_paths(changes), ["a.md", "e.md"])

    def test_zone_folded_reasons_in_needs_maintainer(self):
        z = Zones(("docs/**", "!docs/decisions/**"), ())
        reasons = needs_maintainer([Change("docs/Decisions/x.md", "A"), Change("docs/ok.md", "A")], z)
        self.assertEqual([r.split(":")[0] for r in reasons], ["CASE-COLLISION"])
        self.assertIn("docs/Decisions/x.md", reasons[0])

    def test_attack_test_file_beside_an_existing_one(self):
        repo = make_repo({"legacy/macos/Tests/NotchlingTests/ChatTests.swift": "final class ChatTests {}\n"})
        base = head_of(repo)
        head = commit_exact(repo, {"legacy/macos/Tests/NotchlingTests/chattests.swift": "final class ChatTests {}\n"})
        reasons = collision_reasons(repo, base, head, diff(repo, base, head))
        self.assertEqual(reasons, ["CASE-COLLISION: legacy/macos/Tests/NotchlingTests/chattests.swift vs "
                                   "legacy/macos/Tests/NotchlingTests/ChatTests.swift"])

    def test_attack_two_new_files_in_one_pr(self):
        repo = make_repo({"README.md": "x\n"})
        base = head_of(repo)
        head = commit_exact(repo, {"legacy/macos/Tests/A/Same.swift": "a\n", "legacy/macos/Tests/A/same.swift": "b\n"})
        reasons = collision_reasons(repo, base, head, diff(repo, base, head))
        self.assertEqual(len(reasons), 2)
        self.assertTrue(all(r.startswith("CASE-COLLISION: legacy/macos/Tests/A/") for r in reasons))

    def test_attack_protected_folder_in_the_base_tree(self):
        repo = make_repo({"docs/decisions/0001.md": "x\n", "docs/chat.md": "x\n"})
        base = head_of(repo)
        head = commit_exact(repo, {"docs/Decisions/0002.md": "y\n"})
        reasons = collision_reasons(repo, base, head, diff(repo, base, head))
        self.assertEqual(reasons, ["CASE-COLLISION: docs/Decisions/0002.md vs docs/decisions/"])

    def test_clean_pr_has_no_collisions(self):
        repo = make_repo({"docs/a.md": "x\n", "legacy/macos/Tests/T.swift": "x\n"})
        base = head_of(repo)
        head = commit(repo, {"docs/b.md": "y\n", "docs/sub/c.md": "z\n"})
        self.assertEqual(collision_reasons(repo, base, head, diff(repo, base, head)), [])


class BaselineTests(unittest.TestCase):
    PATH = str(BASELINE)

    def baseline(self, *entries):
        return json.dumps({"entries": list(entries)})

    def reasons(self, base_entries, head_content):
        repo = make_repo({self.PATH: self.baseline(*base_entries)})
        base = head_of(repo)
        head = commit(repo, {self.PATH: head_content})
        return baseline_reasons(repo, base, head)

    def test_grown_baseline_is_a_reason(self):
        reasons = self.reasons(["A|a.swift|x"], self.baseline("A|a.swift|x", "B|b.swift|y"))
        self.assertEqual(len(reasons), 1)
        self.assertTrue(reasons[0].startswith("GUARD-BASELINE:"))
        self.assertIn("1 ", reasons[0])
        self.assertIn("B|b.swift|y", reasons[0])

    def test_shrunk_baseline_is_fine(self):
        self.assertEqual(self.reasons(["A|a|x", "B|b|y"], self.baseline("A|a|x")), [])

    def test_equal_baseline_is_fine(self):
        self.assertEqual(self.reasons(["A|a|x", "B|b|y"], self.baseline("B|b|y", "A|a|x")), [])

    def test_duplicate_entry_added_is_growth(self):
        reasons = self.reasons(["A|a|x"], self.baseline("A|a|x", "A|a|x"))
        self.assertEqual(len(reasons), 1)
        self.assertTrue(reasons[0].startswith("GUARD-BASELINE:"))

    def test_invalid_json_at_head_is_a_reason(self):
        reasons = self.reasons(["A|a|x"], "{ not json")
        self.assertEqual(len(reasons), 1)
        self.assertTrue(reasons[0].startswith("GUARD-BASELINE:"))

    def test_file_added_fresh_with_entries_is_growth(self):
        repo = make_repo({"x": "1\n"})
        base = head_of(repo)
        head = commit(repo, {self.PATH: self.baseline("A|a|x")})
        self.assertEqual(len(baseline_reasons(repo, base, head)), 1)

    def test_missing_at_both_is_fine(self):
        repo = make_repo({"x": "1\n"})
        base = head_of(repo)
        head = commit(repo, {"y": "2\n"})
        self.assertEqual(baseline_reasons(repo, base, head), [])

    def test_deleted_at_head_is_fine(self):
        repo = make_repo({self.PATH: self.baseline("A|a|x")})
        base = head_of(repo)
        head = commit(repo, {self.PATH: None})
        self.assertEqual(baseline_reasons(repo, base, head), [])


class GateCliTests(unittest.TestCase):
    def run_cli(self, argv):
        out = io.StringIO()
        with redirect_stdout(out):
            code = main(argv)
        return code, out.getvalue()

    def repo_with_zones(self, change):
        zones = json.dumps({"auto": ["legacy/macos/Sources/Notchling/Search/**"], "new_files_only": []})
        repo = make_repo({"tools/guard/zones.json": zones, "build.sh": "echo 1\n",
                          "legacy/macos/Sources/Notchling/Search/S.swift": "let a = 1\n"})
        base = head_of(repo)
        commit(repo, change)
        return repo, base, head_of(repo)

    def test_cli_fails_on_protected(self):
        repo, base, head = self.repo_with_zones({"build.sh": "echo 2\n"})
        code, out = self.run_cli(["gate", "--repo", str(repo), "--base", base, "--head", head])
        self.assertEqual(code, 1)
        self.assertIn("ZONE: build.sh", out)

    def test_cli_passes_with_reviews_file(self):
        repo, base, head = self.repo_with_zones({"build.sh": "echo 2\n"})
        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
            json.dump([rev("dtthaison", "APPROVED", head)], f)
        try:
            code, out = self.run_cli(["gate", "--repo", str(repo), "--base", base, "--head", head,
                                      "--reviews", f.name])
        finally:
            os.unlink(f.name)
        self.assertEqual(code, 0)
        self.assertIn("Approved by the maintainer", out)

    def test_cli_fails_on_case_collision(self):
        zones = json.dumps({"auto": ["legacy/macos/Tests/**"], "new_files_only": ["legacy/macos/Tests/**"]})
        repo = make_repo({"tools/guard/zones.json": zones, "legacy/macos/Tests/ChatTests.swift": "final class ChatTests {}\n"})
        base = head_of(repo)
        head = commit_exact(repo, {"legacy/macos/Tests/chattests.swift": "final class ChatTests {}\n"})
        code, out = self.run_cli(["gate", "--repo", str(repo), "--base", base, "--head", head])
        self.assertEqual(code, 1)
        self.assertIn("CASE-COLLISION: legacy/macos/Tests/chattests.swift vs legacy/macos/Tests/ChatTests.swift", out)

    def test_cli_fails_on_protected_folder_that_differs_by_case(self):
        zones = json.dumps({"auto": ["docs/**", "!docs/decisions/**"], "new_files_only": []})
        repo = make_repo({"tools/guard/zones.json": zones, "docs/a.md": "x\n"})
        base = head_of(repo)
        head = commit(repo, {"docs/Decisions/0001.md": "x\n"})
        code, out = self.run_cli(["gate", "--repo", str(repo), "--base", base, "--head", head])
        self.assertEqual(code, 1)
        self.assertIn("CASE-COLLISION: docs/Decisions/0001.md", out)

    def test_cli_case_collision_passes_with_approval(self):
        zones = json.dumps({"auto": ["legacy/macos/Tests/**"], "new_files_only": ["legacy/macos/Tests/**"]})
        repo = make_repo({"tools/guard/zones.json": zones, "legacy/macos/Tests/ChatTests.swift": "final class ChatTests {}\n"})
        base = head_of(repo)
        head = commit_exact(repo, {"legacy/macos/Tests/chattests.swift": "final class ChatTests {}\n"})
        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
            json.dump([rev("dtthaison", "APPROVED", head)], f)
        try:
            code, _ = self.run_cli(["gate", "--repo", str(repo), "--base", base, "--head", head, "--reviews", f.name])
        finally:
            os.unlink(f.name)
        self.assertEqual(code, 0)

    def test_cli_passes_zone_only(self):
        repo, base, head = self.repo_with_zones({"legacy/macos/Sources/Notchling/Search/S.swift": "let a = 2\n"})
        self.assertEqual(self.run_cli(["gate", "--repo", str(repo), "--base", base, "--head", head])[0], 0)


if __name__ == "__main__":
    unittest.main()
