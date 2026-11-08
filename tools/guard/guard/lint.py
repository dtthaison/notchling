"""The architecture lint: the first rules from CLAUDE.md, each with a bad and a good example.

Existing violations live in `tools/guard/lint-baseline.json`, which may only shrink; the lint
fails on any violation the baseline doesn't hold. A fingerprint is the rule, the file and the
line's text (spacing ignored), so moving code around doesn't count as new."""

from __future__ import annotations

import json
import re
import subprocess
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Dict, List, Optional, Tuple

from guard.source import strip_code, strip_comments
from guard.zones import glob_regex

EXCLUDE = ("legacy/macos/Tests/**", "legacy/macos/Demo/**", "tools/**", "apps/flutter/test/**")
BASELINE = Path("tools/guard/lint-baseline.json")
NOTCH_VIEWS = tuple(f"legacy/macos/Sources/Notchling/Notch/{n}.swift" for n in
                    ("NotchView", "NotchRows", "NotchPeek", "NotchDetail", "NotchCardParts", "TabPerch")) + \
              ("legacy/macos/Sources/Notchling/Notch/Pet/**",)

Found = List[Tuple[int, str]]
Check = Callable[[str, List[str], Path], Found]


@dataclass(frozen=True)
class Rule:
    id: str
    why: str
    paths: Tuple[str, ...]
    check: Check
    bad: Tuple[str, str]
    good: Tuple[str, str]
    exclude: Tuple[str, ...] = EXCLUDE
    warn: bool = False

    def applies(self, path: str) -> bool:
        if any(glob_regex(g).match(path) for g in self.exclude):
            return False
        return any(glob_regex(g).match(path) for g in self.paths)


@dataclass(frozen=True)
class Violation:
    rule: str
    path: str
    line: int
    text: str
    warn: bool = False


def _code_matches(*patterns: str) -> Check:
    regs = [re.compile(p) for p in patterns]

    def check(path: str, lines: List[str], repo: Path) -> Found:
        return [(i + 1, l.strip()) for i, l in enumerate(lines)
                if any(r.search(strip_code(path, l)) for r in regs)]
    return check


def _pointer_cursor(path: str, lines: List[str], repo: Path) -> Found:
    out = []
    for i, l in enumerate(lines):
        if ".onTapGesture" in strip_code(path, l):
            window = " ".join(strip_code(path, x) for x in lines[i:i + 4])
            if ".pointerCursor()" not in window:
                out.append((i + 1, l.strip()))
    return out


_MIGRATED_CACHE: Dict[str, frozenset] = {}
DEFAULTS_KEY = [re.compile(r"(?i)defaults[^\n]*forKey:\s*\"([^\"]+)\""), re.compile(r"@AppStorage\(\"([^\"]+)\"")]


def migrated_keys(repo: Path) -> frozenset:
    """The keys in `AppDelegate.migrateFromAwake`'s `for key in [...]` list."""
    path = Path(repo) / "legacy/macos/Sources/Notchling/App/AppDelegate.swift"
    cache_key = str(path)
    if cache_key not in _MIGRATED_CACHE:
        text = path.read_text() if path.exists() else ""
        start = text.find("func migrateFromAwake")
        m = re.search(r"for key in \[([^\]]*)\]", text[start:]) if start >= 0 else None
        _MIGRATED_CACHE[cache_key] = frozenset(re.findall(r"\"([^\"]+)\"", m.group(1))) if m else frozenset()
    return _MIGRATED_CACHE[cache_key]


def _defaults_migrated(path: str, lines: List[str], repo: Path) -> Found:
    keys = migrated_keys(repo) | {"migratedFromAwake"}
    out = []
    for i, l in enumerate(lines):
        text = strip_comments(path, l)
        for r in DEFAULTS_KEY:
            for m in r.finditer(text):
                if m.group(1) not in keys:
                    out.append((i + 1, l.strip()))
    return out


GPUI_DEP = re.compile(r"^\s*gpui\s*=")
# The one table a GPUI dependency may sit in (and its dev and build forms), until Linux and Windows build it.
# The cfg in a TOML literal string ('...') or a basic one ("..." with the inner quotes escaped).
MACOS_DEPS = re.compile(r"""^\[target\.(['"])cfg\(target_os = \\?"macos\\?"\)\1\.(dev-|build-)?dependencies(\.gpui)?\]\s*$""")
ANY_TABLE = re.compile(r"^\s*\[")


