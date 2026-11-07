"""What a pull request changed: one `Change` per touched file, from `git diff base...head`.

Only the PR's own changes count (three dots: from the merge base). Paths come from
`--name-status -z`, so spaces and odd characters survive; each file's lines come from its
own `--unified=0 --text --no-textconv` diff. Without `--text` git prints no lines for a file it
thinks is binary (a NUL byte, or a `-diff` line in a `.gitattributes` the PR or an earlier PR
added), and swiftc compiles such a file anyway. `--numstat` says which files git calls binary,
so the caller can still send those to the maintainer.
"""

from __future__ import annotations

import os
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional, Tuple

HUNK = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@")

# Files that are binary on purpose: their bytes aren't read as lines. Any other file the diff
# calls binary is still read as text, since a code file with a NUL byte in a comment compiles.
ASSET_EXTS = frozenset({".png", ".jpg", ".jpeg", ".gif", ".webp", ".heic", ".ico", ".icns", ".pdf", ".mp3",
                        ".mp4", ".mov", ".wav", ".m4a", ".zip", ".gz", ".tar", ".ttf", ".otf", ".woff", ".woff2"})


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


def _lines(patch: str) -> Tuple[Tuple[Tuple[int, str], ...], Tuple[str, ...]]:
    added, removed, in_hunk, line_no = [], [], False, 0
    for raw in patch.split("\n"):
        m = HUNK.match(raw)
        if m:
            in_hunk, line_no = True, int(m.group(1))
            continue
        if not in_hunk:
            continue
        if raw.startswith("+"):
            added.append((line_no, raw[1:]))
            line_no += 1
        elif raw.startswith("-"):
            removed.append(raw[1:])
        elif raw.startswith(" "):
            line_no += 1
    return tuple(added), tuple(removed)


def _binary(repo: Path, base: str, head: str, paths: List[str]) -> bool:
    """Does git call this file binary? `--numstat` prints `-` for both counts then."""
    stat = _git(repo, "diff", "-M", "--numstat", "--no-color", "--no-ext-diff", "--no-textconv",
                f"{base}...{head}", "--", *paths)
    return stat.startswith("-\t-\t")


def diff(repo: Path, base: str, head: str) -> List[Change]:
    changes = []
    for status, old, path in _entries(repo, base, head):
        paths = [p for p in (old, path) if p]
        binary = _binary(repo, base, head, paths)
        if binary and os.path.splitext(path)[1].lower() in ASSET_EXTS:
            changes.append(Change(path, status, old, (), (), True))
            continue
        patch = _git(repo, "diff", "-M", "--unified=0", "--no-color", "--no-ext-diff", "--text",
                     "--no-textconv", f"{base}...{head}", "--", *paths)
        added, removed = _lines(patch)
        changes.append(Change(path, status, old, added, removed, binary))
    return changes
