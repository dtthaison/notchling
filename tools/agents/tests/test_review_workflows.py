"""The review workflows' security shape, read as text (no YAML library), and their PR lookup run for real."""

import json
import os
import re
import stat
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
RUN = ".github/workflows/review-run.yml"
POST = ".github/workflows/review-post.yml"
DEFAULT_BRANCH_REF = "${{ github.event.repository.default_branch }}"


def raw(path):
    return (ROOT / path).read_text()


def read(path):
    """The workflow without its comment lines, which may name what they forbid."""
    return "".join(l for l in raw(path).splitlines(keepends=True) if not l.lstrip().startswith("#"))


def run_blocks(text):
    return re.findall(r"run: \|\n((?:\s{10,}.*\n?)+)", text)


def step_named(text, prefix):
    for step in re.split(r"(?m)^(?=      - )", text):
        if re.match(r"      - name: " + re.escape(prefix), step):
            return step
    raise AssertionError("no step named " + prefix)


def run_script(step):
    m = re.search(r"run: \|\n((?:[ ]{10,}.*\n?|\n)+)", step)
    assert m, step
    return textwrap.dedent(m.group(1))


class ReviewRunTests(unittest.TestCase):
    def test_untrusted_pr_job_has_no_permissions_secrets_or_actions(self):
        text = read(RUN)
        self.assertRegex(text, r"(?m)^permissions:\s*\{\}\s*$")
        self.assertIn("name: Review run", text)
        self.assertRegex(text, r"(?m)^  pull_request:\s*$")
        self.assertNotIn("pull_request_target", text)
        self.assertNotIn("secrets.", text)
        self.assertNotIn("uses:", text)  # nothing from the PR is checked out or run, and no action is pulled in
        self.assertNotIn("GITHUB_TOKEN", text)
        self.assertNotIn("github.token", text)

    def test_event_fields_reach_the_shell_only_through_env(self):
        for run in run_blocks(read(RUN)):
            self.assertNotIn("${{", run)

    def test_it_records_the_pr_number_and_head_as_outputs(self):
        text = read(RUN)
        self.assertIn("github.event.pull_request.number", text)
        self.assertIn("github.event.pull_request.head.sha", text)
        self.assertIn("GITHUB_OUTPUT", text)

    def test_a_label_event_only_for_the_review_label(self):
        text = read(RUN)
        types = re.search(r"types: \[(.*?)\]", text).group(1).replace(" ", "").split(",")
        self.assertEqual(set(types), {"opened", "synchronize", "ready_for_review", "labeled"})
        self.assertIn("github.event.label.name == 'notchling:review'", text)

    def test_reopening_does_not_start_a_run(self):
        """A reopen brings no new commit, so it would only ask the model about a head it has already judged."""
        self.assertNotIn("reopened", read(RUN))

    def test_drafts_are_skipped(self):
        job = re.search(r"(?m)^    if: (.*)$", read(RUN)).group(1)
        self.assertIn("github.event.pull_request.draft == false", job)
        self.assertIn("github.event.label.name == 'notchling:review'", job)


