#!/usr/bin/env python3
from __future__ import annotations

import re
from typing import Optional, Tuple

Version = Tuple[int, int, int, Optional[int]]

NUM = r"(0|[1-9][0-9]*)"
TAG_RE = re.compile(rf"^v{NUM}\.{NUM}\.{NUM}(?:-beta\.([1-9][0-9]*))?$")
MAX_BETA = 98
MAX_PART = 99


def parse_tag(tag: str) -> Optional[Version]:
    m = TAG_RE.match(tag)
    if not m:
        return None
    x, y, z, n = m.groups()
    return int(x), int(y), int(z), (int(n) if n is not None else None)


def format_tag(v: Version) -> str:
    base = f"v{v[0]}.{v[1]}.{v[2]}"
    return base if v[3] is None else f"{base}-beta.{v[3]}"


def build_number(v: Version) -> int:
    x, y, z, n = v
    if y > MAX_PART or z > MAX_PART:
        raise ValueError(f"minor and patch stop at {MAX_PART} ({format_tag(v)})")
    if n is not None and not 1 <= n <= MAX_BETA:
        raise ValueError(f"a beta number must be 1 to {MAX_BETA} ({format_tag(v)})")
    return x * 1000000 + y * 10000 + z * 100 + (99 if n is None else n)
