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


def cmd_lint(args: argparse.Namespace) -> int:
    from guard import lint

    return lint.main(Path("."), write_new=args.write_baseline, update=args.update_baseline,
                     show_warnings=args.warnings)


def cmd_gate(args: argparse.Namespace) -> int:
    from guard import gate

    return gate.main(Path(args.repo), args.base, args.head, Path(args.reviews) if args.reviews else None,
                     args.post, args.target_url)


def cmd_pins(args: argparse.Namespace) -> int:
    from guard import pins

    workflows = Path(".github/workflows")
    if args.fix:
        for line in pins.pin(workflows):
            print(line)
    problems = pins.check(workflows, pins.load_allowlist(pins.ALLOWLIST))
    for p in problems:
        print(p)
    if not problems:
        print("actions pinned and allowlisted")
    return 1 if problems else 0


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(prog="guard")
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("pins", help="every workflow action pinned to a SHA and allowlisted")
    p.add_argument("--fix", action="store_true", help="pin tag references to their SHA (uses git ls-remote)")
    p.set_defaults(func=cmd_pins)
    p = sub.add_parser("gate", help="does this change need the maintainer, and is it approved")
    p.add_argument("--repo", default=".")
    p.add_argument("--base", required=True)
    p.add_argument("--head", required=True)
    p.add_argument("--reviews", help="the PR's reviews as GitHub's list-reviews JSON")
    p.add_argument("--post", action="store_true", help="post the maintainer-gate commit status (workflow only)")
    p.add_argument("--target-url")
    p.set_defaults(func=cmd_gate)
    p = sub.add_parser("lint", help="the architecture lint against its baseline")
    mode = p.add_mutually_exclusive_group()
    mode.add_argument("--write-baseline", action="store_true", help="first run only: record what exists")
    mode.add_argument("--update-baseline", action="store_true", help="shrink the baseline to what's left")
    p.add_argument("--warnings", action="store_true")
    p.set_defaults(func=cmd_lint)
    p = sub.add_parser("codeowners", help="render or check .github/CODEOWNERS")
    mode = p.add_mutually_exclusive_group()
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    p.set_defaults(func=cmd_codeowners)
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
