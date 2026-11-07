"""The workflows' security contract, read as text (no YAML library)."""

import re
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


class WorkflowTests(unittest.TestCase):
    def test_gate_workflow_checks_out_base_only(self):
        text = read(".github/workflows/maintainer-gate.yml")
        self.assertIn("pull_request_target", text)
        self.assertIn("workflow_run", text)
        self.assertNotRegex(text, r"ref:\s*\$\{\{\s*github\.event\.pull_request\.head")
        self.assertNotRegex(text, r"ref:")
        self.assertNotIn("actions/checkout@", text.split(FETCH)[1])  # no second checkout of PR code

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
        for f in ("guard.yml", "build.yml", "phone.yml"):
            self.assertNotIn("secrets.", read(".github/workflows/" + f), f)

    def test_every_workflow_sets_permissions(self):
        for f in sorted(WORKFLOWS.glob("*.yml")):
            self.assertRegex(f.read_text(), r"(?m)^permissions:", f.name)

    def test_required_check_jobs_exist(self):
        found = {j for f in WORKFLOWS.glob("*.yml") for j in jobs(f.read_text())}
        for name in ("build", "app-build", "guard", "phone-analyze", "phone-test"):
            self.assertIn(name, found)

    def test_guard_job_runs_every_check(self):
        text = read(".github/workflows/guard.yml")
        for cmd in ("unittest discover", "guard lint", "guard pins", "guard codeowners --check", "guard rulesets"):
            self.assertIn(cmd, text)

    def test_pr_workflows_dont_persist_credentials(self):
        for f in ("guard.yml", "build.yml", "phone.yml", "maintainer-gate.yml"):
            text = read(".github/workflows/" + f)
            self.assertEqual(text.count("actions/checkout@"), text.count("persist-credentials: false"), f)


if __name__ == "__main__":
    unittest.main()
