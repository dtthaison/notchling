#!/usr/bin/env python3
"""Changelog fragments: one small file per change in changes/, folded into CHANGELOG.md at release.

    fragments.py check --base <sha> --head <sha> [--label no-changelog]...
    fragments.py notes [--since <tag>]          Markdown release notes, grouped, on stdout
    fragments.py fold <version>                 move the fragments into CHANGELOG.md, delete them

A fragment is changes/<name>.md: front matter between `---` lines with `kind: fix|feature|breaking|internal`,
then one short paragraph. changes/README.md explains it and is not a fragment.
"""

from __future__ import annotations

import argparse
import datetime
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import List, NamedTuple, Optional, Sequence

KINDS = ("fix", "feature", "breaking", "internal")
CHANGES_DIR = "changes"
NO_CHANGELOG_LABEL = "no-changelog"

# Paths whose change is something a user could notice, so the PR needs a fragment.
APP_CODE = (
    re.compile(r"^legacy/macos/Sources/"),
    re.compile(r"^apps/flutter/lib/"),
    re.compile(r"^crates/([^/]+/)+src/"),
)

# kind -> (CHANGELOG section, bullet prefix). `internal` is not in the log or the notes: it exists so a PR
# that touches app code with nothing to tell users can still say so.
SECTIONS = {
    "feature": ("Added", ""),
    "breaking": ("Changed", "**Breaking:** "),
    "fix": ("Fixed", ""),
}
SECTION_ORDER = ("Added", "Changed", "Fixed")
NOTHING = "No user-facing changes."


class FragmentError(Exception):
    pass


class Fragment(NamedTuple):
    name: str
    kind: str
    text: str


def parse(name: str, content: str) -> Fragment:
    """A fragment's kind and its one paragraph, or a FragmentError that names the file."""
    lines = [line.rstrip() for line in content.replace("\r\n", "\n").split("\n")]
    if not lines or lines[0] != "---":
        raise FragmentError(f"{name}: must start with a line of ---, then 'kind: ...'")
    try:
        end = lines.index("---", 1)
    except ValueError:
        raise FragmentError(f"{name}: front matter is not closed by a line of ---") from None
    fields = {}
    for line in lines[1:end]:
        if not line.strip():
            continue
        key, sep, value = line.partition(":")
        key, value = key.strip(), value.strip()
        if not sep or not key:
            raise FragmentError(f"{name}: front matter line {line!r} is not 'key: value'")
        if key != "kind":
            raise FragmentError(f"{name}: unknown front matter key {key!r} (only 'kind')")
        if key in fields:
            raise FragmentError(f"{name}: 'kind' given twice")
        fields[key] = value
    kind = fields.get("kind", "")
    if not kind:
        raise FragmentError(f"{name}: missing 'kind: {'|'.join(KINDS)}'")
    if kind not in KINDS:
        raise FragmentError(f"{name}: kind {kind!r} is not one of {', '.join(KINDS)}")
    paragraphs = [p for p in "\n".join(lines[end + 1:]).split("\n\n") if p.strip()]
    if not paragraphs:
        raise FragmentError(f"{name}: no text after the front matter")
    if len(paragraphs) > 1:
        raise FragmentError(f"{name}: one short paragraph only, found {len(paragraphs)}")
    return Fragment(name, kind, " ".join(paragraphs[0].split()))


def is_fragment_name(name: str) -> bool:
    return name.endswith(".md") and name != "README.md"


def load(changes_dir) -> List[Fragment]:
    """Every fragment in the folder, by file name. A missing folder has none."""
    d = Path(changes_dir)
    if not d.is_dir():
        return []
    found = []
    for p in sorted(d.iterdir()):
        if p.is_file() and is_fragment_name(p.name):
            found.append(parse(p.name, p.read_text(encoding="utf-8")))
    return found


