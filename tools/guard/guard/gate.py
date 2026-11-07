"""The `maintainer-gate` decision: does this pull request need the maintainer, and has he approved it?

A PR needs the maintainer when it touches a path outside the auto-merge zone, uses a capability
(processes, ptys, network, Keychain, file writes, protected types) or weakens a test. It then
passes only with a maintainer's approval on the PR's current head; a later push needs a new one."""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, List, Optional

from guard import capability, weakening
from guard.changes import Change
from guard.zones import Zones, protected_paths

MAINTAINERS = frozenset({"dtthaison"})
STATUS_CONTEXT = "maintainer-gate"
SUMMARY_MAX = 140


@dataclass(frozen=True)
class Decision:
    passed: bool
    summary: str
    details: List[str] = field(default_factory=list)


def needs_maintainer(changes: List[Change], zones: Zones) -> List[str]:
    reasons = [f"ZONE: {p}" for p in protected_paths(changes, zones)]
    for t in capability.scan(changes) + weakening.scan(changes):
        where = f"{t.path}:{t.line}" if t.line else t.path
        reasons.append(f"{t.rule}: {where} {t.text}".rstrip())
    return reasons


def approved_head(reviews: List[Dict], head_sha: str, maintainers: frozenset) -> bool:
    latest: Dict[str, Dict] = {}
    for r in sorted(reviews, key=lambda r: r.get("submitted_at") or ""):
        login = (r.get("user") or {}).get("login")
        if login in maintainers and r.get("state") in ("APPROVED", "CHANGES_REQUESTED"):
            latest[login] = r
    return any(r["state"] == "APPROVED" and r.get("commit_id") == head_sha for r in latest.values())


def _fit(text: str) -> str:
    return text if len(text) <= SUMMARY_MAX else text[:SUMMARY_MAX - 1] + "…"


def decide(reasons: List[str], reviews: List[Dict], head_sha: str) -> Decision:
    if not reasons:
        return Decision(True, "Nothing protected", [])
    if approved_head(reviews, head_sha, MAINTAINERS):
        return Decision(True, _fit(f"Approved by the maintainer at {head_sha[:7]}"), list(reasons))
    more = f" (+{len(reasons) - 1} more)" if len(reasons) > 1 else ""
    first = reasons[0]
    room = SUMMARY_MAX - len("Needs the maintainer: ") - len(more)
    if len(first) > room:
        first = first[:room - 1] + "…"
    return Decision(False, f"Needs the maintainer: {first}{more}", list(reasons))


def post_status(head_sha: str, decision: Decision, target_url: Optional[str] = None) -> None:
    """Only the workflow calls this (it has GH_TOKEN and GH_REPO); tests never reach `gh`."""
    args = ["gh", "api", "--method", "POST", f"repos/{{owner}}/{{repo}}/statuses/{head_sha}",
            "-f", f"state={'success' if decision.passed else 'failure'}",
            "-f", f"context={STATUS_CONTEXT}", "-f", f"description={decision.summary}"]
    if target_url:
        args += ["-f", f"target_url={target_url}"]
    subprocess.run(args, check=True, stdout=subprocess.DEVNULL)


def main(repo: Path, base: str, head: str, reviews_file: Optional[Path], post: bool,
         target_url: Optional[str] = None) -> int:
    from guard.changes import diff
    from guard.zones import load

    zones = load(Path(repo) / "tools/guard/zones.json")
    reviews = json.loads(Path(reviews_file).read_text()) if reviews_file else []
    head_sha = subprocess.run(["git", "-C", str(repo), "rev-parse", head], check=True,
                              capture_output=True, text=True).stdout.strip()
    decision = decide(needs_maintainer(diff(Path(repo), base, head), zones), reviews, head_sha)
    for line in decision.details:
        print(line)
    print(("PASS " if decision.passed else "FAIL ") + decision.summary)
    if post:
        post_status(head_sha, decision, target_url)
    return 0 if decision.passed else 1