def _gpui_macos_only(path: str, lines: List[str], repo: Path) -> Found:
    """A `gpui` dependency line under any table but the macOS target's, or a `[dependencies.gpui]` table."""
    out = []
    macos = False
    for i, raw in enumerate(lines):
        line = strip_comments(path, raw).rstrip()
        if ANY_TABLE.match(line):
            macos = bool(MACOS_DEPS.match(line.strip()))
            if not macos and re.match(r"^\s*\[.*dependencies\.gpui\]\s*$", line):
                out.append((i + 1, raw.strip()))
            continue
        if GPUI_DEP.match(line) and not macos:
            out.append((i + 1, raw.strip()))
    return out


# What tools/protocol/gen.py writes, and nobody splits. Exact paths: a mark inside a file would let any file opt out.
GENERATED_FILES = frozenset({
    "crates/protocol/src/generated.rs",
    "apps/flutter/lib/core/protocol/generated.dart",
})


def _file_size(limit: int) -> Check:
    def check(path: str, lines: List[str], repo: Path) -> Found:
        if path in GENERATED_FILES:
            return []
        return [(0, f"over {limit} lines")] if len(lines) > limit else []
    return check


RULES: List[Rule] = [
    Rule("LINT-CORE-NO-NOTCHLING", "ConsoleCore must stay free of Notchling types (it's shared with hostd)",
         ("legacy/macos/Sources/ConsoleCore/**",), _code_matches(r"^\s*(@testable\s+)?import\s+Notchling\b"),
         ("legacy/macos/Sources/ConsoleCore/A.swift", "import Notchling\n"), ("legacy/macos/Sources/ConsoleCore/A.swift", "import Foundation\n")),
    Rule("LINT-CORE-MACOS13", "ConsoleCore targets macOS 13: no @Observable or macOS 14+ checks",
         ("legacy/macos/Sources/ConsoleCore/**",), _code_matches(r"@Observable\b", r"[#@]available\(macOS 1[4-9]"),
         ("legacy/macos/Sources/ConsoleCore/A.swift", "@Observable final class Store {}\n"),
         ("legacy/macos/Sources/ConsoleCore/A.swift", "final class Store: ObservableObject {}\n")),
    Rule("LINT-NO-FORCE-UNWRAP", "No force-unwraps, try! or as! outside probes and tests",
         ("legacy/macos/Sources/**/*.swift",), _code_matches(r"[\w\)\]]!(?!=)", r"\b(try|as)!"),
         ("legacy/macos/Sources/Notchling/A.swift", "let a = b!\n"), ("legacy/macos/Sources/Notchling/A.swift", "guard let a = b else { return }\n")),
    Rule("LINT-KIT-TOKENS", "Views use the design kit's Kit* tokens, not raw colors or font sizes",
         ("legacy/macos/Sources/Notchling/Chat/Views/**",) + NOTCH_VIEWS,
         _code_matches(r"Color\((red|white|hue|\.sRGB)", r"\.font\(\.system\(size:",
                       r"Color\.(red|green|blue|orange|yellow|purple|pink|gray|black|white)\b"),
         ("legacy/macos/Sources/Notchling/Chat/Views/A.swift", "Text(x).foregroundColor(Color.red)\n"),
         ("legacy/macos/Sources/Notchling/Chat/Views/A.swift", "Text(x).foregroundColor(KitInk.danger)\n")),
    Rule("LINT-DEFAULTS-MIGRATED", "Every UserDefaults key is in AppDelegate.migrateFromAwake's list",
         ("legacy/macos/Sources/Notchling/**",), _defaults_migrated,
         ("legacy/macos/Sources/Notchling/A.swift", 'UserDefaults.standard.bool(forKey: "newSetting")\n'),
         ("legacy/macos/Sources/Notchling/A.swift", 'UserDefaults.standard.bool(forKey: "mode")\n')),
    Rule("LINT-POINTER-CURSOR", "Clickable views show the pointing hand (.pointerCursor() or a kit style)",
         ("legacy/macos/Sources/Notchling/**",), _pointer_cursor,
         ("legacy/macos/Sources/Notchling/A.swift", "row\n    .onTapGesture { open() }\n"),
         ("legacy/macos/Sources/Notchling/A.swift", "row\n    .onTapGesture { open() }\n    .pointerCursor()\n")),
    Rule("LINT-NO-APPLESCRIPT", "No NSAppleScript or osascript (notifications show Script Editor's name)",
         ("legacy/macos/Sources/**/*.swift", "crates/**/*.rs"), _code_matches(r"\bNSAppleScript\b", r"\bosascript\b"),
         ("legacy/macos/Sources/Notchling/A.swift", "let s = NSAppleScript(source: src)\n"),
         ("legacy/macos/Sources/Notchling/A.swift", "// never osascript\n")),
    Rule("LINT-GPUI-MACOS-ONLY", "gpui is a dependency only under [target.'cfg(target_os = \"macos\")'.dependencies] "
         "(Linux needs system libraries CI doesn't install yet; Windows is unverified)",
         ("crates/**/Cargo.toml",), _gpui_macos_only,
         ("crates/kit/Cargo.toml", '[dependencies]\ngpui = { version = "=0.2.2", features = ["runtime_shaders"] }\n'),
         ("crates/kit/Cargo.toml", '[target.\'cfg(target_os = "macos")\'.dependencies]\ngpui = { version = "=0.2.2" }\n')),
    Rule("LINT-FILE-SIZE", "Files stay under 1,000 lines",
         ("legacy/macos/Sources/**", "crates/**", "apps/flutter/lib/**"), _file_size(1000),
         ("legacy/macos/Sources/Notchling/A.swift", "x\n" * 1001), ("legacy/macos/Sources/Notchling/A.swift", "x\n" * 1000)),
    Rule("LINT-FILE-SIZE-WARN", "Files over 600 lines should be split",
         ("legacy/macos/Sources/**", "crates/**", "apps/flutter/lib/**"), _file_size(600),
         ("legacy/macos/Sources/Notchling/A.swift", "x\n" * 601), ("legacy/macos/Sources/Notchling/A.swift", "x\n" * 600), warn=True),
]


