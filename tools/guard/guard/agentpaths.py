from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path
from typing import Dict, List, Optional, Set

RULE = "GUARD-AGENT-PATH"
GOLDEN = Path("crates/notchlingd/tests/golden/agent-paths.json")
WIRE = Path("crates/local-api/src/wire.rs")
SPECS = Path("docs/superpowers/specs")
FIRST_DATE = "2026-11-21"
GROWING = ("planned", "internal")
LISTS = ("planned", "userOnly", "internal")
FIELDS = {
    "planned": ("feature", "phase"),
    "userOnly": ("feature", "why"),
    "internal": ("why",),
}
INTERNAL_WHYS = ("app_ui", "hook", "node", "protocol")
DATED = re.compile(r"^(\d{4}-\d{2}-\d{2})-.*\.md$")
OP = re.compile(r"`[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)+`")
COMMAND = re.compile(r"`notchling [a-z]")
NO_PATH = re.compile(r"No agent path:\s*(\w+)")


def wire_ops(text: str) -> List[str]:
    start = text.find("pub mod ops {")
    if start < 0:
        return []
    end = text.find("\n}\n", start)
    body = text[start:end if end >= 0 else len(text)]
    names = []
    for line in body.splitlines():
        if line.lstrip().startswith("pub const") and line.count('"') >= 2:
            names.append(line.split('"')[1])
    return names


def parse(text: Optional[str]) -> Dict:
    if text is None:
        raise ValueError("is missing")
    try:
        data = json.loads(text)
    except ValueError as e:
        raise ValueError(f"is not JSON ({e.__class__.__name__})")
    if not isinstance(data, dict) or not isinstance(data.get("commands"), list):
        raise ValueError("has no commands list")
    for name in LISTS:
        if not isinstance(data.get(name), list):
            raise ValueError(f"has no {name} list")
    return data


def placements(data: Dict) -> Dict[str, List[str]]:
    where: Dict[str, List[str]] = {}
    for row in data["commands"]:
        for op in row.get("ops", []):
            where.setdefault(op.get("op"), []).append(f"command {row.get('command')}")
    for name in LISTS:
        for entry in data[name]:
            where.setdefault(entry.get("op"), []).append(name)
    return where


def _missing_fields(data: Dict) -> List[str]:
    problems = []
    for name, fields in FIELDS.items():
        for entry in data[name]:
            for f in fields:
                if entry.get(f) in (None, ""):
                    problems.append(f"{RULE}: {entry.get('op')} in {name} has no {f}")
            if name == "internal" and entry.get("why") not in INTERNAL_WHYS:
                problems.append(f"{RULE}: {entry.get('op')} in internal has the reason {entry.get('why')!r}, "
                                f"one of {', '.join(INTERNAL_WHYS)}")
    return problems


def check_ops(wire_text: str, golden_text: Optional[str]) -> List[str]:
    try:
        data = parse(golden_text)
    except ValueError as e:
        return [f"{RULE}: {GOLDEN} {e}"]
    where = placements(data)
    problems = []
    for op in wire_ops(wire_text):
        places = where.get(op, [])
        if not places:
            problems.append(f"{RULE}: {op} is in no command, planned, userOnly or internal list of {GOLDEN}")
    for op, places in sorted(where.items()):
        if len(places) > 1:
            problems.append(f"{RULE}: {op} is in {len(places)} places of {GOLDEN}: {', '.join(places)}")
    return problems + _missing_fields(data)


def check_spec(name: str, text: str) -> List[str]:
    m = DATED.match(name)
    if not m or m.group(1) < FIRST_DATE:
        return []
    parts = re.split(r"^## Agent path[ \t]*$", text, maxsplit=1, flags=re.M)
    if len(parts) < 2:
        return [f"{RULE}: {SPECS / name} has no '## Agent path' section"]
    section = re.split(r"^## ", parts[1], maxsplit=1, flags=re.M)[0]
    no_path = NO_PATH.search(section)
    if no_path:
        if no_path.group(1) in INTERNAL_WHYS:
            return []
        return [f"{RULE}: {SPECS / name} says 'No agent path:' without a reason from "
                f"{', '.join(INTERNAL_WHYS)}"]
    if COMMAND.search(section) and OP.search(section):
        return []
    return [f"{RULE}: {SPECS / name} '## Agent path' names no `notchling` command and op, "
            f"and does not say 'No agent path: <reason>'"]


def check(repo: Path) -> List[str]:
    repo = Path(repo)
    golden = repo / GOLDEN
    wire = repo / WIRE
    problems: List[str] = []
    if not wire.is_file():
        problems.append(f"{RULE}: {WIRE} is missing")
    else:
        problems += check_ops(wire.read_text(encoding="utf-8"),
                              golden.read_text(encoding="utf-8") if golden.is_file() else None)
    specs = repo / SPECS
    if specs.is_dir():
        for path in sorted(specs.glob("*.md")):
            problems += check_spec(path.name, path.read_text(encoding="utf-8"))
    return problems


def _show(repo: Path, sha: str) -> Optional[str]:
    r = subprocess.run(["git", "-C", str(repo), "show", f"{sha}:{GOLDEN.as_posix()}"], capture_output=True)
    return r.stdout.decode("utf-8", errors="replace") if r.returncode == 0 else None


def _ops_of(data: Dict, name: str) -> Set[str]:
    return {e.get("op") for e in data[name] if isinstance(e, dict)}


def growth_reasons(repo: Path, base: str, head: str) -> List[str]:
    try:
        after = parse(_show(repo, head))
    except ValueError as e:
        if _show(repo, head) is None and _show(repo, base) is None:
            return []
        return [f"{RULE}: {GOLDEN} at the head {e}"]
    try:
        before: Optional[Dict] = parse(_show(repo, base))
    except ValueError:
        before = None
    reasons = []
    for name in GROWING:
        old = _ops_of(before, name) if before else set()
        added = sorted(_ops_of(after, name) - old)
        if added:
            shown = ", ".join(added[:3]) + (f" (+{len(added) - 3} more)" if len(added) > 3 else "")
            reasons.append(f"{RULE}: {GOLDEN} {name} grew by {len(added)} ops, it can only shrink: {shown}")
    return reasons


def main(repo: Path) -> int:
    problems = check(repo)
    if problems:
        print("\n".join(problems))
        return 1
    print("every op has an agent path or a reason, and every new design has its Agent path")
    return 0
