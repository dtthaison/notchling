from __future__ import annotations

import os
import subprocess
import tempfile
from pathlib import Path
from typing import Dict, Optional, Union

ENV = {
    **os.environ,
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com",
    "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull,
}


def git(repo: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True,
                          text=True, env=ENV).stdout.strip()


def write(repo: Path, files: Dict[str, Optional[Union[str, bytes]]]) -> None:
    for name, content in files.items():
        p = repo / name
        if content is None:
            git(repo, "rm", "-q", name)
            continue
        p.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, bytes):
            p.write_bytes(content)
        else:
            p.write_text(content, encoding="utf-8")
        git(repo, "add", name)


def make_repo(files: Dict[str, Union[str, bytes]]) -> Path:
    repo = Path(tempfile.mkdtemp(prefix="guard-test-"))
    git(repo, "init", "-q", "-b", "main")
    write(repo, files)
    git(repo, "commit", "-q", "--allow-empty", "-m", "base")
    return repo


def commit(repo: Path, files: Dict[str, Optional[Union[str, bytes]]], message: str = "change") -> str:
    write(repo, files)
    git(repo, "commit", "-q", "--allow-empty", "-m", message)
    return head_of(repo)


def move(repo: Path, old: str, new: str, message: str = "move") -> str:
    (repo / new).parent.mkdir(parents=True, exist_ok=True)
    git(repo, "mv", old, new)
    git(repo, "commit", "-q", "-m", message)
    return head_of(repo)


def head_of(repo: Path) -> str:
    return git(repo, "rev-parse", "HEAD")