def rule(rule_id: str) -> Rule:
    return next(r for r in RULES if r.id == rule_id)


def _tracked(repo: Path) -> List[str]:
    out = subprocess.run(["git", "-C", str(repo), "ls-files", "-z"], check=True, capture_output=True).stdout
    return [p for p in out.decode("utf-8", errors="replace").split("\0") if p]


def run(repo: Path, paths: Optional[List[str]] = None) -> List[Violation]:
    found: List[Violation] = []
    for path in paths if paths is not None else _tracked(repo):
        rules = [r for r in RULES if r.applies(path)]
        if not rules:
            continue
        try:
            lines = (Path(repo) / path).read_text(encoding="utf-8").splitlines()
        except (OSError, UnicodeDecodeError):
            continue
        for r in rules:
            for line_no, text in r.check(path, lines, Path(repo)):
                found.append(Violation(r.id, path, line_no, text, r.warn))
    return found


def fingerprint(v: Violation) -> str:
    return f"{v.rule}|{v.path}|{' '.join(v.text.split())}"


def new_violations(found: List[Violation], baseline: List[str]) -> List[Violation]:
    left = Counter(baseline)
    out = []
    for v in found:
        fp = fingerprint(v)
        if left[fp] > 0:
            left[fp] -= 1
        else:
            out.append(v)
    return out


def read_baseline(path: Path) -> List[str]:
    return json.loads(path.read_text())["entries"] if path.exists() else []


def write_baseline(path: Path, entries: List[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({
        "_comment": "Generated by python3 -m guard lint. Existing violations; it may only shrink "
                    "(python3 -m guard lint --update-baseline). Changing it is a protected change.",
        "entries": sorted(entries),
    }, indent=1, ensure_ascii=False) + "\n")


def main(repo: Path, write_new: bool = False, update: bool = False, show_warnings: bool = False) -> int:
    found = run(repo)
    errors = [v for v in found if not v.warn]
    warnings = [v for v in found if v.warn]
    path = Path(repo) / BASELINE
    if write_new:
        if path.exists():
            print(f"GUARD-BASELINE: {BASELINE} already exists; it can only shrink (--update-baseline)")
            return 1
        write_baseline(path, [fingerprint(v) for v in errors])
        counts = Counter(v.rule for v in errors)
        print(f"wrote {BASELINE}: " + ", ".join(f"{k} {n}" for k, n in sorted(counts.items())))
        return 0
    baseline = read_baseline(path)
    new = new_violations(errors, baseline)
    if update:
        if new:
            for v in new:
                print(f"{v.path}:{v.line}: {v.rule} {rule(v.rule).why}")
            print("GUARD-BASELINE: the baseline can only shrink; fix these instead")
            return 1
        kept = Counter(fingerprint(v) for v in errors) & Counter(baseline)
        write_baseline(path, list(kept.elements()))
        print(f"baseline: {len(baseline)} → {sum(kept.values())}")
        return 0
    for v in new:
        print(f"{v.path}:{v.line}: {v.rule} {rule(v.rule).why}")
    if show_warnings:
        for v in warnings:
            print(f"{v.path}: warning {v.rule} {rule(v.rule).why}")
    print(f"lint: {len(new)} new, {len(errors) - len(new)} in the baseline, {len(warnings)} warnings")
    return 1 if new else 0