class ReviewPostTests(unittest.TestCase):
    def test_trusted_job_permissions_are_the_minimum(self):
        """Nothing at the top; the one job asks for what review.py uses: the diff (contents), the check
        results (checks), comments and labels (pull requests) and the commit status (statuses)."""
        text = read(POST)
        self.assertRegex(text, r"(?m)^permissions: \{\}\s*$")
        self.assertNotRegex(text, r"(?m)^permissions:\n")
        job = text.split("\njobs:\n", 1)[1]
        m = re.search(r"(?m)^    permissions:\n((?:      [a-z-]+: (?:read|write)\n)+)", job)
        self.assertIsNotNone(m, "the job needs its own permissions block")
        granted = dict(re.findall(r"([a-z-]+): (read|write)", m.group(1)))
        self.assertEqual(granted, {"checks": "read", "contents": "read", "pull-requests": "write", "statuses": "write"})
        self.assertEqual(text.count("permissions:"), 2)
        self.assertNotRegex(text, r"(?m)^\s*(issues|actions|id-token|packages|deployments):")
        self.assertNotRegex(text, r"checks:\s*write")
        self.assertNotRegex(text, r"contents:\s*write")

    def test_a_run_is_never_cancelled_after_it_may_have_asked_the_model(self):
        """A cancelled run could lose the comment that records the model call, and the next one would ask again."""
        text = read(POST)
        self.assertRegex(text, r"(?m)^concurrency:\n  group: review-post-.*\n  cancel-in-progress: false\n")

    def test_the_daily_cap_variable_reaches_review_py(self):
        text = read(POST)
        self.assertRegex(text, r"(?m)^      REVIEW_DAILY_CAP: \$\{\{ vars\.REVIEW_DAILY_CAP \}\}$")
        self.assertNotIn("secrets.REVIEW_DAILY_CAP", text)

    def test_it_waits_on_review_run_and_only_when_that_run_succeeded(self):
        text = read(POST)
        self.assertRegex(text, r"workflow_run:\n    workflows: \[\"Review run\"\]\n    types: \[completed\]")
        self.assertNotRegex(text, r"(?m)^  pull_request")
        self.assertIn("github.event.workflow_run.conclusion == 'success'", text)
        self.assertIn("github.event.workflow_run.event == 'pull_request'", text)

    def test_checks_out_the_default_branch_only(self):
        text = read(POST)
        refs = re.findall(r"(?m)^\s*(?:-\s*)?ref:\s*(.*?)\s*$", text)
        self.assertEqual(refs, [DEFAULT_BRANCH_REF])
        self.assertEqual(text.count("actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7"), 1)
        self.assertEqual(text.count("actions/checkout@"), 1)
        self.assertEqual(text.count("persist-credentials: false"), 1)
        self.assertEqual(re.findall(r"(?m)^\s*(?:-\s+)?uses: (\S+)", text), ["actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"])
        self.assertNotIn("github.event.pull_request.head", text)
        for line in text.splitlines():  # the run's head fields may only be copied into env: for the lookup step
            if "workflow_run.head_branch" in line or "workflow_run.head_repository" in line:
                self.assertRegex(line, r"^ {10}RUN_HEAD_(BRANCH|REPO): \$\{\{ github\.event\.workflow_run\.head_(branch|repository\.full_name) \}\}$")
        self.assertNotRegex(text, r"git (checkout|switch|worktree|fetch|clone|merge|cherry-pick)")

    def test_event_fields_reach_the_shell_only_through_env(self):
        text = read(POST)
        for run in run_blocks(text):
            self.assertNotIn("${{", run)
        self.assertNotIn("github.head_ref", text)
        self.assertNotIn("pull_request.title", text)
        self.assertNotIn("pull_request.body", text)

    def test_the_only_secret_is_the_model_key_and_it_is_in_one_step(self):
        text = read(POST)
        self.assertEqual(re.findall(r"secrets\.(\w+)", text), ["ANTHROPIC_API_KEY"])
        step = step_named(text, "Review")
        self.assertIn("secrets.ANTHROPIC_API_KEY", step)
        self.assertIn("vars.AGENT_APP_LOGIN", text)

    def test_it_runs_review_py_from_the_default_branch_and_not_with_the_lookup_step(self):
        text = read(POST)
        self.assertIn("python3 tools/agents/review.py", text)
        self.assertNotIn("ANTHROPIC_API_KEY", step_named(text, "Find the pull request"))

    def test_the_workflow_never_asks_for_approval_or_merge(self):
        text = read(POST)
        for word in ("auto-merge", "gh pr merge", "gh pr review", "--approve", "/merge", "requested_reviewers"):
            self.assertNotIn(word, text)

    def test_header_says_recommend_only_and_not_required(self):
        head = raw(POST).split("\non:\n", 1)[0]
        self.assertIn("recommend-only", head)
        self.assertIn("review-agent", head)
        self.assertRegex(head, r"NOT .*required")

    def test_review_agent_is_not_a_required_check(self):
        for f in sorted((ROOT / ".github/rulesets").glob("*.json")):
            self.assertNotIn("review-agent", f.read_text(), f.name)


