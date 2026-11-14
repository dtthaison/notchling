from __future__ import annotations

import contextlib
import io
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Callable, Dict, Iterator, List, Optional, Tuple

from guard import capability, claudemd, featuremap, gate, lint, pins, rulesets, weakening
from guard.changes import Change, diff
from guard.zones import Zones

RULE = "GUARD-RULES"
DOCS = Path("docs/rules.md")
ID = re.compile(r"^[A-Z][A-Z0-9]*(?:-[A-Z0-9]+)*$")
PACKAGE_ROOT = Path(__file__).resolve().parents[1]

GIT_ENV = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_SYSTEM": os.devnull}
GIT = ["git", "-c", "user.name=x", "-c", "user.email=x@x", "-c", "commit.gpgsign=false", "-c", "core.ignorecase=false"]


@contextlib.contextmanager
def _tmp() -> Iterator[Path]:
    with tempfile.TemporaryDirectory(prefix="guard-rules-") as d:
        yield Path(d)


def _git(repo: Path, *args: str, stdin: Optional[str] = None) -> str:
    out = subprocess.run(GIT + ["-C", str(repo), *args], check=True, capture_output=True, text=True,
                         env=GIT_ENV, input=stdin)
    return out.stdout.strip()


def _repo(root: Path, files: Dict[str, str]) -> str:
    _git(root, "init", "-q")
    write(root, files)
    _git(root, "add", "-A")
    _git(root, "commit", "-q", "--allow-empty", "-m", "base")
    return _git(root, "rev-parse", "HEAD")


def write(root: Path, files: Dict[str, str]) -> None:
    for name, text in files.items():
        p = root / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding="utf-8")


def _lint_probe(rule_id: str) -> Callable[[], bool]:
    def probe() -> bool:
        path, text = lint.rule(rule_id).bad
        with _tmp() as root:
            write(root, {path: text})
            return any(v.rule == rule_id for v in lint.run(root, [path]))
    return probe


ZONE_FILE = "legacy/macos/Sources/Notchling/Search/X.swift"
CAP_BAD: Dict[str, Tuple[str, str]] = {
    "CAP-PROCESS": (ZONE_FILE, "let p = Process()"),
    "CAP-PTY": (ZONE_FILE, "let pid = forkpty(&master, nil, nil, nil)"),
    "CAP-APPLESCRIPT": (ZONE_FILE, "let s = NSAppleScript(source: src)"),
    "CAP-DYNAMIC": (ZONE_FILE, 'let f = dlsym(nil, "SecItemCopyMatching")'),
    "CAP-NETWORK": (ZONE_FILE, "URLSession.shared.dataTask(with: url)"),
    "CAP-KEYCHAIN": (ZONE_FILE, "SecItemCopyMatching(query, &out)"),
    "CAP-FILEWRITE": (ZONE_FILE, "try data.write(to: url)"),
    "CAP-PROTECTED-TYPE": ("legacy/macos/Sources/Notchling/Chat/Views/Row.swift", "ReplySender.send(text, to: agent)"),
    "CAP-OBSCURE": (ZONE_FILE, "a /* b */ " * (capability.MAX_COMMENT_ENDS + 1)),
}


def _cap_probe(rule_id: str) -> Callable[[], bool]:
    def probe() -> bool:
        path, line = CAP_BAD[rule_id]
        return any(t.rule == rule_id for t in capability.scan([Change(path, "M", None, ((1, line),))]))
    return probe


def _cap_binary() -> bool:
    trips = capability.scan([Change(ZONE_FILE, "M", None, (), (), True)])
    return any(t.rule == capability.BINARY_RULE for t in trips)


def _weak(rule_id: str, change: Change) -> Callable[[], bool]:
    return lambda: any(t.rule == rule_id for t in weakening.scan([change]))


def _gate_zone() -> bool:
    with _tmp() as root:
        base = _repo(root, {"build.sh": "echo 1\n"})
        write(root, {"build.sh": "echo 2\n"})
        _git(root, "commit", "-q", "-am", "change")
        changes = diff(root, base, _git(root, "rev-parse", "HEAD"))
        reasons = gate.needs_maintainer(changes, Zones(("docs/**",), ()))
    return any(r.startswith("ZONE:") for r in reasons)


