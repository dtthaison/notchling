from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path
from typing import Callable, List, Optional, Set

USES = re.compile(r"""^(?P<lead>\s*(?:-\s+)?uses:\s*)(?P<q>['"]?)(?P<ref>[^'"\s#]+)(?P=q)(?P<rest>.*)$""")
PINNED = re.compile(r"^(?P<repo>[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)(?:/[^@]*)?@(?P<sha>[0-9a-f]{40})$")
TAGGED = re.compile(r"^(?P<repo>[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)(?P<sub>/[^@]*)?@(?P<tag>[^@]+)$")
ALLOWLIST = Path("tools/guard/actions-allowlist.json")


def _workflows(workflows_dir: Path) -> List[Path]:
    d = Path(workflows_dir)
    return sorted(list(d.glob("*.yml")) + list(d.glob("*.yaml")))


def load_allowlist(path: Path) -> Set[str]:
    return set(json.loads(Path(path).read_text())["allow"])


def check(workflows_dir: Path, allowlist: Set[str]) -> List[str]:
    problems: List[str] = []
    for f in _workflows(workflows_dir):
        for n, line in enumerate(f.read_text().splitlines(), 1):
            m = USES.match(line)
            if not m:
                continue
            ref = m.group("ref")
            if ref.startswith("./"):
                continue
            where = f"{f.name}:{n}"
            p = PINNED.match(ref)
            if not p:
                problems.append(f"GUARD-PINS: {where} {ref} is not pinned to a 40-character commit SHA")
            elif p.group("repo") not in allowlist:
                problems.append(f"GUARD-PINS: {where} {p.group('repo')} is not in {ALLOWLIST}")
    return problems


def peeled_sha(ls_remote: str) -> Optional[str]:
    plain = None
    for line in ls_remote.splitlines():
        sha, _, ref = line.partition("\t")
        if ref.endswith("^{}"):
            return sha
        plain = plain or sha
    return plain


def resolve_tag(repo: str, tag: str) -> str:
    out = subprocess.run(["git", "ls-remote", f"https://github.com/{repo}", f"refs/tags/{tag}",
                          f"refs/tags/{tag}^{{}}"], check=True, capture_output=True, text=True).stdout
    sha = peeled_sha(out)
    if not sha:
        raise SystemExit(f"GUARD-PINS: no tag {tag} in {repo}")
    return sha


def pin(workflows_dir: Path, resolve: Callable[[str, str], str] = resolve_tag) -> List[str]:
    changed: List[str] = []
    for f in _workflows(workflows_dir):
        lines = f.read_text().splitlines(keepends=True)
        for i, line in enumerate(lines):
            m = USES.match(line.rstrip("\n"))
            if not m or m.group("ref").startswith("./") or PINNED.match(m.group("ref")):
                continue
            t = TAGGED.match(m.group("ref"))
            if not t:
                continue
            sha = resolve(t.group("repo"), t.group("tag"))
            new = f"{m.group('lead')}{t.group('repo')}{t.group('sub') or ''}@{sha} # {t.group('tag')}"
            lines[i] = new + ("\n" if line.endswith("\n") else "")
            changed.append(f"{f.name}:{i + 1} {m.group('ref')} -> {sha}")
        f.write_text("".join(lines))
    return changed
