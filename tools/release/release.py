#!/usr/bin/env python3
"""Starts a release of the Rust app: tools/release/release.py 2.0.0

    release.py <X.Y.Z> [--dry-run] [--base origin/main] [--date YYYY-MM-DD] [--push] [--open-pr]
               [--no-fetch] [--repo .]

Makes branch release/<version> from the base with `[workspace.package] version` in Cargo.toml and the
matching version of each path package in Cargo.lock set to <version>, changes/*.md folded into CHANGELOG.md
under `## [<version>] - <date>` (the fragments are deleted), and one commit "Notchling <version>". It
pushes only with --push and opens the pull request only with --open-pr (which implies --push). Merging
that pull request tags v<version> and publishes the release. --dry-run fetches nothing and writes nothing:
it prints the version change, the Cargo.lock entries, the fragments and the new changelog section.
"""

from __future__ import annotations

import argparse
import datetime
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import List, Optional, Sequence, Tuple

import fragments

VERSION_RE = re.compile(r"^(\d+)\.(\d+)\.(\d+)$")
VERSION_LINE_RE = re.compile(r'^(\s*version\s*=\s*")([^"]*)(")')
CARGO_TOML = "Cargo.toml"
CARGO_LOCK = "Cargo.lock"
CHANGELOG = "CHANGELOG.md"
CHANGES_DIR = fragments.CHANGES_DIR


class ReleaseError(Exception):
    pass


def run(argv: Sequence[str], cwd, check: bool = True) -> subprocess.CompletedProcess:
    env = dict(os.environ, LC_ALL="C", GIT_TERMINAL_PROMPT="0")
    try:
        r = subprocess.run(list(argv), cwd=str(cwd), capture_output=True, text=True, env=env)
    except OSError as e:
        raise ReleaseError(f"cannot run {argv[0]}: {e.strerror}") from None
    if check and r.returncode != 0:
        raise ReleaseError(f"{' '.join(argv[:3])}: {r.stderr.strip() or 'failed'}")
    return r


def git(repo, *args, check: bool = True) -> str:
    return run(["git", *args], repo, check).stdout


def parse_version(text: str) -> Tuple[int, int, int]:
    m = VERSION_RE.match(text)
    if not m:
        raise ReleaseError(f"{text!r} is not X.Y.Z")
    return int(m.group(1)), int(m.group(2)), int(m.group(3))


def workspace_version(cargo_toml: str) -> str:
    in_section = False
    for line in cargo_toml.split("\n"):
        stripped = line.strip()
        if stripped.startswith("["):
            in_section = stripped == "[workspace.package]"
            continue
        if in_section:
            m = VERSION_LINE_RE.match(line)
            if m:
                return m.group(2)
    raise ReleaseError(f"{CARGO_TOML} has no version in [workspace.package]")


def bump_cargo_toml(text: str, new: str) -> str:
    lines = text.split("\n")
    in_section = False
    for i, line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith("["):
            in_section = stripped == "[workspace.package]"
            continue
        if in_section:
            m = VERSION_LINE_RE.match(line)
            if m:
                lines[i] = m.group(1) + new + m.group(3) + line[m.end():]
                return "\n".join(lines)
    raise ReleaseError(f"{CARGO_TOML} has no version in [workspace.package]")


def bump_cargo_lock(text: str, old: str, new: str) -> Tuple[str, List[str]]:
    lines = text.splitlines(keepends=True)
    changed: List[str] = []
    i = 0
    while i < len(lines):
        if lines[i].rstrip("\r\n") != "[[package]]":
            i += 1
            continue
        end = i + 1
        while end < len(lines) and lines[end].strip() != "" and not lines[end].startswith("["):
            end += 1
        name = None
        version_at = None
        has_source = False
        for j in range(i + 1, end):
            key = lines[j].split("=", 1)[0].strip()
            if key == "name":
                name = lines[j].split("=", 1)[1].strip().strip('"')
            elif key == "version":
                version_at = j
            elif key == "source":
                has_source = True
        if version_at is not None and not has_source and name is not None:
            m = VERSION_LINE_RE.match(lines[version_at])
            if m and m.group(2) == old:
                lines[version_at] = m.group(1) + new + m.group(3) + lines[version_at][m.end():]
                changed.append(name)
        i = end
    return "".join(lines), changed


