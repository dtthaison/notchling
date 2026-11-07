#!/usr/bin/env python3
"""Versions from tags, and the version written into a built app.

    version.py next --channel beta|stable [--from-tag vX.Y.Z-beta.N] [--allow-breaking]
                    [--repo .] [--tags-file F] [--changes-dir changes] [--info-plist legacy/macos/Info.plist]
    version.py plist <path/to/Notchling.app> <tag-or-version>

`vX.Y.Z` is a Stable tag and `vX.Y.Z-beta.N` a Beta tag; anything else is ignored. `next` prints the tag to
cut, or exits 1 with the reason on stderr. The source Info.plist is never edited: `plist` rewrites the copy
inside a built bundle.

The next Beta, from the last Stable tag S (or the Info.plist version while there is none):

* Only fragments that were not in S's tree (`git ls-tree` of `changes/` at S) count. The fold PR runs after
  the Stable tag, so a released fragment can still sit in changes/ and must not bump the version again.
  With no Stable tag every fragment counts. If S's tree cannot be listed the run fails; it never falls back
  to counting everything.
* Their kinds pick the line: `breaking` X+1.0.0, else `feature` X.Y+1.0, else X.Y.Z+1.
* A Beta's build number must rise: when that line is below the newest Beta above S (a feature fragment
  reverted, an older Beta promoted), the newest Beta's line is continued instead.
* A `breaking` fragment needs `--allow-breaking` until a Beta of line X+1.0.0 exists; after that the line
  goes on without it.

`next --channel stable` promotes a Beta and never looks at fragments: clicking Promote is the approval.
"""

from __future__ import annotations

import argparse
import os
import plistlib
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import List, Optional, Sequence, Tuple

import fragments

# (major, minor, patch, beta number or None for a Stable)
Version = Tuple[int, int, int, Optional[int]]

NUM = r"(0|[1-9][0-9]*)"
TAG_RE = re.compile(rf"^v{NUM}\.{NUM}\.{NUM}(?:-beta\.([1-9][0-9]*))?$")
DEFAULT_INFO_PLIST = Path(__file__).resolve().parents[2] / "legacy" / "macos" / "Info.plist"
MAX_BETA = 98
MAX_PART = 99


class VersionError(Exception):
    pass


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
    """CFBundleVersion: X*1000000 + Y*10000 + Z*100 + N for a Beta, + 99 for a Stable.

    Sparkle compares it as a number, so a Stable sorts after every Beta of it and betas sort by N. Minor and
    patch stop at 99 and a beta number at 98, or two versions could collide or invert."""
    x, y, z, n = v
    if y > MAX_PART or z > MAX_PART:
        raise ValueError(f"minor and patch stop at {MAX_PART} ({format_tag(v)})")
    if n is not None and not 1 <= n <= MAX_BETA:
        raise ValueError(f"a beta number must be 1 to {MAX_BETA} ({format_tag(v)})")
    return x * 1000000 + y * 10000 + z * 100 + (99 if n is None else n)


def read_tags(tags_file: Optional[str], repo: str = ".") -> List[str]:
    if tags_file:
        try:
            text = Path(tags_file).read_text(encoding="utf-8")
        except OSError as e:
            raise VersionError(f"cannot read --tags-file {tags_file}: {e.strerror}") from None
    else:
        r = subprocess.run(["git", "-C", repo, "tag", "--list", "v*"], capture_output=True, text=True)
        if r.returncode != 0:
            raise VersionError(f"git tag --list 'v*': {r.stderr.strip() or 'failed'}")
        text = r.stdout
    return [line.strip() for line in text.splitlines() if line.strip()]


def plist_stable_version(info_plist: Path) -> Version:
    try:
        short = plistlib.loads(info_plist.read_bytes())["CFBundleShortVersionString"]
    except (OSError, ValueError, KeyError, plistlib.InvalidFileException) as e:
        raise VersionError(f"no Stable tag yet, and {info_plist} gives no version ({type(e).__name__})") from None
    v = parse_tag("v" + str(short))
    if v is None or v[3] is not None:
        raise VersionError(f"no Stable tag yet, and CFBundleShortVersionString {short!r} in {info_plist} "
                           f"is not X.Y.Z")
    return v


def last_stable(tags: Sequence[str]) -> Optional[Version]:
    return max((v for v in map(parse_tag, tags) if v and v[3] is None), default=None)


def new_fragment_kinds(tags: Sequence[str], repo: str, changes_dir: str) -> set:
    """Kinds of the fragments in changes/ that the last Stable tag did not already contain."""
    stable = last_stable(tags)
    released = set()
    if stable is not None:
        tag = format_tag(stable)
        try:
            released = fragments.names_at(repo, f"refs/tags/{tag}", changes_dir)
        except fragments.FragmentError as e:
            raise VersionError(f"cannot list {changes_dir}/ at the last Stable tag {tag} "
                               f"(are the tags fetched?): {e}") from None
    return {f.kind for f in fragments.load(Path(repo) / changes_dir, skip=released)}


