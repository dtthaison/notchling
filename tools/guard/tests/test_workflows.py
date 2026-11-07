"""The workflows' security contract, read as text (no YAML library)."""

import os
import re
import stat
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
WORKFLOWS = ROOT / ".github/workflows"


def read(path):
    """The workflow without its comment lines, which may name what they forbid."""
    return "".join(l for l in (ROOT / path).read_text().splitlines(keepends=True) if not l.lstrip().startswith("#"))


FETCH = 'origin "pull/'  # the head is fetched with an auth header: git -c ... fetch --no-tags origin "pull/<n>/head"


def jobs(text):
    block = text.split("\njobs:\n", 1)[1]
    return re.findall(r"^  ([A-Za-z0-9_-]+):", block, re.M)


GATE = ".github/workflows/maintainer-gate.yml"
DEFAULT_BRANCH_REF = "${{ github.event.repository.default_branch }}"


def step_named(text, prefix):
    """The text of the step whose `name:` starts with prefix (steps start at 6 spaces of indent)."""
    for step in re.split(r"(?m)^(?=      - )", text):
        if re.match(r"      - name: " + re.escape(prefix), step):
            return step
    raise AssertionError("no step named " + prefix)


def run_script(step):
    """The `run: |` block of a step, dedented."""
    m = re.search(r"run: \|\n((?:[ ]{10,}.*\n?|\n)+)", step)
    assert m, step
    return textwrap.dedent(m.group(1))


