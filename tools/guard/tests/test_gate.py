import io
import json
import os
import tempfile
import unittest
from contextlib import redirect_stdout

from guard.__main__ import main
from guard.changes import diff
from guard.gate import MAINTAINERS, approved_head, decide, needs_maintainer
from guard.zones import Zones
from tests.helpers import commit, head_of, make_repo

H = "a" * 40
R = ["ZONE: build.sh"]
Z = Zones(("Sources/Notchling/Search/**",), ("Tests/**",))


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

    def test_summary_fits_status(self):
        self.assertLessEqual(len(decide(["ZONE: " + "x" * 300], [], H).summary), 140)


class NeedsMaintainerTests(unittest.TestCase):
    def test_needs_maintainer_combines(self):
        repo = make_repo({"build.sh": "echo 1\n", "Sources/Notchling/Search/S.swift": "let a = 1\n"})
        base = head_of(repo)
        commit(repo, {"build.sh": "echo 2\n", "Sources/Notchling/Search/S.swift": "let p = Process()\n"})
        changes = diff(repo, base, head_of(repo))
        self.assertEqual(sorted(r.split(":")[0] for r in needs_maintainer(changes, Z)), ["CAP-PROCESS", "ZONE"])

    def test_needs_maintainer_weakening(self):
        repo = make_repo({"Tests/A.swift": "func testA() {}\n"})
        base = head_of(repo)
        commit(repo, {"Tests/A.swift": "func testA() { throw XCTSkip(\"x\") }\n"})
        reasons = needs_maintainer(diff(repo, base, head_of(repo)), Z)
        self.assertIn("WEAK-TEST-EDIT", [r.split(":")[0] for r in reasons])

    def test_zone_only_change_is_clean(self):
        repo = make_repo({"Sources/Notchling/Search/S.swift": "let a = 1\n"})
        base = head_of(repo)
        commit(repo, {"Sources/Notchling/Search/S.swift": "let a = 2\n"})
        self.assertEqual(needs_maintainer(diff(repo, base, head_of(repo)), Z), [])


class GateCliTests(unittest.TestCase):
    def run_cli(self, argv):
        out = io.StringIO()
        with redirect_stdout(out):
            code = main(argv)
        return code, out.getvalue()

    def repo_with_zones(self, change):
        zones = json.dumps({"auto": ["Sources/Notchling/Search/**"], "new_files_only": []})
        repo = make_repo({"tools/guard/zones.json": zones, "build.sh": "echo 1\n",
                          "Sources/Notchling/Search/S.swift": "let a = 1\n"})
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

    def test_cli_passes_zone_only(self):
        repo, base, head = self.repo_with_zones({"Sources/Notchling/Search/S.swift": "let a = 2\n"})
        self.assertEqual(self.run_cli(["gate", "--repo", str(repo), "--base", base, "--head", head])[0], 0)


if __name__ == "__main__":
    unittest.main()
