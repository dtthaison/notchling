from __future__ import annotations

import fnmatch
import subprocess
from pathlib import Path
from typing import List

RULE = "GUARD-CLAUDE-MD"
ROOT_FILE = "CLAUDE.md"
ROOT_CONTENT = b"@AGENTS.md\n"


def _tracked(repo: Path) -> List[str]:
    out = subprocess.run(["git", "-C", str(repo), "ls-files", "-z"], check=True, capture_output=True).stdout
    return [p for p in out.decode("utf-8", errors="replace").split("\0") if p]


def check(repo: Path) -> List[str]:
    repo = Path(repo)
    problems: List[str] = []
    tracked = _tracked(repo)
    for path in tracked:
        name = path.rsplit("/", 1)[-1]
        if fnmatch.fnmatchcase(name.casefold(), "claude*.md") and path != ROOT_FILE:
            problems.append(f"{RULE}: {path} is a second instruction source; keep instructions in AGENTS.md")
    if ROOT_FILE not in tracked:
        problems.append(f"{RULE}: {ROOT_FILE} is missing; it must hold exactly '@AGENTS.md' and a newline")
        return problems
    root = repo / ROOT_FILE
    if root.is_symlink():
        problems.append(f"{RULE}: {ROOT_FILE} must be a plain file, not a link")
    elif root.read_bytes() != ROOT_CONTENT:
        problems.append(f"{RULE}: {ROOT_FILE} must hold exactly '@AGENTS.md' and a newline")
    return problems


def main(repo: Path) -> int:
    problems = check(repo)
    if problems:
        print("\n".join(problems))
        return 1
    print("CLAUDE.md is @AGENTS.md and no other CLAUDE*.md is tracked")
    return 0