def sections(frags: Sequence[Fragment]):
    """[(heading, [bullet, ...])] in CHANGELOG order, user-facing kinds only."""
    out = {}
    for f in frags:
        if f.kind in SECTIONS:
            heading, prefix = SECTIONS[f.kind]
            out.setdefault(heading, []).append(f"- {prefix}{f.text}")
    return [(h, out[h]) for h in SECTION_ORDER if h in out]


def render_notes(frags: Sequence[Fragment]) -> str:
    secs = sections(frags)
    if not secs:
        return NOTHING + "\n"
    return "\n".join(f"### {h}\n\n" + "\n".join(b) + "\n" for h, b in secs)


def git(repo, *args) -> str:
    env = dict(os.environ, LC_ALL="C")
    r = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True, env=env)
    if r.returncode != 0:
        raise FragmentError(f"git {' '.join(args)}: {r.stderr.strip() or 'failed'}")
    return r.stdout


def is_app_code(path: str) -> bool:
    return any(rx.search(path) for rx in APP_CODE)


def cmd_check(args) -> int:
    out = git(args.repo, "diff", "--name-status", "--no-renames", "-z", f"{args.base}...{args.head}")
    parts = out.split("\0")
    changes = []
    for i in range(0, len(parts) - 1, 2):
        changes.append((parts[i], parts[i + 1]))
    added = [p for s, p in changes
             if s == "A" and p.startswith(CHANGES_DIR + "/") and "/" not in p[len(CHANGES_DIR) + 1:]
             and is_fragment_name(p.rsplit("/", 1)[1])]
    for p in added:
        parse(p, git(args.repo, "show", f"{args.head}:{p}"))
    app = [p for _, p in changes if is_app_code(p)]
    if not app or NO_CHANGELOG_LABEL in (args.label or []) or added:
        return 0
    shown = "\n".join(f"  {p}" for p in app[:10]) + (f"\n  ... and {len(app) - 10} more" if len(app) > 10 else "")
    raise FragmentError(
        f"this change touches app code but adds no changelog fragment:\n{shown}\n"
        f"Add {CHANGES_DIR}/<name>.md (see {CHANGES_DIR}/README.md; kind: internal if users won't notice), "
        f"or have the maintainer label the PR {NO_CHANGELOG_LABEL}.")


def cmd_notes(args) -> int:
    frags = load(Path(args.repo) / args.changes_dir)
    if args.since:
        git(args.repo, "rev-parse", "--verify", "--quiet", f"{args.since}^{{commit}}")
        listed = git(args.repo, "ls-tree", "-r", "--name-only", args.since, "--", CHANGES_DIR + "/")
        already = {line.rsplit("/", 1)[1] for line in listed.split("\n") if line}
        frags = [f for f in frags if f.name not in already]
    sys.stdout.write(render_notes(frags))
    return 0


VERSION_RE = re.compile(r"^v?(\d+\.\d+\.\d+)$")
UNRELEASED = "## [Unreleased]"
LINK_RE = re.compile(r"^\[Unreleased\]: (.+)/compare/v(\d+\.\d+\.\d+)\.\.\.HEAD$")