def check_date(date: str) -> str:
    if not re.match(r"^\d{4}-\d{2}-\d{2}$", date):
        raise ReleaseError(f"--date {date!r} is not YYYY-MM-DD")
    try:
        datetime.date.fromisoformat(date)
    except ValueError:
        raise ReleaseError(f"--date {date!r} is not a real day") from None
    return date


def today() -> str:
    return datetime.datetime.now(datetime.timezone.utc).date().isoformat()


def changelog_section(text: str, version: str) -> str:
    lines = text.split("\n")
    start = next((i for i, line in enumerate(lines) if line.startswith(f"## [{version}]")), None)
    if start is None:
        return ""
    end = next((i for i in range(start + 1, len(lines))
                if lines[i].startswith("## [") or re.match(r"^\[[^\]]+\]: ", lines[i])), len(lines))
    return "\n".join(lines[start:end]).rstrip("\n") + "\n"


class Plan:
    def __init__(self, repo: Path, version: str, date: str):
        self.repo = repo
        self.version = version
        self.date = date
        toml_path = repo / CARGO_TOML
        lock_path = repo / CARGO_LOCK
        log_path = repo / CHANGELOG
        try:
            self.toml_old = toml_path.read_text(encoding="utf-8")
            self.lock_old = lock_path.read_text(encoding="utf-8")
            self.log_old = log_path.read_text(encoding="utf-8")
        except OSError as e:
            raise ReleaseError(f"cannot read {e.filename}: {e.strerror}") from None
        self.old_version = workspace_version(self.toml_old)
        self.frags = fragments.load(repo / CHANGES_DIR)
        self.toml_new = bump_cargo_toml(self.toml_old, version)
        self.lock_new, self.lock_names = bump_cargo_lock(self.lock_old, self.old_version, version)
        self.log_new = fragments.fold_text(self.log_old, version, date, self.frags)

    def check_newer(self) -> None:
        if parse_version(self.version) <= parse_version_or_refuse(self.old_version):
            raise ReleaseError(f"{self.version} isn't newer than {self.old_version}")
        if not self.lock_names:
            raise ReleaseError(f"{CARGO_LOCK} has no path package at {self.old_version}")

    def apply(self) -> List[str]:
        fragments.write_atomic(self.repo / CARGO_TOML, self.toml_new)
        fragments.write_atomic(self.repo / CARGO_LOCK, self.lock_new)
        fragments.write_atomic(self.repo / CHANGELOG, self.log_new)
        paths = [CARGO_TOML, CARGO_LOCK, CHANGELOG]
        for f in self.frags:
            (self.repo / CHANGES_DIR / f.name).unlink()
            paths.append(f"{CHANGES_DIR}/{f.name}")
        return paths


def parse_version_or_refuse(text: str) -> Tuple[int, int, int]:
    try:
        return parse_version(text)
    except ReleaseError:
        raise ReleaseError(f"the version in {CARGO_TOML} ({text!r}) is not X.Y.Z") from None


def describe(plan: Plan) -> str:
    out = [f"{CARGO_TOML}: [workspace.package] version {plan.old_version} -> {plan.version}",
           f"{CARGO_LOCK}: {len(plan.lock_names)} package(s) {plan.old_version} -> {plan.version}: "
           + ", ".join(plan.lock_names)]
    if plan.frags:
        out.append(f"{CHANGES_DIR}/: {len(plan.frags)} fragment(s) folded and deleted:")
        out += [f"  {f.name}" for f in plan.frags]
    else:
        out.append(f"{CHANGES_DIR}/: no fragments")
    out += ["", f"{CHANGELOG} would get:", "", changelog_section(plan.log_new, plan.version).rstrip("\n")]
    return "\n".join(out)


def pr_body(version: str) -> str:
    return (f"Bumps the version to {version} and moves the changelog entries under it. "
            f"Merging tags v{version} and publishes the release.")


def current_ref(repo) -> Tuple[str, bool]:
    name = git(repo, "rev-parse", "--abbrev-ref", "HEAD").strip()
    if name and name != "HEAD":
        return name, True
    return git(repo, "rev-parse", "HEAD").strip(), False


