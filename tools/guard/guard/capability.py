from __future__ import annotations

import os
import re
from dataclasses import dataclass
from typing import Dict, List, Pattern, Tuple

from guard.changes import Change
from guard.source import SLASH, strip_code, strip_comments

CODE_EXTS = {".swift", ".rs", ".dart", ".sh", ".py"}
BINARY_RULE = "CAP-BINARY"
OBSCURE_RULE = "CAP-OBSCURE"
MAX_COMMENT_ENDS = 8

OWN_FILES: Dict[str, Tuple[str, ...]] = {
    "VaultStore": ("legacy/macos/Sources/Notchling/Vault/VaultStore.swift",),
    "AccountStore": ("legacy/macos/Sources/Notchling/Accounts/Accounts.swift",),
    "LaneStore": ("legacy/macos/Sources/Notchling/Lanes/Lanes.swift",),
    "ReplySender": ("legacy/macos/Sources/Notchling/Agents/ReplySender.swift", "legacy/macos/Sources/Notchling/Agents/ReplySender+Dialog.swift"),
    "HookBridge": ("legacy/macos/Sources/Notchling/Hook/HookBridge.swift",),
    "OwnerPublish": ("legacy/macos/Sources/Notchling/Pages/OwnerPublish.swift",),
    "TerminalSession": ("legacy/macos/Sources/Notchling/Terminal/TerminalSession.swift",),
    "AgentActions": ("legacy/macos/Sources/Notchling/Agents/AgentActions.swift",),
    "CodexRunner": ("legacy/macos/Sources/Notchling/Agents/CodexRunner.swift",),
    "AntigravityRunner": ("legacy/macos/Sources/Notchling/Agents/AntigravityCLI.swift",),
    "ACPRunner": ("legacy/macos/Sources/Notchling/Agents/ACPRunner.swift",),
    "ACPConnection": ("legacy/macos/Sources/Notchling/Agents/ACPConnection.swift",),
    "HostTerminals": ("legacy/macos/Sources/Notchling/Agents/ReplySender.swift",),
    "SecretRunner": ("legacy/macos/Sources/Notchling/Vault/SecretVault.swift",),
    "SecretVault": ("legacy/macos/Sources/Notchling/Vault/SecretVault.swift",),
    "VaultServer": ("legacy/macos/Sources/Notchling/Vault/VaultServer.swift",),
}
PROTECTED_TYPES = list(OWN_FILES)


@dataclass(frozen=True)
class Trip:
    rule: str
    path: str
    line: int
    text: str


def _c(*patterns: str) -> List[Pattern[str]]:
    return [re.compile(p) for p in patterns]


_FM_CALLS = r"createFile|createDirectory|createSymbolicLink|copyItem|moveItem|replaceItem|replaceItemAt|linkItem|trashItem"
_FETCHERS = r"Data|NSData|String|NSString|NSArray|NSMutableArray|NSDictionary|NSMutableDictionary|NSImage|NSAttributedString"

_RS_FS_WRITES = r"write|remove_\w+|rename|create_dir\w*|copy|hard_link|symlink\w*|set_permissions"
_RS_COMMAND_IMPORT = r"\bprocess::(?:Command\b|\{(?:[^}]*\bCommand\b|[^}]*$))"

_WORKSPACE_OPEN = r"\bNSWorkspace\b[^;]*\.(open|openApplication|openURLs?|launchApplication|launchApp|openFile|openTemporaryFile)\b"

