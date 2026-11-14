from __future__ import annotations

import json
import re
from pathlib import Path
from typing import List, Tuple

from guard.source import strip_comments
from guard.zones import glob_regex

AREAS = "tools/locale/areas.json"
AREAS_MOVED = json.dumps({"areas": [{"name": "notch", "paths": ["crates/app/src/notch/**"], "moved": True}]})

Found = List[Tuple[int, str]]

EXEMPT = (".part(", ".part_at(", ".part_inside(", ".window_region(", ".id(", "ElementId", "eprintln!", "println!",
          "debugPrint(", "log::", "tracing::", "env::var", "_ENV", "#[", "import ", "Key(")
RUST_STRING = re.compile(r'"((?:[^"\\]|\\.)*)"')
DART_STRING = re.compile(r'"((?:[^"\\]|\\.)*)"|\'((?:[^\'\\]|\\.)*)\'')
WORDS = re.compile(r"[A-Za-z]{2,}")
CAPITAL = re.compile(r"^[A-Z].*[a-z]")
CONSENT_START = ("Always Allow", "Allow", "Deny", "Approve", "Pair", "Trust", "Install", "Sign In")
MESSAGE = re.compile(r"^[a-z][a-z0-9-]*\s*=\s*(.*)$")


def moved_paths(repo: Path) -> List[str]:
    try:
        data = json.loads((Path(repo) / AREAS).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return []
    out: List[str] = []
    for area in data.get("areas", []):
        if area.get("moved") is True:
            out.extend(str(p) for p in area.get("paths", []))
    return out


def is_literal(text: str) -> bool:
    if " " in text and WORDS.search(text):
        return True
    return bool(CAPITAL.match(text))


def _strings(path: str, line: str) -> List[str]:
    pattern = DART_STRING if path.endswith(".dart") else RUST_STRING
    return ["".join(g for g in m.groups() if g) for m in pattern.finditer(line)]


def literal(path: str, lines: List[str], repo: Path) -> Found:
    if not any(glob_regex(g).match(path) for g in moved_paths(repo)):
        return []
    rust = path.endswith(".rs")
    out: Found = []
    for i, raw in enumerate(lines):
        if rust and raw.rstrip() == "#[cfg(test)]":
            break
        line = strip_comments(path, raw)
        if any(token in line for token in EXEMPT):
            continue
        if any(is_literal(s) for s in _strings(path, line)):
            out.append((i + 1, raw.strip()))
    return out


def consent(path: str, lines: List[str], repo: Path) -> Found:
    out: Found = []
    for i, raw in enumerate(lines):
        m = MESSAGE.match(raw)
        if not m:
            continue
        value = m.group(1).lstrip()
        if any(value == w or value.startswith(w + " ") for w in CONSENT_START):
            out.append((i + 1, raw.strip()))
    return out