def rollback(repo, branch: str, origin: str, on_branch: bool) -> None:
    run(["git", "reset", "-q", "--hard", "HEAD"], repo, check=False)
    args = ["git", "switch", "-q"] + ([origin] if on_branch else ["--detach", origin])
    run(args, repo, check=False)
    run(["git", "branch", "-q", "-D", branch], repo, check=False)


def refuse_early(repo: Path, version: str, dry_run: bool) -> None:
    branch = f"release/{version}"
    if not dry_run and git(repo, "status", "--porcelain").strip():
        raise ReleaseError("Commit or stash your changes first.")
    if run(["git", "rev-parse", "-q", "--verify", f"refs/heads/{branch}"], repo, check=False).returncode == 0:
        raise ReleaseError(f"branch {branch} already exists")


def start(args) -> int:
    repo = Path(args.repo).resolve()
    version = args.version
    parse_version(version)
    date = check_date(args.date) if args.date else today()
    branch = f"release/{version}"
    tag = f"refs/tags/v{version}"

    refuse_early(repo, version, args.dry_run)

    if args.dry_run:
        if run(["git", "rev-parse", "-q", "--verify", tag], repo, check=False).returncode == 0:
            raise ReleaseError(f"v{version} is already tagged")
        plan = Plan(repo, version, date)
        plan.check_newer()
        print(describe(plan))
        print("\nDry run: nothing was fetched, switched or written.", file=sys.stderr)
        return 0

    if not args.no_fetch:
        git(repo, "fetch", "-q", "origin", "main", "--tags")
    if run(["git", "rev-parse", "-q", "--verify", tag], repo, check=False).returncode == 0:
        raise ReleaseError(f"v{version} is already tagged")
    if run(["git", "rev-parse", "-q", "--verify", f"{args.base}^{{commit}}"], repo, check=False).returncode != 0:
        raise ReleaseError(f"base {args.base} does not exist")
    base_toml = git(repo, "show", f"{args.base}:{CARGO_TOML}")
    base_version = workspace_version(base_toml)
    if parse_version(version) <= parse_version_or_refuse(base_version):
        raise ReleaseError(f"{version} isn't newer than {base_version}")

    origin, on_branch = current_ref(repo)
    git(repo, "switch", "-q", "-c", branch, args.base)
    try:
        plan = Plan(repo, version, date)
        plan.check_newer()
        paths = plan.apply()
        git(repo, "add", "--", *paths)
        git(repo, "commit", "-q", "-m", f"Notchling {version}")
    except BaseException:
        rollback(repo, branch, origin, on_branch)
        raise

    print(f"Made branch {branch} with one commit, Notchling {version}.")
    print(describe(plan))
    if args.push or args.open_pr:
        git(repo, "push", "-q", "-u", "origin", branch)
        print(f"\nPushed {branch} to origin.")
    if args.open_pr:
        r = run(["gh", "pr", "create", "--base", "main", "--head", branch,
                 "--title", f"Notchling {version}", "--body", pr_body(version)], repo)
        print(r.stdout.strip())
    else:
        print("\nNext:")
        if not (args.push or args.open_pr):
            print(f"  git push -u origin {branch}")
        print(f"  gh pr create --base main --head {branch} --title \"Notchling {version}\"")
        print(f"Merging that pull request tags v{version} and publishes the release.")
    return 0


def main(argv: Optional[Sequence[str]] = None) -> int:
    ap = argparse.ArgumentParser(prog="release.py", description=__doc__.split("\n")[0])
    ap.add_argument("version", help="X.Y.Z")
    ap.add_argument("--dry-run", action="store_true", help="print what would change; fetch and write nothing")
    ap.add_argument("--base", default="origin/main", help="the ref the release branch starts from")
    ap.add_argument("--date", help="YYYY-MM-DD, default today (UTC)")
    ap.add_argument("--push", action="store_true", help="push the branch to origin")
    ap.add_argument("--open-pr", action="store_true", help="push, then open the pull request with gh")
    ap.add_argument("--no-fetch", action="store_true", help="do not run git fetch origin main --tags")
    ap.add_argument("--repo", default=".")
    args = ap.parse_args(argv)
    try:
        return start(args)
    except (ReleaseError, fragments.FragmentError) as e:
        print(f"release.py: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
