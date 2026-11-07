import json
import os
import shutil
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

from guard.rulesets import job_ids, validate

ROOT = Path(__file__).resolve().parents[3]
REAL_RULESETS = ROOT / ".github" / "rulesets"
REAL_WORKFLOWS = ROOT / ".github" / "workflows"
SCRIPT = ROOT / "tools" / "github" / "apply-settings.sh"

STUB = """name: Stub
jobs:
  build:
    runs-on: macos-latest
  guard:
    runs-on: ubuntu-latest
  app-build:  # comment
    runs-on: macos-latest
# the maintainer gate posts its own status: maintainer-gate
"""


def fixtures():
    """A copy of the real rulesets, with phone.yml's jobs and a stub for the guard, build and gate ones.

    The stub keeps these tests independent of the other workflows being edited."""
    tmp = Path(tempfile.mkdtemp())
    rs, wf = tmp / "rulesets", tmp / "workflows"
    shutil.copytree(REAL_RULESETS, rs)
    wf.mkdir()
    shutil.copy(REAL_WORKFLOWS / "phone.yml", wf / "phone.yml")
    (wf / "stub.yml").write_text(STUB)
    return rs, wf


def edit(path, fn):
    data = json.loads(path.read_text())
    fn(data)
    path.write_text(json.dumps(data))


def rule(data, kind):
    return next(r for r in data["rules"] if r["type"] == kind)


def without(kind):
    return lambda d: d.update(rules=[r for r in d["rules"] if r["type"] != kind])


class RulesetsTests(unittest.TestCase):
    def expect(self, mutate, name="main.json", needle=""):
        rs, wf = fixtures()
        edit(rs / name, mutate)
        out = validate(rs, wf)
        self.assertTrue(out, "expected a problem")
        self.assertTrue(all(p.startswith("GUARD-RULESETS:") for p in out), out)
        self.assertTrue(any(needle in p for p in out), (needle, out))

    def test_real_files_validate_clean(self):
        rs, wf = fixtures()
        self.assertEqual(validate(rs, wf), [])

    def test_real_workflows_have_the_jobs_they_provide(self):
        self.assertIn("build", job_ids((REAL_WORKFLOWS / "build.yml").read_text()))
        self.assertLessEqual({"phone-analyze", "phone-test"}, job_ids((REAL_WORKFLOWS / "phone.yml").read_text()))

    def test_job_ids(self):
        self.assertEqual(job_ids(STUB), {"build", "guard", "app-build"})
        self.assertEqual(job_ids("on: push\njobs:\n  a:\n    steps:\n      - run: x\n  b-c:\n"), {"a", "b-c"})

    def test_wrong_target_branch(self):
        self.expect(lambda d: d["conditions"]["ref_name"].update(include=["refs/heads/main"]), needle="~DEFAULT_BRANCH")

    def test_missing_deletion(self):
        self.expect(without("deletion"), needle="deletion")

    def test_missing_non_fast_forward(self):
        self.expect(without("non_fast_forward"), needle="non_fast_forward")

    def test_pull_request_flags(self):
        cases = [("require_code_owner_review", False), ("require_last_push_approval", False),
                 ("dismiss_stale_reviews_on_push", False), ("required_approving_review_count", 1),
                 ("allowed_merge_methods", ["merge", "squash"])]
        for key, bad in cases:
            with self.subTest(key=key):
                self.expect(lambda d: rule(d, "pull_request")["parameters"].update({key: bad}), needle=key)

    def test_missing_pull_request(self):
        self.expect(without("pull_request"), needle="pull_request")

    def test_strict_policy(self):
        self.expect(lambda d: rule(d, "required_status_checks")["parameters"].update(
            strict_required_status_checks_policy=True), needle="strict_required_status_checks_policy")

    def test_missing_context(self):
        def drop(d):
            p = rule(d, "required_status_checks")["parameters"]
            p["required_status_checks"] = [c for c in p["required_status_checks"] if c["context"] != "guard"]
        self.expect(drop, needle="guard")

    def test_wrong_integration_id(self):
        def bad(d):
            rule(d, "required_status_checks")["parameters"]["required_status_checks"][0]["integration_id"] = 1
        self.expect(bad, needle="integration_id")

    def test_bypass_actors(self):
        actor = [{"actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always"}]
        for name in ("main.json", "agent-branches.json", "release-tags.json"):
            with self.subTest(name=name):
                self.expect(lambda d: d.update(bypass_actors=actor), name=name, needle="bypass_actors")

    def test_agent_branches(self):
        self.expect(lambda d: d["conditions"]["ref_name"].update(include=["refs/heads/other/**"]),
                    name="agent-branches.json", needle="refs/heads/agent/**")
        self.expect(without("update"), name="agent-branches.json", needle="update")

    def test_release_tags(self):
        self.expect(lambda d: d.update(target="branch"), name="release-tags.json", needle="tag")
        self.expect(lambda d: d["conditions"]["ref_name"].update(include=["refs/tags/*"]),
                    name="release-tags.json", needle="refs/tags/v*")
        self.expect(without("creation"), name="release-tags.json", needle="creation")

    def test_missing_file(self):
        rs, wf = fixtures()
        (rs / "main.json").unlink()
        out = validate(rs, wf)
        self.assertTrue(out and out[0].startswith("GUARD-RULESETS:"))

    def test_context_without_job_fails(self):
        rs, wf = fixtures()
        (wf / "stub.yml").write_text(STUB.replace("app-build:", "other-job:"))
        out = validate(rs, wf)
        self.assertEqual(len(out), 1, out)
        self.assertIn("app-build", out[0])

    def test_maintainer_gate_needs_a_workflow_mention(self):
        rs, wf = fixtures()
        (wf / "stub.yml").write_text(STUB.replace("maintainer-gate", "nothing"))
        out = validate(rs, wf)
        self.assertEqual(len(out), 1, out)
        self.assertIn("maintainer-gate", out[0])