def _gate_case_collision() -> bool:
    with _tmp() as root:
        base = _repo(root, {"docs/Notes.md": "one\n"})
        blob = _git(root, "hash-object", "-w", "--stdin", stdin="two\n")
        _git(root, "update-index", "--add", "--cacheinfo", f"100644,{blob},docs/notes.md")
        _git(root, "commit", "-q", "-m", "collide")
        head = _git(root, "rev-parse", "HEAD")
        changes = diff(root, base, head)
        reasons = gate.collision_reasons(root, base, head, changes)
    return any(r.startswith("CASE-COLLISION:") for r in reasons)


def _guard_codeowners() -> bool:
    with _tmp() as root:
        write(root, {"tools/guard/zones.json": json.dumps({"auto": ["docs/**"]})})
        env = dict(GIT_ENV, PYTHONPATH=str(PACKAGE_ROOT))
        r = subprocess.run([sys.executable, "-m", "guard", "codeowners", "--check"], cwd=str(root), env=env,
                           capture_output=True, text=True, timeout=60)
    return r.returncode == 1 and "GUARD-CODEOWNERS" in r.stdout


def _guard_pins() -> bool:
    with _tmp() as root:
        write(root, {"x.yml": "jobs:\n  a:\n    steps:\n      - uses: actions/checkout@v4\n"})
        problems = pins.check(root, {"actions/checkout"})
    return any(p.startswith("GUARD-PINS") for p in problems)


def _guard_rulesets() -> bool:
    with _tmp() as root:
        write(root, {"rulesets/main.json": json.dumps({"target": "branch", "enforcement": "disabled"})})
        problems = rulesets.validate(root / "rulesets", root / "workflows")
    return any(p.startswith("GUARD-RULESETS") for p in problems)


def _guard_baseline() -> bool:
    with _tmp() as root:
        base = _repo(root, {str(lint.BASELINE): json.dumps({"entries": ["A|a.swift|x"]})})
        write(root, {str(lint.BASELINE): json.dumps({"entries": ["A|a.swift|x", "B|b.swift|y"]})})
        _git(root, "commit", "-q", "-am", "grow")
        grew = gate.baseline_reasons(root, base, _git(root, "rev-parse", "HEAD"))
    with _tmp() as root:
        _repo(root, {str(lint.BASELINE): json.dumps({"entries": []})})
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = lint.main(root, write_new=True)
    return (any(r.startswith("GUARD-BASELINE:") for r in grew)
            and code == 1 and "GUARD-BASELINE" in out.getvalue())


def _guard_provider_gaps() -> bool:
    gaps, unwired = gate.PROVIDER_FILES
    with _tmp() as root:
        base = _repo(root, {gaps: json.dumps({"entries": ["codex subagent.list"]}),
                            unwired: json.dumps({"entries": []})})
        write(root, {gaps: json.dumps({"entries": ["codex subagent.list", "codex subagent.open"]})})
        _git(root, "commit", "-q", "-am", "grow")
        grew = gate.provider_gap_reasons(root, base, _git(root, "rev-parse", "HEAD"))
    return any(r.startswith("GUARD-PROVIDER-GAPS:") for r in grew)


def _guard_claude_md() -> bool:
    with _tmp() as root:
        _repo(root, {"CLAUDE.md": "@AGENTS.md\n", "legacy/macos/Sources/CLAUDE.md": "a second source\n"})
        problems = claudemd.check(root)
    return any(p.startswith("GUARD-CLAUDE-MD") for p in problems)


def _guard_feature_map() -> bool:
    with _tmp() as root:
        _repo(root, {"docs/feature-map.md": "| Notch | `crates/app/src/notch/` | t | auto | U2 |\n",
                     "crates/app/src/notch/mod.rs": "", "crates/app/src/phone/mod.rs": ""})
        problems = featuremap.check(root)
    return (len(problems) == 1 and problems[0].startswith("GUARD-FEATURE-MAP")
            and "crates/app/src/phone/" in problems[0])


def _guard_rules() -> bool:
    with _tmp() as root:
        write(root, {"rules.md": "| ID | Rule | Why | Check |\n| --- | --- | --- | --- |\n"
                                 "| NOT-A-RULE | r | w | c |\n"})
        problems = check(root / "rules.md", {}, run_probes=False)
    return any(p.startswith(RULE) and "NOT-A-RULE" in p for p in problems)


