"""The capability check: a new use of a powerful API sends the PR to the maintainer, wherever the
file is. It reads only added lines in code files, with comments removed. Most patterns ignore
string contents too; the ones that only appear inside strings (a tool's path, a Dart import) keep
them."""

from __future__ import annotations

import os
import re
from dataclasses import dataclass
from typing import Dict, List, Pattern

from guard.changes import Change
from guard.source import strip_code, strip_comments

CODE_EXTS = {".swift", ".rs", ".dart", ".sh", ".py"}
PROTECTED_TYPES = ["VaultStore", "AccountStore", "LaneStore", "ReplySender", "HookBridge", "OwnerPublish"]


@dataclass(frozen=True)
class Trip:
    rule: str
    path: str
    line: int
    text: str


def _c(*patterns: str) -> List[Pattern[str]]:
    return [re.compile(p) for p in patterns]


# Matched against the line with comments and string contents blanked.
RULES: Dict[str, List[Pattern[str]]] = {
    "CAP-PROCESS": _c(r"\bProcess\s*\(", r"\.executableURL\b", r"std::process::Command", r"\bprocess::Command\b",
                      r"\bProcess\.(run|start)\b", r"\bsubprocess\.", r"\bos\.system\("),
    "CAP-PTY": _c(r"\bforkpty\b", r"\bposix_spawn\w*\b", r"\bportable_pty\b", r"\bopenpty\b"),
    "CAP-APPLESCRIPT": _c(r"\bNSAppleScript\b", r"\bosascript\b"),
    "CAP-NETWORK": _c(r"\bURLSession\b", r"\bNWConnection\b", r"\breqwest\b", r"\bHttpClient\b", r"\bWebSocket\b"),
    "CAP-KEYCHAIN": _c(r"\bSecItem\w+", r"\bkeyring\b", r"\bSecKeychain\w*"),
    "CAP-FILEWRITE": _c(r"FileManager.*\.(createFile|createDirectory|moveItem|copyItem|removeItem|replaceItem|setAttributes)\b",
                        r"\.write\(to:", r"\bstd::fs::(write|remove_\w+|rename|create_dir\w*|copy)\b",
                        r"\bFile\([^)]*\)\.(writeAs\w+|delete|rename)\b"),
    "CAP-PROTECTED-TYPE": [],  # by file name, below
}

# Matched against the line with only comments blanked: these live inside string literals.
IN_STRINGS: Dict[str, List[Pattern[str]]] = {
    "CAP-APPLESCRIPT": _c(r"\bosascript\b"),
    "CAP-KEYCHAIN": _c(r"/usr/bin/security\b"),
    "CAP-NETWORK": _c(r"^\s*import\s+['\"]package:(http|dio|web_socket_channel)/"),
}

TYPE_USE = re.compile(r"\b(" + "|".join(PROTECTED_TYPES) + r")\b")


def _own_file(path: str, type_name: str) -> bool:
    return os.path.basename(path).startswith(type_name)


def _line_trips(path: str, text: str) -> List[str]:
    code, comments_out = strip_code(path, text), strip_comments(path, text)
    rules = [r for r, pats in RULES.items() if any(p.search(code) for p in pats)]
    rules += [r for r, pats in IN_STRINGS.items() if r not in rules and any(p.search(comments_out) for p in pats)]
    if any(not _own_file(path, m.group(1)) for m in TYPE_USE.finditer(code)):
        rules.append("CAP-PROTECTED-TYPE")
    return rules


def scan(changes: List[Change]) -> List[Trip]:
    trips: List[Trip] = []
    for c in changes:
        if os.path.splitext(c.path)[1].lower() not in CODE_EXTS or c.binary:
            continue
        for line_no, text in c.added:
            for rule in _line_trips(c.path, text):
                trips.append(Trip(rule, c.path, line_no, text.strip()))
    return trips
