#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import re
import shutil
import sys
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import check
import emit_android
import emit_arb
import emit_rust
import emit_xcstrings
import ftl
import report

ROOT = Path(__file__).resolve().parents[2]
RULE_GEN = "GUARD-LOCALE-GEN"
RULE_SUBSET = check.RULE_SUBSET
RULE_MATCH = check.RULE_MATCH
LOCALES = ROOT / "locales"
RUST_OUT = ROOT / "crates/locale/src/generated.rs"
ARB_DIR = ROOT / "apps/flutter/lib/l10n"


def shown(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return str(path)


def validate(root: Path) -> Tuple[check.Tree, List[check.Problem]]:
    tree, problems = check.load(root)
    problems = list(problems)
    problems += check.check_structure(tree)
    problems += check.check_match(tree)
    return tree, problems


def expected(tree: check.Tree, rust: Path, arb_dir: Path) -> Dict[Path, str]:
    out: Dict[Path, str] = {}
    outputs = tree.config.outputs
    if "rust" in outputs:
        out[rust] = emit_rust.render(tree)
    if "arb" in outputs:
        for tag in tree.config.ship:
            if tag in tree.langs:
                out[arb_dir / emit_arb.file_name(tag)] = emit_arb.render(tree, tag)
    return out


def read_raw(path: Path) -> Dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8"))


def write_raw(path: Path, raw: Dict[str, object]) -> None:
    body = ",\n".join(f'  "{key}": {json.dumps(value, ensure_ascii=False)}' for key, value in raw.items())
    path.write_text("{\n" + body + "\n}\n", encoding="utf-8")


def command_new(root: Path, tag: str) -> int:
    if not check.TAG_RE.match(tag):
        print(f"{RULE_SUBSET}: {tag}: not a language tag in the form de, pt-BR, zh-Hans", file=sys.stderr)
        return 1
    target = root / tag
    if target.exists():
        print(f"{RULE_SUBSET}: {shown(target)}: the folder exists already", file=sys.stderr)
        return 1
    cfg_path = root / "languages.json"
    raw = read_raw(cfg_path)
    if tag in raw.get("ship", []) or tag in raw.get("draft", []):
        print(f"{RULE_SUBSET}: {shown(cfg_path)}: {tag} is listed already", file=sys.stderr)
        return 1
    source = root / str(raw.get("source", "en"))
    shutil.copytree(source, target)
    shared = target / "shared.ftl"
    if shared.is_file():
        kept = [l for l in shared.read_text(encoding="utf-8").split("\n") if not l.startswith("-brand-")]
        shared.write_text(re.sub(r"\n{3,}", "\n\n", "\n".join(kept)), encoding="utf-8")
    raw["draft"] = list(raw.get("draft", [])) + [tag]
    write_raw(cfg_path, raw)
    print(f"created {shown(target)} and listed {tag} in draft; translate it, then run python3 tools/locale/gen.py")
    return 0


def prune_text(text: str, res: ftl.Resource, keys: set) -> Tuple[str, int]:
    lines = text.split("\n")
    drop = set()
    count = 0
    for msg in res.messages:
        if msg.key in keys:
            count += 1
            for no in range(msg.first, msg.last + 1):
                drop.add(no)
    kept = [line for no, line in enumerate(lines, 1) if no not in drop]
    joined = re.sub(r"\n{3,}", "\n\n", "\n".join(kept)).strip("\n")
    return (joined + "\n" if joined else ""), count


def command_prune(root: Path) -> int:
    tree, problems = check.load(root)
    if problems:
        for p in problems:
            print(p, file=sys.stderr)
        return 1
    en = check.english(tree)
    total = 0
    for tag, lang in sorted(tree.langs.items()):
        if tag == tree.config.source:
            continue
        for rel, res in sorted(lang.files.items()):
            stale = {m.key for m in res.messages if m.key not in en}
            if not stale:
                continue
            path = root / tag / rel
            new, count = prune_text(path.read_text(encoding="utf-8"), res, stale)
            if new:
                path.write_text(new, encoding="utf-8")
            else:
                path.unlink()
            total += count
            print(f"pruned {count} from {res.path}")
    print(f"pruned {total} messages")
    return 0


def command_report(tree: check.Tree, args: argparse.Namespace) -> int:
    if args.missing:
        tags = [args.lang] if args.lang else [t for t in check.listed(tree.config) if t != tree.config.source]
        for tag in tags:
            for key in report.missing(tree, tag):
                print(f"{tag}: {key}")
        return 0
    items = report.rows(tree, args.lang)
    sys.stdout.write(report.markdown(items) if args.markdown else report.text(items))
    return 0


def command_check(tree: check.Tree, files: Dict[Path, str], arb_dir: Path) -> int:
    status = 0
    for path, text in sorted(files.items()):
        have = path.read_text(encoding="utf-8") if path.is_file() else None
        if have != text:
            print(f"{RULE_GEN}: {shown(path)}:1: out of date; run python3 tools/locale/gen.py", file=sys.stderr)
            status = 1
    if "arb" in tree.config.outputs and arb_dir.is_dir():
        keep = {p.resolve() for p in files}
        for path in sorted(arb_dir.glob("app_*.arb")):
            if path.resolve() not in keep:
                print(
                    f"{RULE_GEN}: {shown(path)}:1: no shipped language writes this file; run python3 tools/locale/gen.py",
                    file=sys.stderr,
                )
                status = 1
    return status


def command_write(files: Dict[Path, str], arb_on: bool, arb_dir: Path) -> None:
    for path, text in sorted(files.items()):
        if path.is_file() and path.read_text(encoding="utf-8") == text:
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        print(f"wrote {shown(path)}")
    if arb_on and arb_dir.is_dir():
        keep = {p.resolve() for p in files}
        for path in sorted(arb_dir.glob("app_*.arb")):
            if path.resolve() not in keep:
                path.unlink()
                print(f"removed {shown(path)}")


def command_native(tree: check.Tree, ios: Optional[str], android: Optional[str]) -> None:
    if ios:
        target = Path(ios)
        target.mkdir(parents=True, exist_ok=True)
        (target / "Localizable.xcstrings").write_text(emit_xcstrings.render(tree), encoding="utf-8")
        print(f"wrote {shown(target / 'Localizable.xcstrings')}")
    if android:
        target = Path(android)
        for rel, text in emit_android.render(tree).items():
            dest = target / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(text, encoding="utf-8")
            print(f"wrote {shown(dest)}")


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="Check locales/ and write every platform's words.")
    parser.add_argument("--check", action="store_true", help="write nothing; exit 1 on a problem or a stale output")
    parser.add_argument("--report", action="store_true", help="print how complete each language is")
    parser.add_argument("--lang", help="with --report: one language")
    parser.add_argument("--missing", action="store_true", help="with --report: list the missing keys")
    parser.add_argument("--markdown", action="store_true", help="with --report: a markdown table")
    parser.add_argument("--new", metavar="TAG", help="copy en/ to TAG/ as a start and list it as a draft")
    parser.add_argument("--prune", action="store_true", help="remove keys English no longer has from every translation")
    parser.add_argument("--ios", metavar="DIR", help="write Localizable.xcstrings into DIR")
    parser.add_argument("--android", metavar="DIR", help="write values*/strings.xml into DIR")
    parser.add_argument("--locales", default=str(LOCALES), help="the locales folder")
    parser.add_argument("--rust", default=str(RUST_OUT), help="where the Rust tables go")
    parser.add_argument("--arb-dir", default=str(ARB_DIR), help="where the ARB files go")
    args = parser.parse_args(argv)

    root = Path(args.locales)
    rust = Path(args.rust)
    arb_dir = Path(args.arb_dir)

    if args.new:
        return command_new(root, args.new)
    if args.prune:
        return command_prune(root)

    tree, problems = validate(root)
    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        return 1

    if args.report:
        return command_report(tree, args)
    if args.ios or args.android:
        command_native(tree, args.ios, args.android)
        return 0

    files = expected(tree, rust, arb_dir)
    if args.check:
        return command_check(tree, files, arb_dir)
    command_write(files, "arb" in tree.config.outputs, arb_dir)
    sys.stdout.write(report.text(report.rows(tree)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