RULES: Dict[str, List[Pattern[str]]] = {
    "CAP-PROCESS": _c(r"\bProcess\b", r"\blaunchPath\b", r"\.launch\s*\(\s*\)", r"\.executableURL\b",
                      r"std::process::Command", _RS_COMMAND_IMPORT, r"\bCommand::new\b", r"\bCommandExt\b",
                      r"\bsubprocess\.", r"\bos\.system\(",
                      r"\bpopen\b", _WORKSPACE_OPEN, r"\bNSWorkspace\.shared\b(?!\s*\.)"),
    "CAP-PTY": _c(r"\bforkpty\b", r"\bposix_spawn\w*\b", r"\bportable_pty\b", r"\bopenpty\b"),
    "CAP-APPLESCRIPT": _c(r"\bNSAppleScript\b", r"\bosascript\b"),
    "CAP-DYNAMIC": _c(r"\bdlsym\b", r"\bdlopen\b", r"@_silgen_name\b", r"@_cdecl\b"),
    "CAP-NETWORK": _c(r"\bURLSession\b", r"\bNWConnection\b", r"\breqwest\b", r"\bHttpClient\b", r"\bWebSocket\b",
                      r"\b(%s)\s*\(\s*contentsOf\s*:" % _FETCHERS, r"\.init\s*\(\s*contentsOf\s*:",
                      r"\bSocket\.(connect|startConnect)\b", r"\b(RawSocket|ServerSocket|HttpServer)\b",
                      r"\b(TcpStream|TcpListener|UdpSocket)\b"),
    "CAP-KEYCHAIN": _c(r"\bSecItem\w+", r"\bkeyring\b", r"\bSecKeychain\w*"),
    "CAP-FILEWRITE": _c(r"FileManager.*\.(createFile|createDirectory|moveItem|copyItem|removeItem|replaceItem|setAttributes)\b",
                        r"\.(%s)\s*\(" % _FM_CALLS, r"\.removeItem\s*\(\s*at(Path)?\s*:",
                        r"\.write\(to:", r"\btoFile\s*:", r"\bFileHandle\s*\(\s*for(Writing|Updating)",
                        r"\bfs::(%s)\b" % _RS_FS_WRITES, r"\bfs::\*", r"\bfs::\{(?:[^}]*\b(?:%s)\b|[^}]*$)" % _RS_FS_WRITES,
                        r"\bFile::(create|create_new|options)\b", r"\bOpenOptions\b",
                        r"\bFile\([^)]*\)\.(writeAs\w+|delete|rename)\b"),
    "CAP-PROTECTED-TYPE": [],
}

IN_STRINGS: Dict[str, List[Pattern[str]]] = {
    "CAP-APPLESCRIPT": _c(r"\bosascript\b"),
    "CAP-KEYCHAIN": _c(r"/usr/bin/security\b"),
    "CAP-NETWORK": _c(r"^\s*import\s+['\"]package:(http|dio|web_socket_channel)/"),
}

TYPE_USE = re.compile(r"\b(" + "|".join(PROTECTED_TYPES) + r")\b")


def _own_file(path: str, type_name: str) -> bool:
    return path in OWN_FILES.get(type_name, ())


def _view_trips(path: str, text: str) -> List[str]:
    code, comments_out = strip_code(path, text), strip_comments(path, text)
    rules = [r for r, pats in RULES.items() if any(p.search(code) for p in pats)]
    rules += [r for r, pats in IN_STRINGS.items() if r not in rules and any(p.search(comments_out) for p in pats)]
    if any(not _own_file(path, m.group(1)) for m in TYPE_USE.finditer(code)):
        rules.append("CAP-PROTECTED-TYPE")
    return rules


def _line_trips(path: str, text: str) -> List[str]:
    views = [text]
    if os.path.splitext(path)[1].lower() in SLASH:
        ends = [m.end() for m in re.finditer(r"\*/", text)]
        if len(ends) > MAX_COMMENT_ENDS:
            return [OBSCURE_RULE]
        views += [text[e:] for e in ends]
    rules: List[str] = []
    for view in views:
        rules += [r for r in _view_trips(path, view) if r not in rules]
    return rules


def _pieces(text: str) -> List[str]:
    return text.split("\r")


def scan(changes: List[Change]) -> List[Trip]:
    trips: List[Trip] = []
    for c in changes:
        if os.path.splitext(c.path)[1].lower() not in CODE_EXTS:
            continue
        if c.binary:
            trips.append(Trip(BINARY_RULE, c.path, 0, "binary change to a code file"))
        for line_no, text in c.added:
            for piece in _pieces(text):
                for rule in _line_trips(c.path, piece):
                    trips.append(Trip(rule, c.path, line_no, piece.strip()))
    return trips