class DependabotTests(unittest.TestCase):
    def test_ecosystems(self):
        text = (ROOT / ".github" / "dependabot.yml").read_text()
        for eco in ("github-actions", "pub", "swift"):
            self.assertIn(f"package-ecosystem: {eco}", text)
        self.assertIn("directory: /mobile", text)
        self.assertEqual(text.count("interval: weekly"), 3)
        self.assertEqual(text.count("default-days: 7"), 3)
        self.assertNotIn("auto-merge", text)


class ApplySettingsTests(unittest.TestCase):
    def test_syntax(self):
        subprocess.run(["bash", "-n", str(SCRIPT)], check=True)

    def test_dry_run_prints_requests_and_never_calls_gh(self):
        bin_dir = Path(tempfile.mkdtemp())
        log = bin_dir / "gh.log"
        gh = bin_dir / "gh"
        gh.write_text(f'#!/bin/sh\necho "$@" >> "{log}"\nexit 99\n')
        gh.chmod(gh.stat().st_mode | stat.S_IXUSR)
        env = dict(os.environ, PATH=f"{bin_dir}:{os.environ['PATH']}")
        r = subprocess.run(["bash", str(SCRIPT), "--dry-run", "--repo", "example/repo"], env=env,
                           capture_output=True, text=True, timeout=60, stdin=subprocess.DEVNULL)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse(log.exists(), log.read_text() if log.exists() else "")
        for needle in ("PUT /repos/example/repo/actions/permissions", "POST /repos/example/repo/rulesets",
                       "DELETE /repos/example/repo/branches/main/protection",
                       "PUT /repos/example/repo/private-vulnerability-reporting",
                       "AGENT_DAILY_RUN_CAP", "REVIEW_DAILY_CAP", "spend limit", "actions/checkout@*"):
            self.assertIn(needle, r.stdout)
        self.assertNotIn("Apply?", r.stdout)


if __name__ == "__main__":
    unittest.main()