def fold_text(text: str, version: str, date: str, frags: Sequence[Fragment]) -> str:
    lines = text.split("\n")
    try:
        start = next(i for i, line in enumerate(lines) if line.strip() == UNRELEASED)
    except StopIteration:
        raise FragmentError(f"CHANGELOG.md has no '{UNRELEASED}' heading") from None
    if any(line.startswith(f"## [{version}]") for line in lines):
        raise FragmentError(f"CHANGELOG.md already has a section for {version}")
    end = next((i for i in range(start + 1, len(lines))
                if lines[i].startswith("## [") or re.match(r"^\[[^\]]+\]: ", lines[i])), len(lines))

    # Text already under [Unreleased] (written by hand) is kept and merged with the fragments.
    preamble, merged, current = [], {}, None
    for line in lines[start + 1:end]:
        if line.startswith("### "):
            current = line[4:].strip()
            merged.setdefault(current, [])
        elif current is None:
            preamble.append(line)
        else:
            merged[current].append(line)
    for heading, bullets in sections(frags):
        body = merged.setdefault(heading, [])
        while body and not body[-1].strip():
            body.pop()
        body.extend(bullets)
    # Hand-written sections keep their order; sections only the fragments have follow in CHANGELOG order.
    ordered = list(merged)

    block = [f"## [{version}] - {date}", ""]
    pre = "\n".join(preamble).strip("\n")
    if pre.strip():
        block += [pre, ""]
    for heading in ordered:
        body = list(merged[heading])
        while body and not body[0].strip():
            body.pop(0)
        while body and not body[-1].strip():
            body.pop()
        block += [f"### {heading}", "", *body, ""]
    if not pre.strip() and not ordered:
        block += [NOTHING, ""]

    new = lines[:start + 1] + [""] + block + lines[end:]
    for i, line in enumerate(new):
        m = LINK_RE.match(line)
        if m:
            repo, prev = m.groups()
            new[i:i + 1] = [f"[Unreleased]: {repo}/compare/v{version}...HEAD",
                            f"[{version}]: {repo}/compare/v{prev}...v{version}"]
            break
    return "\n".join(new)


def write_atomic(path: Path, text: str) -> None:
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=path.name + ".")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            f.write(text)
        os.chmod(tmp, path.stat().st_mode & 0o777)
        os.replace(tmp, path)
    except BaseException:
        if os.path.exists(tmp):
            os.unlink(tmp)
        raise


def cmd_fold(args) -> int:
    m = VERSION_RE.match(args.version)
    if not m:
        raise FragmentError(f"{args.version!r} is not a stable version (X.Y.Z); fold is for the Stable release")
    version = m.group(1)
    date = args.date or datetime.datetime.now(datetime.timezone.utc).date().isoformat()
    if not re.match(r"^\d{4}-\d{2}-\d{2}$", date):
        raise FragmentError(f"--date {date!r} is not YYYY-MM-DD")
    try:
        datetime.date.fromisoformat(date)
    except ValueError:
        raise FragmentError(f"--date {date!r} is not a real day") from None
    repo = Path(args.repo)
    changelog = repo / args.changelog
    changes = repo / args.changes_dir
    frags = load(changes)
    try:
        text = changelog.read_text(encoding="utf-8")
    except OSError as e:
        raise FragmentError(f"cannot read {changelog}: {e.strerror}") from None
    write_atomic(changelog, fold_text(text, version, date, frags))
    for f in frags:
        (changes / f.name).unlink()
    print(f"folded {len(frags)} fragment(s) into {changelog} under {version}", file=sys.stderr)
    return 0


def main(argv: Optional[Sequence[str]] = None) -> int:
    ap = argparse.ArgumentParser(prog="fragments.py", description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)

    c = sub.add_parser("check", help="a PR that changes app code must add a fragment")
    c.add_argument("--base", required=True)
    c.add_argument("--head", required=True)
    c.add_argument("--label", action="append", help=f"a label on the PR; {NO_CHANGELOG_LABEL} skips the check")
    c.add_argument("--repo", default=".")
    c.set_defaults(fn=cmd_check)

    n = sub.add_parser("notes", help="release notes from the fragments, as Markdown")
    n.add_argument("--since", help="leave out fragments already present at this tag")
    n.add_argument("--repo", default=".")
    n.add_argument("--changes-dir", default=CHANGES_DIR)
    n.set_defaults(fn=cmd_notes)

    f = sub.add_parser("fold", help="move the fragments into CHANGELOG.md under a version")
    f.add_argument("version")
    f.add_argument("--date", help="YYYY-MM-DD, default today (UTC)")
    f.add_argument("--repo", default=".")
    f.add_argument("--changelog", default="CHANGELOG.md")
    f.add_argument("--changes-dir", default=CHANGES_DIR)
    f.set_defaults(fn=cmd_fold)

    args = ap.parse_args(argv)
    try:
        return args.fn(args)
    except FragmentError as e:
        print(f"fragments.py: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