def next_tag(channel: str, tags: Sequence[str], kinds: set, allow_breaking: bool,
             from_tag: Optional[str], info_plist: Path) -> Version:
    """`kinds` are the kinds of the fragments the last Stable tag did not contain (new_fragment_kinds)."""
    parsed = [v for v in (parse_tag(t) for t in tags) if v]
    stables = sorted(v for v in parsed if v[3] is None)
    betas = [v for v in parsed if v[3] is not None]

    given = None
    if from_tag is not None:
        given = parse_tag(from_tag)
        if given is None or given[3] is None:
            raise VersionError(f"--from-tag {from_tag!r} is not a Beta tag (vX.Y.Z-beta.N)")
        if given not in parsed:
            raise VersionError(f"--from-tag {from_tag} is not among the tags")

    if channel == "stable":
        if given is None:
            later = [b for b in betas if not stables or b[:3] > stables[-1][:3]]
            if not later:
                raise VersionError("no Beta newer than the last Stable to promote")
            given = max(later)
        target = given[:3] + (None,)
        if target in stables:
            raise VersionError(f"{format_tag(target)} is already released")
        if stables and target[:3] < stables[-1][:3]:
            raise VersionError(f"{format_tag(target)} is older than the last Stable {format_tag(stables[-1])}")
        return target

    base = stables[-1] if stables else plist_stable_version(info_plist)
    x, y, z, _ = base
    major = (x + 1, 0, 0)
    above = [b for b in betas if b[:3] > base[:3]]  # Betas still ahead of the last Stable
    newest = max(above) if above else None
    if "breaking" in kinds and not allow_breaking and not any(b[:3] == major for b in above):
        raise VersionError("changes/ holds a new 'breaking' fragment and no Beta of "
                           f"{format_tag(major + (None,))} exists yet: starting a major line needs the maintainer; "
                           "pass --allow-breaking once they approve")

    if given is not None:
        line = given[:3]
        if line <= base[:3]:
            raise VersionError(f"--from-tag {from_tag}: {format_tag(line + (None,))} is not newer than the last Stable "
                               f"{format_tag(base)}, so a Beta of it would reach nobody")
        if newest is not None and line < newest[:3]:
            raise VersionError(f"--from-tag {from_tag} would cut a Beta below {format_tag(newest)}, and a "
                               f"Beta's build number must rise; continue from {format_tag(newest)} instead")
    else:
        if "breaking" in kinds:
            line = major
        elif "feature" in kinds:
            line = (x, y + 1, 0)
        else:
            line = (x, y, z + 1)
        if newest is not None and line < newest[:3]:
            line = newest[:3]  # the fragments went back (reverted, or an older Beta was promoted)
    n = max([b[3] for b in betas if b[:3] == line] + [0]) + 1
    return line + (n,)


def cmd_next(args) -> int:
    tags = read_tags(args.tags_file, args.repo)
    # A Stable never looks at fragments: the maintainer's click on Promote is the approval.
    kinds = new_fragment_kinds(tags, args.repo, args.changes_dir) if args.channel == "beta" else set()
    v = next_tag(args.channel, tags, kinds, args.allow_breaking, args.from_tag, Path(args.info_plist))
    try:
        build_number(v)
    except ValueError as e:
        raise VersionError(str(e)) from None
    print(format_tag(v))
    return 0


def cmd_plist(args) -> int:
    v = parse_tag(args.version if args.version.startswith("v") else "v" + args.version)
    if v is None:
        raise VersionError(f"{args.version!r} is not X.Y.Z or X.Y.Z-beta.N")
    try:
        number = build_number(v)
    except ValueError as e:
        raise VersionError(str(e)) from None
    path = Path(args.app) / "Contents" / "Info.plist"
    try:
        data = path.read_bytes()
    except OSError as e:
        raise VersionError(f"cannot read {path}: {e.strerror}") from None
    try:
        info = plistlib.loads(data)
    except (ValueError, plistlib.InvalidFileException) as e:
        raise VersionError(f"{path} is not a property list: {e}") from None
    info["CFBundleShortVersionString"] = format_tag(v)[1:]
    info["CFBundleVersion"] = str(number)
    fmt = plistlib.FMT_BINARY if data.startswith(b"bplist") else plistlib.FMT_XML
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix="Info.plist.")
    try:
        with os.fdopen(fd, "wb") as f:
            plistlib.dump(info, f, fmt=fmt, sort_keys=False)
        os.chmod(tmp, path.stat().st_mode & 0o777)
        os.replace(tmp, path)
    except BaseException:
        if os.path.exists(tmp):
            os.unlink(tmp)
        raise
    print(f"{path}: {info['CFBundleShortVersionString']} ({info['CFBundleVersion']})", file=sys.stderr)
    return 0


def main(argv: Optional[Sequence[str]] = None) -> int:
    ap = argparse.ArgumentParser(prog="version.py", description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)

    n = sub.add_parser("next", help="print the next tag to cut")
    n.add_argument("--channel", required=True, choices=("beta", "stable"))
    n.add_argument("--from-tag", help="a Beta tag: bump its N (beta) or promote it (stable)")
    n.add_argument("--allow-breaking", action="store_true", help="the maintainer approved a breaking change")
    n.add_argument("--tags-file", help="tags, one per line, instead of `git tag --list 'v*'`")
    n.add_argument("--repo", default=".")
    n.add_argument("--changes-dir", default=fragments.CHANGES_DIR)
    n.add_argument("--info-plist", default=str(DEFAULT_INFO_PLIST),
                   help="read for the last Stable only while no Stable tag exists")
    n.set_defaults(fn=cmd_next)

    p = sub.add_parser("plist", help="write the version into a built bundle")
    p.add_argument("app")
    p.add_argument("version", help="vX.Y.Z, vX.Y.Z-beta.N or the same without the v")
    p.set_defaults(fn=cmd_plist)

    args = ap.parse_args(argv)
    try:
        return args.fn(args)
    except (VersionError, fragments.FragmentError) as e:
        print(f"version.py: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
