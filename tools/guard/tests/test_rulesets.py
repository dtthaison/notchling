import json
import os
import re
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

ADMIN_ON_PR = {"actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "pull_request"}
ACTIONS_APP = {"actor_id": 15368, "actor_type": "Integration", "bypass_mode": "always"}

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

    def test_main_lets_only_repository_admins_bypass_through_a_pull_request(self):
        # The maintainer can't approve his own pull request, and he is the only code owner.
        self.assertEqual(json.loads((REAL_RULESETS / "main.json").read_text())["bypass_actors"], [ADMIN_ON_PR])
        bad = [[], [dict(ADMIN_ON_PR, bypass_mode="always")], [dict(ADMIN_ON_PR, actor_id=4)],
               [dict(ADMIN_ON_PR, actor_id=2)], [dict(ADMIN_ON_PR, actor_type="Team")],
               [ADMIN_ON_PR, ACTIONS_APP], [ACTIONS_APP], [dict(ADMIN_ON_PR, extra=1)]]
        for actors in bad:
            with self.subTest(actors=actors):
                self.expect(lambda d: d.update(bypass_actors=actors), name="main.json", needle="bypass_actors")

    def test_release_tags_let_only_the_actions_app_bypass(self):
        # release.yml's `gh release create` makes the v* tag with GITHUB_TOKEN.
        self.assertEqual(json.loads((REAL_RULESETS / "release-tags.json").read_text())["bypass_actors"],
                         [ACTIONS_APP])
        bad = [[], [dict(ACTIONS_APP, bypass_mode="pull_request")], [dict(ACTIONS_APP, actor_id=1)],
               [dict(ACTIONS_APP, actor_type="RepositoryRole")], [ACTIONS_APP, ADMIN_ON_PR], [ADMIN_ON_PR]]
        for actors in bad:
            with self.subTest(actors=actors):
                self.expect(lambda d: d.update(bypass_actors=actors), name="release-tags.json",
                            needle="bypass_actors")

    def test_agent_branches_have_no_bypass(self):
        self.assertEqual(json.loads((REAL_RULESETS / "agent-branches.json").read_text())["bypass_actors"], [])
        for actors in ([ADMIN_ON_PR], [ACTIONS_APP], [dict(ADMIN_ON_PR, bypass_mode="always")]):
            with self.subTest(actors=actors):
                self.expect(lambda d: d.update(bypass_actors=actors), name="agent-branches.json",
                            needle="bypass_actors")

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
        self.assertIn("directory: /apps/flutter", text)
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

    def test_dry_run_says_who_can_bypass_and_why(self):
        r = subprocess.run(["bash", str(SCRIPT), "--dry-run", "--repo", "example/repo"], capture_output=True,
                           text=True, timeout=60, stdin=subprocess.DEVNULL)
        self.assertEqual(r.returncode, 0, r.stderr)
        # The requests carry the actors ...
        self.assertIn('"actor_type":"RepositoryRole"', r.stdout)
        self.assertIn('"actor_type":"Integration"', r.stdout)
        # ... and a note names them, with the reason for the release one.
        note = r.stdout.split("Bypass", 1)[1]
        for needle in ("repository admins", "pull request", "GitHub Actions", "15368", "release.yml", "v*"):
            self.assertIn(needle, note)

    def send_function(self):
        """The script's `send` function, alone: tests run it against a fake gh, never the script."""
        m = re.search(r"(?ms)^send\(\) \{.*?^\}\n", SCRIPT.read_text())
        self.assertIsNotNone(m, "apply-settings.sh must define send()")
        return m.group(0)

    def run_send(self, method, path, gh_stderr, gh_rc, body=""):
        tmp = Path(tempfile.mkdtemp())
        gh = tmp / "gh"
        gh.write_text(f'#!/bin/sh\necho "$@" >> "{tmp}/calls"\ncat >/dev/null\n'
                      f'echo \'{gh_stderr}\' >&2\nexit {gh_rc}\n')
        gh.chmod(gh.stat().st_mode | stat.S_IXUSR)
        script = tmp / "send.sh"
        script.write_text("set -euo pipefail\n" + self.send_function() +
                          f'send {method} {path} \'{body}\'\necho "after: reached"\n')
        env = dict(os.environ, PATH=f"{tmp}:{os.environ['PATH']}")
        r = subprocess.run(["bash", str(script)], env=env, capture_output=True, text=True, timeout=60,
                           stdin=subprocess.DEVNULL)
        return r, (tmp / "calls").read_text() if (tmp / "calls").exists() else ""

    def test_deleting_missing_classic_protection_is_not_an_error(self):
        r, calls = self.run_send("DELETE", "/repos/example/repo/branches/main/protection",
                                 "gh: Branch not protected (HTTP 404)", 1)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("after: reached", r.stdout)
        self.assertIn("api -X DELETE repos/example/repo/branches/main/protection", calls)
        self.assertIn("nothing to remove", r.stdout)

    def test_other_failures_still_stop_the_run(self):
        cases = [("DELETE", "/repos/example/repo/branches/main/protection", "gh: Server Error (HTTP 500)", ""),
                 ("DELETE", "/repos/example/repo/actions/variables/X", "gh: Not Found (HTTP 404)", ""),
                 ("PATCH", "/repos/example/repo", "gh: Not Found (HTTP 404)", '{"a":1}'),
                 ("PUT", "/repos/example/repo/rulesets/1", "gh: Validation Failed (HTTP 422)", '{"a":1}')]
        for method, path, err, body in cases:
            with self.subTest(method=method, path=path, err=err):
                r, _ = self.run_send(method, path, err, 1, body)
                self.assertNotEqual(r.returncode, 0)
                self.assertNotIn("after: reached", r.stdout)
                self.assertIn(err, r.stdout + r.stderr)  # the error stays visible

    def test_send_sends_the_body_on_stdin(self):
        r, calls = self.run_send("PUT", "/repos/example/repo/rulesets/1", "", 0, '{"a":1}')
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("api -X PUT repos/example/repo/rulesets/1 --input -", calls)

    def test_the_apply_loop_goes_through_send(self):
        self.assertRegex(SCRIPT.read_text(), r'(?m)^  send "\$\{METHODS\[\$i\]\}" "\$\{PATHS\[\$i\]\}" "\$\{BODIES\[\$i\]\}"')


if __name__ == "__main__":
    unittest.main()