def _guard_protocol_gen() -> bool:
    repo = PACKAGE_ROOT.parents[1]
    gen = repo / "tools/protocol/gen.py"
    schema = json.loads((repo / "protocol/agentconsole.v1.schema.json").read_text(encoding="utf-8"))
    with _tmp() as root:
        copy = root / "schema.json"
        args = ["--schema", str(copy), "--rust", str(root / "generated.rs"), "--dart", str(root / "generated.dart")]

        def run(*extra: str) -> "subprocess.CompletedProcess[str]":
            return subprocess.run([sys.executable, str(gen), *args, *extra], capture_output=True, text=True, timeout=120)

        copy.write_text(json.dumps(schema), encoding="utf-8")
        if run().returncode != 0 or run("--check").returncode != 0:
            return False
        schema["$defs"]["Session"]["properties"]["guardProbe"] = {"type": "string", "description": "added by the probe"}
        copy.write_text(json.dumps(schema), encoding="utf-8")
        stale = run("--check")
    return stale.returncode == 1 and "GUARD-PROTOCOL-GEN" in stale.stdout + stale.stderr


Registry = Dict[str, Callable[[], bool]]
REGISTRY: Registry = {}
for _r in lint.RULES:
    REGISTRY[_r.id] = _lint_probe(_r.id)
for _id in capability.RULES:
    REGISTRY[_id] = _cap_probe(_id)
REGISTRY[capability.BINARY_RULE] = _cap_binary
REGISTRY[capability.OBSCURE_RULE] = _cap_probe(capability.OBSCURE_RULE)
REGISTRY.update({
    "WEAK-TEST-EDIT": _weak("WEAK-TEST-EDIT", Change("legacy/macos/Tests/NotchlingTests/AThingTests.swift", "M")),
    "WEAK-SKIP": _weak("WEAK-SKIP", Change("legacy/macos/Tests/NotchlingTests/AThingTests.swift", "A",
                                           None, ((1, "try XCTSkip(\"later\")"),))),
    "WEAK-UNICODE": _weak("WEAK-UNICODE", Change("legacy/macos/Sources/Notchling/A.swift", "M",
                                                 None, ((1, "let a = 1 \u202e // reads differently\n"),))),
    "ZONE": _gate_zone,
    "CASE-COLLISION": _gate_case_collision,
    "GUARD-CODEOWNERS": _guard_codeowners,
    "GUARD-PINS": _guard_pins,
    "GUARD-RULESETS": _guard_rulesets,
    "GUARD-BASELINE": _guard_baseline,
    "GUARD-PROVIDER-GAPS": _guard_provider_gaps,
    "GUARD-CLAUDE-MD": _guard_claude_md,
    "GUARD-FEATURE-MAP": _guard_feature_map,
    "GUARD-RULES": _guard_rules,
    "GUARD-PROTOCOL-GEN": _guard_protocol_gen,
})
del _r, _id


def listed_ids(text: str) -> List[str]:
    ids = []
    for line in text.splitlines():
        if not line.lstrip().startswith("|"):
            continue
        cell = line.strip().strip("|").split("|")[0].strip()
        if cell != "ID" and ID.match(cell):
            ids.append(cell)
    return ids


def check(docs: Path, registry: Optional[Registry] = None, run_probes: bool = True) -> List[str]:
    registry = REGISTRY if registry is None else registry
    try:
        text = Path(docs).read_text(encoding="utf-8")
    except OSError as e:
        return [f"{RULE}: {docs} cannot be read: {e}"]
    ids = listed_ids(text)
    problems: List[str] = []
    for rule_id in dict.fromkeys(ids):
        if ids.count(rule_id) > 1:
            problems.append(f"{RULE}: {rule_id} is listed twice in {docs}")
    for rule_id in dict.fromkeys(ids):
        if rule_id not in registry:
            problems.append(f"{RULE}: {rule_id} is listed in {docs} but has no check in guard/rules.py")
    for rule_id in registry:
        if rule_id not in ids:
            problems.append(f"{RULE}: {rule_id} has a check in guard/rules.py but is not listed in {docs}")
    if run_probes:
        for rule_id in dict.fromkeys(ids):
            probe = registry.get(rule_id)
            if probe is None:
                continue
            try:
                fired = probe()
            except Exception as e:
                problems.append(f"{RULE}: {rule_id} probe failed to run: {type(e).__name__}: {e}")
                continue
            if not fired:
                problems.append(f"{RULE}: {rule_id} check did not fire on its own bad example")
    return problems


def main(docs: Path = DOCS) -> int:
    problems = check(docs)
    if problems:
        for p in problems:
            print(p)
        return 1
    print(f"rules: {len(REGISTRY)} rules listed, each check fires on its bad example")
    return 0