class WorkflowTests(unittest.TestCase):
    def test_gate_workflow_checks_out_base_only(self):
        text = read(GATE)
        self.assertIn("pull_request_target", text)
        self.assertIn("workflow_run", text)
        self.assertNotIn("github.event.pull_request.head", text)  # the PR head is never a ref
        # The one checkout names the default branch; any other ref: is refused.
        refs = re.findall(r"(?m)^\s*(?:-\s*)?ref:\s*(.*?)\s*$", text)
        self.assertEqual(refs, [DEFAULT_BRANCH_REF])
        self.assertEqual(text.count("actions/checkout@"), 1)
        self.assertIn("persist-credentials: false", text)
        self.assertNotIn("actions/checkout@", text.split(FETCH)[1])  # no second checkout of PR code

    def test_gate_judges_only_prs_into_main_and_reruns_on_retarget(self):
        text = read(GATE)
        block = text.split("pull_request_target:", 1)[1].split("workflow_run:", 1)[0]
        self.assertRegex(block, r"(?m)^\s+branches: \[main\]\s*$")
        types = re.search(r"types: \[(.*?)\]", block).group(1).replace(" ", "").split(",")
        self.assertIn("edited", types)  # a retarget to main is an `edited` event
        self.assertLessEqual({"opened", "synchronize", "reopened", "ready_for_review"}, set(types))

    def test_gate_merge_base_is_against_the_default_branch(self):
        text = read(GATE)
        self.assertIn("DEFAULT_BRANCH: " + DEFAULT_BRANCH_REF, text)
        self.assertNotIn("BASE_REF", text)  # the PR's own base never decides what counts as changed
        mb = [l for l in text.splitlines() if "merge-base" in l]
        self.assertEqual(len(mb), 1, mb)
        self.assertIn('"origin/$DEFAULT_BRANCH"', mb[0])
        self.assertNotIn("base_ref", mb[0])
        fetch = [l for l in text.splitlines() if "fetch --no-tags" in l]
        self.assertEqual(len(fetch), 1, fetch)
        self.assertIn("refs/heads/$DEFAULT_BRANCH:refs/remotes/origin/$DEFAULT_BRANCH", fetch[0])

    def test_gate_steps_after_lookup_run_only_when_it_says_post(self):
        text = read(GATE)
        for name in ("Fetch the PR head", "Decide and post"):
            self.assertRegex(step_named(text, name), r"(?m)^        if: steps\.pr\.outputs\.post == 'true'\s*$", name)

    def test_gate_lookup_exits_without_posting_when_base_is_not_the_default_branch(self):
        script = run_script(step_named(read(GATE), "Find the pull request"))
        sha = "0123456789abcdef0123456789abcdef01234567"
        cases = [  # (event PR, workflow_run pull_requests, base of the PR, posts?)
            ("7", "null", "main", True),
            ("7", "null", "agent/feature-x", False),
            ("7", "null", "release/1.0", False),
            ("", '[{"number": 7}]', "main", True),
            ("", '[{"number": 7}]', "feature-b", False),
        ]
        for event_pr, run_prs, base, posts in cases:
            with self.subTest(event_pr=event_pr, base=base):
                tmp = Path(tempfile.mkdtemp())
                gh = tmp / "bin" / "gh"
                gh.parent.mkdir()
                gh.write_text('#!/bin/sh\ncase "$*" in\n'
                              '  *.head.sha*) echo ' + sha + ' ;;\n'
                              '  *.base.ref*) echo "$FAKE_BASE" ;;\n'
                              '  *) echo "unexpected gh call: $*" >&2; exit 98 ;;\nesac\n')
                gh.chmod(gh.stat().st_mode | stat.S_IXUSR)
                (tmp / "find.sh").write_text(script)
                out = tmp / "out"
                out.write_text("")
                env = dict(os.environ, PATH=str(gh.parent) + os.pathsep + os.environ["PATH"], GH_REPO="o/r",
                           EVENT_PR=event_pr, RUN_PRS=run_prs, RUN_HEAD=sha, DEFAULT_BRANCH="main",
                           FAKE_BASE=base, GITHUB_OUTPUT=str(out))
                r = subprocess.run(["bash", str(tmp / "find.sh")], env=env, capture_output=True, text=True,
                                   timeout=60, stdin=subprocess.DEVNULL)
                self.assertEqual(r.returncode, 0, r.stderr)  # exits cleanly, never fails the check run
                got = out.read_text()
                self.assertEqual("post=true" in got.splitlines(), posts, got)
                if posts:
                    self.assertIn("number=7", got.splitlines())
                    self.assertIn("head=" + sha, got.splitlines())
                else:
                    self.assertNotIn("number=", got)

    def test_only_the_release_workflow_can_write_contents(self):
        # release-tags lets the GitHub Actions app create v* tags; that is safe only while the one
        # workflow that holds a contents: write token is release.yml.
        writers = [f.name for f in sorted(WORKFLOWS.glob("*.yml")) if re.search(r"contents:\s*write", read(f".github/workflows/{f.name}"))]
        self.assertEqual(writers, ["release.yml"])

    def test_phone_checks_run_on_every_pull_request(self):
        # phone-analyze and phone-test are required on main; a paths: filter leaves them pending forever
        # on a pull request that touches nothing under mobile/.
        text = read(".github/workflows/phone.yml")
        self.assertNotRegex(text, r"(?m)^\s*paths(-ignore)?:")
        self.assertRegex(text, r"(?m)^  pull_request:\s*$")

    def test_gate_workflow_never_runs_pr_code(self):
        text = read(".github/workflows/maintainer-gate.yml")
        after_fetch = text.split(FETCH)[1]
        self.assertNotRegex(after_fetch, r"git (checkout|switch|worktree)")
        self.assertIn("--post", text)
        self.assertRegex(text, r"statuses:\s*write")

    def test_gate_untrusted_values_go_through_env(self):
        # Event fields are data; `${{ }}` inside `run:` would splice them into the shell.
        text = read(".github/workflows/maintainer-gate.yml")
        for run in re.findall(r"run: \|\n((?:\s{10,}.*\n?)+)", text):
            self.assertNotIn("${{", run)

    def test_review_signal_has_no_permissions(self):
        text = read(".github/workflows/review-signal.yml")
        self.assertRegex(text, r"permissions:\s*\{\}")
        self.assertIn("name: Review signal", text)
        self.assertNotIn("actions/checkout", text)

    def test_code_jobs_have_no_secrets(self):
        for f in ("guard.yml", "build.yml", "phone.yml", "rust.yml"):
            self.assertNotIn("secrets.", read(".github/workflows/" + f), f)

    def test_every_workflow_sets_permissions(self):
        for f in sorted(WORKFLOWS.glob("*.yml")):
            self.assertRegex(f.read_text(), r"(?m)^permissions:", f.name)

    def test_required_check_jobs_exist(self):
        found = {j for f in WORKFLOWS.glob("*.yml") for j in jobs(f.read_text())}
        for name in ("build", "app-build", "guard", "phone-analyze", "phone-test", "rust-test", "rust-clippy", "rust-deny"):
            self.assertIn(name, found)

    def test_guard_job_runs_every_check(self):
        text = read(".github/workflows/guard.yml")
        for cmd in ("unittest discover", "guard lint", "guard pins", "guard codeowners --check", "guard rulesets"):
            self.assertIn(cmd, text)

    def test_pr_workflows_dont_persist_credentials(self):
        for f in ("guard.yml", "build.yml", "phone.yml", "rust.yml", "maintainer-gate.yml"):
            text = read(".github/workflows/" + f)
            self.assertEqual(text.count("actions/checkout@"), text.count("persist-credentials: false"), f)


if __name__ == "__main__":
    unittest.main()
