"""python3 -m guard <command>, run from the repository root (PYTHONPATH=tools/guard)."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import List, Optional

ZONES = Path("tools/guard/zones.json")
CODEOWNERS = Path(".github/CODEOWNERS")


def cmd_codeowners(args: argparse.Namespace) -> int:
    from guard.codeowners import render
    from guard.zones import load

    text = render(load(ZONES))
    if args.write:
        CODEOWNERS.parent.mkdir(parents=True, exist_ok=True)
        CODEOWNERS.write_text(text)
        print(f"wrote {CODEOWNERS}")
        return 0
    current = CODEOWNERS.read_text() if CODEOWNERS.exists() else ""
    if current != text:
        print(f"GUARD-CODEOWNERS: {CODEOWNERS} is out of date; run python3 -m guard codeowners --write")
        return 1
    print("CODEOWNERS up to date")
    return 0


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(prog="guard")
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("codeowners", help="render or check .github/CODEOWNERS")
    mode = p.add_mutually_exclusive_group()
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    p.set_defaults(func=cmd_codeowners)
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
