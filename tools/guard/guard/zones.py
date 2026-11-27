from __future__ import annotations

import json
import re
import unicodedata
from dataclasses import dataclass
from pathlib import Path
from typing import List, Pattern, Tuple

from guard.changes import Change


def fold(text: str) -> str:
    return unicodedata.normalize("NFC", unicodedata.normalize("NFC", text).casefold())


def glob_regex(glob: str) -> Pattern[str]:
    out, i = "", 0
    while i < len(glob):
        if glob.startswith("**/", i):
            out += "(?:.*/)?"
            i += 3
        elif glob.startswith("**", i):
            out += ".*"
            i += 2
        elif glob[i] == "*":
            out += "[^/]*"
            i += 1
        elif glob[i] == "?":
            out += "[^/]"
            i += 1
        else:
            out += re.escape(glob[i])
            i += 1
    return re.compile(out + r"\Z")


@dataclass(frozen=True)
class Zones:
    auto: Tuple[str, ...]
    new_files_only: Tuple[str, ...]

    def _match(self, globs: Tuple[str, ...], path: str) -> bool:
        inside = False
        for g in globs:
            negate = g.startswith("!")
            if glob_regex(g[1:] if negate else g).match(path):
                inside = not negate
        return inside

    def in_zone(self, path: str) -> bool:
        return self._match(self.auto, path)

    def new_file_ok(self, path: str) -> bool:
        return self._match(self.new_files_only, path)

    def casefolded(self) -> "Zones":
        return Zones(tuple(fold(g) for g in self.auto), tuple(fold(g) for g in self.new_files_only))


def load(path: Path) -> Zones:
    data = json.loads(Path(path).read_text())
    return Zones(tuple(data["auto"]), tuple(data.get("new_files_only", [])))


def protected_paths(changes: List[Change], zones: Zones) -> List[str]:
    out: List[str] = []
    for c in changes:
        ends = [c.path] + ([c.old_path] if c.old_path else [])
        for p in ends:
            ok = zones.in_zone(p) or (c.status == "A" and zones.new_file_ok(p))
            if not ok and p not in out:
                out.append(p)
    return out


def case_shadowed(changes: List[Change], zones: Zones) -> List[str]:
    real = set(protected_paths(changes, zones))
    folded = []
    for c in changes:
        folded.append(Change(fold(c.path), c.status, fold(c.old_path) if c.old_path else None))
    shadowed = set(protected_paths(folded, zones.casefolded()))
    out: List[str] = []
    for c in changes:
        for p in [c.path] + ([c.old_path] if c.old_path else []):
            if p not in real and fold(p) in shadowed and p not in out:
                out.append(p)
    return out
