from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path
from typing import Dict, List, Optional, Set

RULE = "GUARD-AGENT-PATH"
GOLDEN = Path("crates/notchlingd/tests/golden/agent-paths.json")
WIRE = Path("crates/local-api/src/wire.rs")
COMMANDS = Path("crates/notchlingd/src/commands.rs")
SPECS = Path("docs/superpowers/specs")
FIRST_DATE = "2026-11-21"
GROWING = ("planned", "userOnly", "internal")
LISTS = ("planned", "userOnly", "internal")
FIELDS = {
    "planned": ("feature", "phase"),
    "userOnly": ("feature", "why"),
    "internal": ("why",),
}
INTERNAL_WHYS = ("app_ui", "hook", "node", "protocol")
PHASES = range(2, 10)
CONSENTS = ("none", "request", "by_key_class", "own_free", "by_entry", "never")
TIERS = ("agent", "app", "app_quiet", "approval")
DATED = re.compile(r"^(\d{4}-\d{2}-\d{2})-.+\.md$")
CODE = re.compile(r"`([^`\n]+)`")
FENCED = re.compile(r"^[ \t]*```[^\n]*\n(.*?)^[ \t]*```[ \t]*$", re.M | re.S)
COMMAND = re.compile(r"\bnotchling [a-z][a-z-]*")
OP = re.compile(r"(?<![\w.])[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)+(?!\w)")
NO_PATH = re.compile(r"^[ \t]*(?:[-*][ \t]+)?No agent path:[ \t]*(\w+)", re.M)
CONST = re.compile(r"pub const\s+[A-Z][A-Z0-9_]*\s*:\s*&(?:'static\s+)?str\s*=\s*\"([^\"]*)\"\s*;")
OPS_MOD = re.compile(r"pub(?:\([^)]*\))?\s+mod\s+ops\s*\{")
RAW_STRING = re.compile(r'b?r(#*)"')
CHAR_LITERAL = re.compile(r"'(?:\\[^'\n]*|[^\\'\n])'")
ROW = re.compile(r'\brow\(\s*"([^"]+)"')


def _code(text: str) -> str:
    out: List[str] = []
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        raw = RAW_STRING.match(text, i) if c in "rb" and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")) else None
        if text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
        elif text.startswith("/*", i):
            depth, i = 1, i + 2
            while i < n and depth:
                if text.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif text.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
            out.append(" ")
        elif c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            out.append(text[i:j + 1])
            i = j + 1
        elif raw:
            close = '"' + raw.group(1)
            j = text.find(close, raw.end())
            j = n if j < 0 else j
            body = text[raw.end():j].replace("\\", "\\\\").replace('"', '\\"')
            out.append('"' + body + '"')
            i = min(n, j + len(close))
        elif c == "'" and CHAR_LITERAL.match(text, i):
            i = CHAR_LITERAL.match(text, i).end()
            out.append("''")
        else:
            out.append(c)
            i += 1
    return "".join(out)


def _module_body(text: str, header: "re.Pattern[str]") -> Optional[str]:
    found = header.search(text)
    if not found:
        return None
    depth, in_string, i = 1, False, found.end()
    while i < len(text):
        c = text[i]
        if in_string:
            if c == "\\":
                i += 1
            elif c == '"':
                in_string = False
        elif c == '"':
            in_string = True
        elif c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return text[found.end():i]
        i += 1
    return None


def wire_ops(text: str) -> List[str]:
    body = _module_body(_code(text), OPS_MOD)
    return CONST.findall(body) if body is not None else []


def command_names(text: str) -> List[str]:
    return ROW.findall(_code(text))


def _is_object(value) -> bool:
    return isinstance(value, dict)


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
    for name in ("commands",) + LISTS:
        if not all(_is_object(e) for e in data[name]):
            raise ValueError(f"has an entry in {name} that is not an object")
    for row in data["commands"]:
        if not isinstance(row.get("ops", []), list) or not all(_is_object(o) for o in row.get("ops", [])):
            raise ValueError(f"has a command {row.get('command')!r} whose ops are not a list of objects")
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


def _text(value) -> bool:
    return isinstance(value, str) and value.strip() != ""


def _shape_problems(data: Dict) -> List[str]:
    problems = []
    for name, fields in FIELDS.items():
        for entry in data[name]:
            if not _text(entry.get("op")):
                problems.append(f"{RULE}: an entry in {name} has no op")
            for f in fields:
                if entry.get(f) in (None, ""):
                    problems.append(f"{RULE}: {entry.get('op')} in {name} has no {f}")
            if name == "internal" and entry.get("why") not in INTERNAL_WHYS:
                problems.append(f"{RULE}: {entry.get('op')} in internal has the reason {entry.get('why')!r}, "
                                f"one of {', '.join(INTERNAL_WHYS)}")
            if name == "planned" and entry.get("phase") is not None:
                phase = entry.get("phase")
                if isinstance(phase, bool) or not isinstance(phase, int) or phase not in PHASES:
                    problems.append(f"{RULE}: {entry.get('op')} in planned has the phase {phase!r}, "
                                    f"a whole number from {PHASES.start} to {PHASES.stop - 1}")
    for row in data["commands"]:
        label = row.get("command")
        if not _text(label):
            problems.append(f"{RULE}: a command has no name")
        if not (_text(row.get("usage")) and row["usage"].startswith("notchling ")):
            problems.append(f"{RULE}: command {label} has no usage that starts with 'notchling '")
        if row.get("consent") not in CONSENTS:
            problems.append(f"{RULE}: command {label} has the consent {row.get('consent')!r}, "
                            f"one of {', '.join(CONSENTS)}")
        exits = row.get("exits")
        if not (isinstance(exits, list) and exits and all(isinstance(x, int) and not isinstance(x, bool)
                                                           for x in exits)):
            problems.append(f"{RULE}: command {label} has no list of exit codes")
        for op in row.get("ops", []):
            if not _text(op.get("op")):
                problems.append(f"{RULE}: command {label} has an op without a name")
            if op.get("tier") not in TIERS:
                problems.append(f"{RULE}: {op.get('op')} of command {label} has the tier {op.get('tier')!r}, "
                                f"one of {', '.join(TIERS)}")
            if not isinstance(op.get("acts"), bool):
                problems.append(f"{RULE}: {op.get('op')} of command {label} does not say whether it acts")
    return problems


