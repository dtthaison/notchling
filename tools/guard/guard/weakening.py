"""The test-weakening check: changing what already proves the app works goes to the maintainer.

Editing, deleting or renaming an existing test, fixture or golden image; adding a skip; and
invisible or bidirectional Unicode anywhere (it can make code read differently from what runs).
New tests are fine."""

from __future__ import annotations

import re
from typing import List

from guard.capability import Trip
from guard.changes import Change

TEST_PATHS = [re.compile(p) for p in (
    r"^Tests/", r"^Fixtures/", r"^mobile/test/", r"^tools/guard/tests/",
    r"^crates/[^/]+/tests/", r"(^|/)goldens/.*\.png$",
)]
SKIPS = [re.compile(p) for p in (r"\bXCTSkip\b", r"#\[ignore\b", r"\bskip:\s*(true|['\"])", r"@Skip\(")]
HIDDEN = re.compile("[​-‏‪-‮⁠-⁤⁦-⁩﻿]")


def _is_test(path: str) -> bool:
    return any(p.search(path) for p in TEST_PATHS)


def scan(changes: List[Change]) -> List[Trip]:
    trips: List[Trip] = []
    for c in changes:
        ends = [c.path] + ([c.old_path] if c.old_path else [])
        if c.status in ("M", "D", "R") and any(_is_test(p) for p in ends):
            trips.append(Trip("WEAK-TEST-EDIT", c.old_path or c.path, 0, f"{c.status} {c.path}"))
        for line_no, text in c.added:
            if any(p.search(text) for p in SKIPS):
                trips.append(Trip("WEAK-SKIP", c.path, line_no, text.strip()))
            if HIDDEN.search(text):
                shown = HIDDEN.sub(lambda m: f"<U+{ord(m.group()):04X}>", text.strip())
                trips.append(Trip("WEAK-UNICODE", c.path, line_no, shown))
    return trips