class LookupTests(unittest.TestCase):
    """Run the 'Find the pull request' step for real (with review.py from this tree) and a fake gh."""

    SHA = "0123456789abcdef0123456789abcdef01234567"
    APP_PR = {"number": 7, "state": "open", "head": {"repo": {"full_name": "o/r"}, "ref": "agent/1-x", "sha": SHA}}
    FORK_PR = {"number": 12, "state": "open", "head": {"repo": {"full_name": "mallory/r"}, "ref": "agent/1-x", "sha": SHA}}

    def lookup(self, listed, repo="o/r", branch="agent/1-x", path=".github/workflows/review-run.yml", gh_fails=False,
               head=None):
        script = run_script(step_named(read(POST), "Find the pull request"))
        tmp = Path(tempfile.mkdtemp())
        gh = tmp / "bin" / "gh"
        gh.parent.mkdir()
        gh.write_text('#!/bin/sh\ncase "$*" in\n'
                      '  *commits/*/pulls*) [ -n "$FAKE_FAIL" ] && { echo "HTTP 500" >&2; exit 1; }; printf %s "$FAKE_PRS"; exit 0 ;;\n'
                      '  *) echo "unexpected gh call: $*" >&2; exit 98 ;;\nesac\n')
        gh.chmod(gh.stat().st_mode | stat.S_IXUSR)
        (tmp / "find.sh").write_text(script)
        out = tmp / "out"
        out.write_text("")
        env = dict(os.environ, PATH=str(gh.parent) + os.pathsep + os.environ["PATH"], GH_REPO="o/r",
                   RUN_HEAD=head or self.SHA, RUN_HEAD_REPO=repo, RUN_HEAD_BRANCH=branch, RUN_PATH=path,
                   GITHUB_OUTPUT=str(out), FAKE_PRS=json.dumps(listed), FAKE_FAIL="1" if gh_fails else "")
        r = subprocess.run(["bash", str(tmp / "find.sh")], env=env, capture_output=True, text=True, timeout=60,
                           stdin=subprocess.DEVNULL, cwd=str(ROOT))
        return r, out.read_text()

    def test_the_number_of_the_one_matching_pull_request(self):
        r, out = self.lookup([self.APP_PR])
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("number=7", out.splitlines())
        self.assertIn("post=true", out.splitlines())

    def test_a_fork_with_the_apps_head_commit_reviews_its_own_pull_request_not_the_apps(self):
        both = [self.APP_PR, self.FORK_PR]
        r, out = self.lookup(both, repo="mallory/r")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("number=12", out.splitlines())
        self.assertNotIn("number=7", out.splitlines())
        r, out = self.lookup(both, repo="o/r")
        self.assertIn("number=7", out.splitlines())

    def test_no_match_posts_nothing_and_does_not_fail_the_run(self):
        for name, kw in {"no pull request": dict(listed=[]), "a repository that is not its head": dict(listed=[self.APP_PR], repo="mallory/r"),
                         "another branch": dict(listed=[self.APP_PR], branch="agent/2-y"),
                         "another commit": dict(listed=[self.APP_PR], head="f" * 40),
                         "two matches": dict(listed=[self.APP_PR, dict(self.APP_PR, number=8)])}.items():
            with self.subTest(name):
                r, out = self.lookup(**kw)
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertNotIn("number=", out)
                self.assertNotIn("post=true", out)

    def test_a_run_of_any_other_workflow_file_is_not_reviewed(self):
        for path in (".github/workflows/evil.yml", "", ".github/workflows/review-run.yml.bak"):
            with self.subTest(path=path):
                r, out = self.lookup([self.APP_PR], path=path)
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertNotIn("number=", out)

    def test_the_workflow_path_may_carry_a_ref(self):
        r, out = self.lookup([self.APP_PR], path=".github/workflows/review-run.yml@refs/pull/7/merge")
        self.assertIn("number=7", out.splitlines())

    def test_a_github_error_fails_the_step_and_posts_nothing(self):
        r, out = self.lookup([self.APP_PR], gh_fails=True)
        self.assertNotEqual(r.returncode, 0)
        self.assertNotIn("number=", out)

    def test_a_bad_branch_name_is_data_not_shell(self):
        r, out = self.lookup([self.APP_PR], branch="x; echo pwned > /tmp/rv-pwned; $(id)")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertNotIn("number=", out)
        self.assertFalse(Path("/tmp/rv-pwned").exists())


if __name__ == "__main__":
    unittest.main()