def _command_problems(data: Dict, commands_text: str) -> List[str]:
    rows = command_names(commands_text)
    if not rows:
        return [f"{RULE}: {COMMANDS} has no rows, so no command can be checked"]
    problems = []
    named = {row.get("command") for row in data["commands"]}
    for name in sorted(str(n) for n in named - set(rows)):
        problems.append(f"{RULE}: command {name} of {GOLDEN} is not a row of {COMMANDS}")
    for name in sorted(set(rows) - named):
        problems.append(f"{RULE}: command `{name}` of {COMMANDS} is not in {GOLDEN}")
    return problems


def check_ops(wire_text: str, golden_text: Optional[str], commands_text: Optional[str] = None) -> List[str]:
    try:
        data = parse(golden_text)
    except ValueError as e:
        return [f"{RULE}: {GOLDEN} {e}"]
    where = placements(data)
    problems = []
    ops = wire_ops(wire_text)
    if not ops:
        problems.append(f"{RULE}: {WIRE} has no `pub mod ops` with constants, so no op can be checked")
    for op in ops:
        places = where.get(op, [])
        if not places:
            problems.append(f"{RULE}: {op} is in no command, planned, userOnly or internal list of {GOLDEN}")
    for op, places in sorted(where.items(), key=lambda item: str(item[0])):
        if len(places) > 1:
            problems.append(f"{RULE}: {op} is in {len(places)} places of {GOLDEN}: {', '.join(places)}")
    if commands_text is not None:
        problems += _command_problems(data, commands_text)
    return problems + _shape_problems(data)


def _names_command_and_op(section: str, known: Set[str]) -> bool:
    code = "\n".join(CODE.findall(section) + FENCED.findall(section))
    return bool(COMMAND.search(code)) and any(op in known for op in OP.findall(section))


def check_spec(rel: str, text: str, known: Set[str]) -> List[str]:
    where = SPECS / rel
    m = DATED.match(Path(rel).name)
    if not m:
        return [f"{RULE}: {where} has no date in its name; call it YYYY-MM-DD-name.md"]
    if m.group(1) < FIRST_DATE:
        return []
    parts = re.split(r"^## Agent path[ \t]*$", text, maxsplit=1, flags=re.M)
    if len(parts) < 2:
        return [f"{RULE}: {where} has no '## Agent path' section"]
    section = re.split(r"^## ", parts[1], maxsplit=1, flags=re.M)[0]
    no_path = NO_PATH.search(section)
    if no_path:
        if no_path.group(1) in INTERNAL_WHYS:
            return []
        return [f"{RULE}: {where} says 'No agent path:' without a reason from "
                f"{', '.join(INTERNAL_WHYS)}"]
    if _names_command_and_op(section, known):
        return []
    return [f"{RULE}: {where} '## Agent path' does not name a `notchling` command in code together with an op "
            f"of {WIRE} or a planned op of {GOLDEN}, and does not start a line with 'No agent path: <reason>'"]


def _known_ops(wire_text: str, golden_text: Optional[str]) -> Set[str]:
    known = set(wire_ops(wire_text))
    try:
        data = parse(golden_text)
    except ValueError:
        return known
    return known | {e.get("op") for e in data["planned"] if _text(e.get("op"))}


def check(repo: Path) -> List[str]:
    repo = Path(repo)
    golden = repo / GOLDEN
    wire = repo / WIRE
    commands = repo / COMMANDS
    problems: List[str] = []
    known: Set[str] = set()
    golden_text = golden.read_text(encoding="utf-8") if golden.is_file() else None
    if not wire.is_file():
        problems.append(f"{RULE}: {WIRE} is missing")
    elif not commands.is_file():
        problems.append(f"{RULE}: {COMMANDS} is missing")
    else:
        wire_text = wire.read_text(encoding="utf-8")
        problems += check_ops(wire_text, golden_text, commands.read_text(encoding="utf-8"))
        known = _known_ops(wire_text, golden_text)
    specs = repo / SPECS
    if specs.is_dir():
        for path in sorted(specs.rglob("*.md")):
            problems += check_spec(path.relative_to(specs).as_posix(), path.read_text(encoding="utf-8"), known)
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
        added = sorted(str(op) for op in _ops_of(after, name) - old)
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
