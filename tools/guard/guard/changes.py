"""What a pull request changed: one `Change` per touched file, from `git diff base...head`.

Only the PR's own changes count (three dots: from the merge base). Paths come from
`--name-status -z`, so spaces and odd characters survive; each file's lines come from its
own `--unified=0` diff.
"""

from __future__ import annotations

import re
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional, Tuple

HUNK = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@")


@dataclass(frozen=True)
class Change:
    path: str
    status: str  # A, M, D or R
    old_path: Optional[str] = None
    added: Tuple[Tuple[int, str], ...] = ()
    removed: Tuple[str, ...] = ()
    binary: bool = False


def _git(repo: Path, *args: str) -> str:
    out = subprocess.run(["git", "-C", str(repo), "-c", "core.quotePath=false", *args],
                         check=True, capture_output=True)
    return out.stdout.decode("utf-8", errors="replace")


def _entries(repo: Path, base: str, head: str) -> List[Tuple[str, Optional[str], str]]:
    """(status, old path, path) for each touched file."""
    fields = _git(repo, "diff", "-M", "--name-status", "-z", f"{base}...{head}").split("\0")
    out, i = [], 0
    while i < len(fields) and fields[i]:
        status = fields[i][0]
        if status in "RC":
            out.append(("R" if status == "R" else "A", fields[i + 1], fields[i + 2]))
            i += 3
        else:
            out.append(("M" if status == "T" else status, None, fields[i + 1]))
            i += 2
    return out


def _lines(patch: str) -> Tuple[Tuple[Tuple[int, str], ...], Tuple[str, ...], bool]:
    added, removed, binary, in_hunk, line_no = [], [], False, False, 0
    for raw in patch.split("\n"):
        m = HUNK.match(raw)
        if m:
            in_hunk, line_no = True, int(m.group(1))
            continue
        if not in_hunk:
            if raw.startswith("Binary files ") or raw.startswith("GIT binary patch"):
                binary = True
            continue
        if raw.startswith("+"):
            added.append((line_no, raw[1:]))
            line_no += 1
        elif raw.startswith("-"):
            removed.append(raw[1:])
        elif raw.startswith(" "):
            line_no += 1
    return tuple(added), tuple(removed), binary


def diff(repo: Path, base: str, head: str) -> List[Change]:
    changes = []
    for status, old, path in _entries(repo, base, head):
        paths = [p for p in (old, path) if p]
        patch = _git(repo, "diff", "-M", "--unified=0", "--no-color", "--no-ext-diff",
                     f"{base}...{head}", "--", *paths)
        added, removed, binary = _lines(patch)
        changes.append(Change(path, status, old, added, removed, binary))
    return changes
