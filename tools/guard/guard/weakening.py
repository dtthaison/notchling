from __future__ import annotations

import re
import unicodedata
from typing import List

from guard.capability import Trip
from guard.changes import Change

TEST_PATHS = [re.compile(p) for p in (
    r"^legacy/macos/Tests/", r"^legacy/macos/Fixtures/", r"^apps/flutter/test/", r"^tools/guard/tests/",
    r"^crates/[^/]+/tests/", r"(^|/)goldens/.*\.png$",
)]
SKIPS = [re.compile(p) for p in (
    r"\bXCTSkip\b", r"#\[ignore\b", r"\bskip:\s*(true|['\"])", r"@Skip\(",
    r"\binvokeTest\b", r"\bdefaultTestSuite\b", r"\btestInvocations\b", r"\bgoldenFileComparator\b",
)]

INVISIBLE = frozenset(
    list(range(0xFE00, 0xFE10)) + list(range(0xE0100, 0xE01F0))
    + list(range(0x180B, 0x1810))
    + [0x3164, 0xFFA0, 0x115F, 0x1160, 0x17B4, 0x17B5, 0x00AD]
)
BAD_CATEGORIES = frozenset({"Cf", "Cc", "Zl", "Zp"})


def _is_test(path: str) -> bool:
    return any(p.search(path) for p in TEST_PATHS)


def hidden(ch: str) -> bool:
    if ch == "\t":
        return False
    return ord(ch) in INVISIBLE or unicodedata.category(ch) in BAD_CATEGORIES


def scan(changes: List[Change]) -> List[Trip]:
    trips: List[Trip] = []
    for c in changes:
        ends = [c.path] + ([c.old_path] if c.old_path else [])
        if c.status in ("M", "D", "R") and any(_is_test(p) for p in ends):
            trips.append(Trip("WEAK-TEST-EDIT", c.old_path or c.path, 0, f"{c.status} {c.path}"))
        for line_no, text in c.added:
            if any(p.search(text) for p in SKIPS):
                trips.append(Trip("WEAK-SKIP", c.path, line_no, text.strip()))
            body = text[:-1] if text.endswith("\n") else text
            if any(hidden(ch) for ch in body):
                shown = "".join(f"<U+{ord(ch):04X}>" if hidden(ch) else ch for ch in body)
                trips.append(Trip("WEAK-UNICODE", c.path, line_no, shown.strip(" \t")))
    return trips
