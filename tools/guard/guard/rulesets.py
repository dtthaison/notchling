"""The repository rulesets in .github/rulesets/ say what the guard relies on, and CI checks them.

The JSON files are applied by tools/github/apply-settings.sh; nothing else stops someone from
loosening one in a pull request. Every required status check must also be a job (or, for the
maintainer gate, a posted status) in a workflow, or main would wait forever on a check that never
runs."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any, Dict, List, Set

GITHUB_ACTIONS_APP = 15368
# The maintainer can't approve his own pull request and is the only code owner, so repository admins
# (role 5) may merge a pull request without the review. Only through a pull request, never a push.
ADMIN_BYPASS = [{"actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "pull_request"}]
# `gh release create` makes the v* tag with GITHUB_TOKEN, which is the GitHub Actions app. Only a
# workflow holding contents: write can use this: release.yml, beta.yml's publish job and promote.yml's
# release, appcast and fold jobs, none with signing secrets (test_workflows pins that).
ACTIONS_BYPASS = [{"actor_id": GITHUB_ACTIONS_APP, "actor_type": "Integration", "bypass_mode": "always"}]
CONTEXTS = ["build", "app-build", "guard", "maintainer-gate", "phone-analyze", "phone-test",
            "rust-test", "rust-clippy", "rust-deny"]
POSTED_STATUSES = {"maintainer-gate"}
PULL_REQUEST = {
    "require_code_owner_review": True,
    "require_last_push_approval": True,
    "dismiss_stale_reviews_on_push": True,
    "required_approving_review_count": 0,
    "allowed_merge_methods": ["squash"],
}
JOBS_LINE = re.compile(r"^jobs:\s*(?:#.*)?$")
JOB_ID = re.compile(r"^  ([A-Za-z0-9_-]+):\s*(?:#.*)?$")


def job_ids(text: str) -> Set[str]:
    """Job ids of one workflow: the 2-space-indented keys under a top-level `jobs:` line."""
    ids: Set[str] = set()
    in_jobs = False
    for line in text.splitlines():
        if JOBS_LINE.match(line):
            in_jobs = True
        elif in_jobs and line and not line[0].isspace() and not line.startswith("#"):
            in_jobs = False
        elif in_jobs:
            m = JOB_ID.match(line)
            if m:
                ids.add(m.group(1))
    return ids


def _load(rulesets_dir: Path, name: str, problems: List[str]) -> Dict[str, Any]:
    try:
        data = json.loads((Path(rulesets_dir) / name).read_text())
    except (OSError, ValueError) as e:
        problems.append(f"GUARD-RULESETS: {name} cannot be read: {e}")
        return {}
    return data if isinstance(data, dict) else {}


def _rules(data: Dict[str, Any]) -> Dict[str, Dict[str, Any]]:
    return {r.get("type"): r.get("parameters") or {} for r in data.get("rules", []) if isinstance(r, dict)}


def _common(name: str, data: Dict[str, Any], target: str, include: str, types: List[str],
            problems: List[str], bypass: List[Dict[str, Any]]) -> Dict[str, Dict[str, Any]]:
    if data.get("target") != target:
        problems.append(f"GUARD-RULESETS: {name} must target a {target}")
    ref = (data.get("conditions") or {}).get("ref_name") or {}
    if ref.get("include") != [include]:
        problems.append(f"GUARD-RULESETS: {name} must include exactly {include}")
    if data.get("bypass_actors") != bypass:
        want = json.dumps(bypass, separators=(",", ":")) if bypass else "empty"
        problems.append(f"GUARD-RULESETS: {name} bypass_actors must be exactly {want}")
    if data.get("enforcement") != "active":
        problems.append(f"GUARD-RULESETS: {name} enforcement must be active")
    rules = _rules(data)
    for t in types:
        if t not in rules:
            problems.append(f"GUARD-RULESETS: {name} is missing the {t} rule")
    return rules


def _check_main(data: Dict[str, Any], problems: List[str]) -> List[str]:
    """Checks main.json; returns the required status contexts it names."""
    rules = _common("main.json", data, "branch", "~DEFAULT_BRANCH",
                    ["deletion", "non_fast_forward", "pull_request", "required_status_checks"], problems,
                    ADMIN_BYPASS)
    if "pull_request" in rules:
        for key, want in PULL_REQUEST.items():
            if rules["pull_request"].get(key) != want:
                problems.append(f"GUARD-RULESETS: main.json pull_request {key} must be {want!r}")
    checks = rules.get("required_status_checks")
    if checks is None:
        return []
    if checks.get("strict_required_status_checks_policy") is not False:
        problems.append("GUARD-RULESETS: main.json strict_required_status_checks_policy must be false")
    have = {c.get("context"): c.get("integration_id") for c in checks.get("required_status_checks", [])
            if isinstance(c, dict)}
    for ctx in CONTEXTS:
        if ctx not in have:
            problems.append(f"GUARD-RULESETS: main.json does not require {ctx}")
        elif have[ctx] != GITHUB_ACTIONS_APP:
            problems.append(f"GUARD-RULESETS: main.json {ctx} integration_id must be {GITHUB_ACTIONS_APP}")
    return [c for c in have if isinstance(c, str)]


def validate(rulesets_dir: Path, workflows_dir: Path) -> List[str]:
    problems: List[str] = []
    contexts = _check_main(_load(rulesets_dir, "main.json", problems), problems)
    _common("agent-branches.json", _load(rulesets_dir, "agent-branches.json", problems), "branch",
            "refs/heads/agent/**", ["creation", "update", "deletion"], problems, [])
    _common("release-tags.json", _load(rulesets_dir, "release-tags.json", problems), "tag",
            "refs/tags/v*", ["creation", "update", "deletion"], problems, ACTIONS_BYPASS)

    jobs: Set[str] = set()
    texts: List[str] = []
    d = Path(workflows_dir)
    for f in sorted(list(d.glob("*.yml")) + list(d.glob("*.yaml"))):
        text = f.read_text()
        texts.append(text)
        jobs |= job_ids(text)
    for ctx in contexts:
        if ctx in POSTED_STATUSES:
            if not any(ctx in t for t in texts):
                problems.append(f"GUARD-RULESETS: no workflow posts the {ctx} status")
        elif ctx not in jobs:
            problems.append(f"GUARD-RULESETS: required check {ctx} is not a job id in {workflows_dir}")
    return problems


def main() -> int:
    problems = validate(Path(".github/rulesets"), Path(".github/workflows"))
    if problems:
        print("\n".join(problems))
        return 1
    print("rulesets valid")
    return 0


if __name__ == "__main__":
    sys.exit(main())
