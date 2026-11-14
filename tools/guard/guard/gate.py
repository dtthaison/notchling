from __future__ import annotations

import json
import subprocess
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, Iterable, List, Optional, Set, Tuple

from guard import capability, lint, weakening
from guard.changes import Change
from guard.zones import Zones, case_shadowed, fold, protected_paths

MAINTAINERS = frozenset({"dtthaison"})
STATUS_CONTEXT = "maintainer-gate"
SUMMARY_MAX = 140
PROVIDER_FILES = (
    "crates/engine/src/contract/gaps.json",
    "crates/engine/src/contract/unwired.json",
)


@dataclass(frozen=True)
class Decision:
    passed: bool
    summary: str
    details: List[str] = field(default_factory=list)


def needs_maintainer(changes: List[Change], zones: Zones) -> List[str]:
    reasons = [f"ZONE: {p}" for p in protected_paths(changes, zones)]
    reasons += [f"CASE-COLLISION: {p} vs a protected path" for p in case_shadowed(changes, zones)]
    for t in capability.scan(changes) + weakening.scan(changes):
        where = f"{t.path}:{t.line}" if t.line else t.path
        reasons.append(f"{t.rule}: {where} {t.text}".rstrip())
    return reasons


def added_paths(changes: List[Change]) -> List[str]:
    return [c.path for c in changes if c.status in ("A", "R")]


def case_collisions(added: Iterable[str], tree: Iterable[str]) -> List[str]:
    names: Dict[str, Set[Tuple[str, bool]]] = {}
    for path in tree:
        parts = path.split("/")
        for i in range(1, len(parts)):
            name = "/".join(parts[:i])
            names.setdefault(fold(name), set()).add((name, True))
        names.setdefault(fold(path), set()).add((path, False))

    def clash(name: str) -> Optional[str]:
        others = sorted(n + ("/" if is_dir else "") for n, is_dir in names.get(fold(name), ()) if n != name)
        return others[0] if others else None

    out: List[str] = []
    for path in added:
        parts = path.split("/")
        for i in range(1, len(parts) + 1):
            other = clash("/".join(parts[:i]))
            if other:
                out.append(f"CASE-COLLISION: {path} vs {other}")
                break
    return out


def _tree(repo: Path, sha: str) -> List[str]:
    raw = subprocess.run(["git", "-C", str(repo), "ls-tree", "-r", "--name-only", "-z", sha],
                         check=True, capture_output=True).stdout
    return [p for p in raw.decode("utf-8", errors="replace").split("\0") if p]


def collision_reasons(repo: Path, base: str, head: str, changes: List[Change]) -> List[str]:
    added = added_paths(changes)
    if not added:
        return []
    return case_collisions(added, _tree(repo, base) + _tree(repo, head))


def _show(repo: Path, sha: str, path: Path) -> Optional[str]:
    r = subprocess.run(["git", "-C", str(repo), "show", f"{sha}:{path.as_posix()}"],
                       capture_output=True)
    return r.stdout.decode("utf-8", errors="replace") if r.returncode == 0 else None


def _entries(text: Optional[str]) -> Counter:
    if text is None:
        return Counter()
    try:
        entries = json.loads(text)["entries"]
    except (ValueError, KeyError, TypeError) as e:
        raise ValueError(f"not a baseline ({e.__class__.__name__})")
    if not isinstance(entries, list) or not all(isinstance(e, str) for e in entries):
        raise ValueError("entries is not a list of strings")
    return Counter(entries)


def _growth_reasons(repo: Path, base: str, head: str, path: Path, rule: str) -> List[str]:
    try:
        after = _entries(_show(repo, head, path))
    except ValueError as e:
        return [f"{rule}: {path} at the head is unreadable: {e}"]
    try:
        before = _entries(_show(repo, base, path))
    except ValueError:
        before = Counter()
    added = sorted((after - before).elements())
    if not added:
        return []
    shown = "; ".join(a if len(a) <= 100 else a[:99] + "…" for a in added[:3])
    more = f" (+{len(added) - 3} more)" if len(added) > 3 else ""
    return [f"{rule}: {path} grew by {len(added)} entries, it can only shrink: {shown}{more}"]


def baseline_reasons(repo: Path, base: str, head: str) -> List[str]:
    return _growth_reasons(repo, base, head, lint.BASELINE, "GUARD-BASELINE")


def provider_gap_reasons(repo: Path, base: str, head: str) -> List[str]:
    reasons: List[str] = []
    for path in PROVIDER_FILES:
        reasons += _growth_reasons(repo, base, head, Path(path), "GUARD-PROVIDER-GAPS")
    return reasons


def approved_head(reviews: List[Dict], head_sha: str, maintainers: frozenset) -> bool:
    latest: Dict[str, Dict] = {}
    for r in sorted(reviews, key=lambda r: r.get("submitted_at") or ""):
        login = (r.get("user") or {}).get("login")
        if login in maintainers and r.get("state") in ("APPROVED", "CHANGES_REQUESTED", "DISMISSED"):
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
    changes = diff(Path(repo), base, head)
    reasons = needs_maintainer(changes, zones) + collision_reasons(Path(repo), base, head, changes)
    reasons += baseline_reasons(Path(repo), base, head)
    reasons += provider_gap_reasons(Path(repo), base, head)
    decision = decide(reasons, reviews, head_sha)
    for line in decision.details:
        print(line)
    print(("PASS " if decision.passed else "FAIL ") + decision.summary)
    if post:
        post_status(head_sha, decision, target_url)
    return 0 if decision.passed else 1
