from __future__ import annotations

import re
import subprocess
from pathlib import Path
from typing import List, Set

RULE = "GUARD-FEATURE-MAP"
MAP = "docs/feature-map.md"

FULL = re.compile(r"crates/([\w-]+)/src/([\w-]+)/")
CRATE = re.compile(r"crates/([\w-]+)/")
SHORT = re.compile(r"(?<![\w/])src/([\w-]+)/")


def _tracked(repo: Path) -> List[str]:
    out = subprocess.run(["git", "-C", str(repo), "ls-files", "-z", "--", "crates"],
                         check=True, capture_output=True).stdout
    return [p for p in out.decode("utf-8", errors="replace").split("\0") if p]


def folders(tracked: List[str]) -> List[str]:
    found: Set[str] = set()
    for path in tracked:
        parts = path.split("/")
        if len(parts) >= 5 and parts[0] == "crates" and parts[2] == "src":
            found.add(f"crates/{parts[1]}/src/{parts[3]}/")
    return sorted(found)


def named(text: str) -> Set[str]:
    out = {f"crates/{c}/src/{d}/" for c, d in FULL.findall(text)}
    for line in text.splitlines():
        if not line.lstrip().startswith("|"):
            continue
        crates = set(CRATE.findall(line))
        for d in SHORT.findall(line):
            out.update(f"crates/{c}/src/{d}/" for c in crates)
    return out


def check(repo: Path) -> List[str]:
    repo = Path(repo)
    path = repo / MAP
    if not path.is_file():
        return [f"{RULE}: {MAP} is missing"]
    have = named(path.read_text(encoding="utf-8"))
    return [f"{RULE}: {f} is not in {MAP}; add it to its crate's row, or a row of its own"
            for f in folders(_tracked(repo)) if f not in have]


def main(repo: Path) -> int:
    problems = check(repo)
    if problems:
        print("\n".join(problems))
        return 1
    print("every crates/*/src/ folder is in the feature map")
    return 0
