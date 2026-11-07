#!/usr/bin/env python3
"""The push wrapper: takes an agent's finished branch, pushes it as the GitHub App and opens the PR.

    push_wrapper.py --worktree PATH --issue N --title TEXT --body-file FILE [--agent-model ID] [--config FILE]

Run by sd as the wrapper user, never by the agent. The agent's OS user has no GitHub credential; the App's
key lives in a file only the wrapper user can read. What this script promises:

- It pushes only `refs/heads/agent/<issue>-<slug>` (lowercase slug, the issue number matching), as one
  explicit `<sha>:<ref>` refspec: never a force, a tag, a delete or a glob. A branch that exists with other
  history is rejected by the remote, not overwritten.
- The installation token is narrowed to this one repository (`contents` and `pull_requests` write; GitHub
  makes it good for an hour), lives in memory, and reaches git only through GIT_CONFIG_* environment
  variables. It is in no argument list and no file.
- The worktree belongs to the agent, so nothing in its git config, hooks or attributes may run as the
  wrapper. The only git commands run there are `rev-parse` and `symbolic-ref`, which execute nothing. The
  clean check and the push run in a scratch repository of ours that borrows the agent's objects through
  `objects/info/alternates`.

Exit codes: 0 done (prints the PR URL), 1 refused, 2 error. Config (JSON, default
~/.config/notchling/push-wrapper.json): app_id, key_path, repo, and optionally api_url and remote_url."""

from __future__ import annotations

import argparse
import base64
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time
import unicodedata
import urllib.parse
from typing import Any, Callable, List, Mapping, NamedTuple, Optional, TextIO

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from preflight import API_URL, REPO_RE, Api, ApiError  # noqa: E402

BRANCH_RE = re.compile(r"agent/([1-9][0-9]*)-([a-z0-9]+(?:-[a-z0-9]+)*)")
MODEL_RE = re.compile(r"[A-Za-z0-9._:/-]{1,64}")
SHA_RE = re.compile(r"[0-9a-f]{40}|[0-9a-f]{64}")
BRANCH_NAME_RE = re.compile(r"[A-Za-z0-9._/-]{1,100}")
SLUG_MAX = 60
TITLE_MAX = 256
BODY_MAX = 60000
GIT_TIMEOUT = 300
PERMISSIONS = {"contents": "write", "pull_requests": "write"}
DEFAULT_CONFIG = "~/.config/notchling/push-wrapper.json"
PASS_ENV = ("PATH", "HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "NO_PROXY", "no_proxy",
            "SSL_CERT_FILE", "SSL_CERT_DIR", "GIT_SSL_CAINFO")


class Refused(Exception):
    """The request breaks a rule. Nothing was sent anywhere."""


class Failed(Exception):
    """Something outside our control went wrong (GitHub, git, openssl)."""


class Result(NamedTuple):
    code: int
    message: str


class Config(NamedTuple):
    app_id: str
    key_path: str
    repo: str
    api_url: str = API_URL
    remote_url: str = ""


class Request(NamedTuple):
    worktree: str
    issue: int
    title: str
    body_file: str
    agent_model: Optional[str] = None


class Runner:
    """Runs a command and returns the CompletedProcess. A seam: tests wrap it to read every argv."""

    def __call__(self, argv: List[str], env: Optional[Mapping[str, str]] = None,
                 input: Optional[bytes] = None, cwd: Optional[str] = None) -> "subprocess.CompletedProcess[bytes]":
        try:
            return subprocess.run(argv, env=None if env is None else dict(env), input=input, cwd=cwd,
                                  capture_output=True, timeout=GIT_TIMEOUT)
        except (OSError, subprocess.TimeoutExpired) as e:
            raise Failed("%s could not run (%s)" % (argv[0], type(e).__name__))


# --- rules -------------------------------------------------------------------------------------------

def validate_branch(branch: str, issue: int) -> str:
    m = BRANCH_RE.fullmatch(branch or "")
    if not m or int(m.group(1)) != issue or len(m.group(2)) > SLUG_MAX:
        raise Refused("the branch must be agent/%d-<lowercase-slug>" % issue)
    return branch


def validate_ref(ref: str) -> str:
    """The one place a ref is let through: refs/heads/agent/<n>-<slug>, nothing else."""
    prefix = "refs/heads/"
    m = BRANCH_RE.fullmatch(ref[len(prefix):]) if ref.startswith(prefix) else None
    if not m or len(m.group(2)) > SLUG_MAX:
        raise Refused("only refs/heads/agent/<issue>-<slug> may be pushed")
    return ref


def _plain(text: str) -> bool:
    return all(unicodedata.category(c) not in ("Cc", "Cf", "Zl", "Zp") for c in text)


def validate_title(title: str) -> str:
    if not isinstance(title, str) or not title.strip() or len(title) > TITLE_MAX or not _plain(title):
        raise Refused("the title must be one plain line of 1 to %d characters" % TITLE_MAX)
    return title.strip()


def validate_model(model: Optional[str]) -> Optional[str]:
    if model is None:
        return None
    if not MODEL_RE.fullmatch(model):
        raise Refused("the model id is not a plain id")
    return model


def read_body(path: str) -> str:
    try:
        with open(path, "rb") as f:
            raw = f.read(BODY_MAX + 1)
        if len(raw) > BODY_MAX:
            raise Refused("the body file is over %d bytes" % BODY_MAX)
        text = raw.decode("utf-8")
    except (OSError, UnicodeDecodeError):
        raise Refused("the body file could not be read as text")
    if "\x00" in text:
        raise Refused("the body file is not text")
    return text


def check_key(path: str) -> None:
    """The App key must be a regular file, ours, and unreadable by anyone else."""
    try:
        st = os.lstat(path)
    except OSError:
        raise Refused("the App key file is missing")
    if not stat.S_ISREG(st.st_mode):
        raise Refused("the App key path is not a regular file")
    if st.st_mode & 0o077:
        raise Refused("the App key file must not be readable by group or others (mode 0600)")
    if st.st_uid != os.geteuid():
        raise Refused("the App key file is not owned by the user running the wrapper")


# --- JWT and tokens ----------------------------------------------------------------------------------

def _b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def make_jwt(app_id: str, key_path: str, now: int, runner: Runner) -> str:
    header = _b64url(json.dumps({"alg": "RS256", "typ": "JWT"}, separators=(",", ":")).encode())
    iss: Any = int(app_id) if str(app_id).isdigit() else str(app_id)
    payload = _b64url(json.dumps({"iat": now - 60, "exp": now + 540, "iss": iss},
                                 separators=(",", ":")).encode())
    signing_input = ("%s.%s" % (header, payload)).encode()
    proc = runner(["openssl", "dgst", "-sha256", "-sign", key_path], env={"PATH": os.environ.get("PATH", "")},
                  input=signing_input)
    if proc.returncode != 0 or not proc.stdout:
        raise Failed("openssl could not sign with the App key")
    return "%s.%s.%s" % (header, payload, _b64url(proc.stdout))


def installation_token(api: Api, repo: str, jwt: str) -> str:
    inst = api.request("GET", "/repos/%s/installation" % repo, token=jwt)
    inst_id = inst.get("id") if isinstance(inst, dict) else None
    if not isinstance(inst_id, int):
        raise Failed("the App is not installed on %s" % repo)
    out = api.request("POST", "/app/installations/%d/access_tokens" % inst_id,
                      {"repositories": [repo.split("/")[1]], "permissions": PERMISSIONS}, token=jwt)
    token = out.get("token") if isinstance(out, dict) else None
    if not isinstance(token, str) or not token:
        raise Failed("GitHub gave no installation token")
    return token


# --- git ---------------------------------------------------------------------------------------------

def git_env(token: Optional[str], remote_url: str, scratch: str, global_config: str = os.devnull) -> dict:
    """A bare-bones environment for git. The token, if any, goes in as a per-URL extraHeader through
    GIT_CONFIG_*, so it is never in an argument list or a config file."""
    env = {k: os.environ[k] for k in PASS_ENV if k in os.environ}
    env.update({"HOME": scratch, "LANG": "C", "LC_ALL": "C", "GIT_TERMINAL_PROMPT": "0",
                "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": global_config, "GIT_ASKPASS": "true"})
    parts = urllib.parse.urlsplit(remote_url)
    if token and parts.scheme in ("http", "https") and parts.netloc:
        basic = base64.b64encode(("x-access-token:" + token).encode()).decode()
        env.update({"GIT_CONFIG_COUNT": "1",
                    "GIT_CONFIG_KEY_0": "http.%s://%s/.extraheader" % (parts.scheme, parts.netloc),
                    "GIT_CONFIG_VALUE_0": "AUTHORIZATION: basic " + basic})
    return env


def _git(runner: Runner, args: List[str], env: Mapping[str, str], what: str, secret: str = "") -> bytes:
    proc = runner(["git"] + args, env=env)
    if proc.returncode != 0:
        tail = proc.stderr.decode("utf-8", "replace").strip().splitlines()[-1:] or [""]
        text = tail[0][:300]
        if secret:
            text = text.replace(secret, "[token]")
        raise Failed("%s failed (exit %d)%s" % (what, proc.returncode, ": " + text if text else ""))
    return proc.stdout


def inspect_worktree(runner: Runner, worktree: str, issue: int, scratch: str) -> tuple:
    """Returns (branch, sha, scratch_git_dir) after checking the branch and that the tree is clean."""
    wt = os.path.realpath(worktree)
    if not os.path.isdir(wt) or not os.path.exists(os.path.join(wt, ".git")):
        raise Refused("the worktree is not a git worktree")
    # git honours safe.directory only from global or system config, never from -c, and the agent's
    # directories belong to another user. So the scratch dir carries a global config that allows them.
    allow = os.path.join(scratch, "gitconfig")
    with open(allow, "w") as f:
        f.write("[safe]\n\tdirectory = *\n")
    env = git_env(None, "", scratch, global_config=allow)
    safe = ["-c", "core.fsmonitor=false", "-C", wt]
    # rev-parse and symbolic-ref only read refs and the repository layout; they run no configured command.
    try:
        git_dir = _git(runner, safe + ["rev-parse", "--path-format=absolute", "--absolute-git-dir"],
                       env, "reading the worktree").decode().strip()
        common = _git(runner, safe + ["rev-parse", "--path-format=absolute", "--git-common-dir"],
                      env, "reading the worktree").decode().strip()
        branch = _git(runner, safe + ["symbolic-ref", "-q", "--short", "HEAD"], env,
                      "reading the branch").decode().strip()
    except Failed:
        raise Refused("the worktree has no readable branch")
    validate_branch(branch, issue)
    sha = _git(runner, safe + ["rev-parse", "--verify", "-q", "HEAD^{commit}"], env,
               "reading the commit").decode().strip()
    if not SHA_RE.fullmatch(sha) or "\n" in common or not os.path.isdir(os.path.join(common, "objects")):
        raise Refused("the worktree's commit could not be read")

    sgit = os.path.join(scratch, "s.git")
    _git(runner, ["init", "-q", "--bare", "--template=", sgit], env, "preparing the scratch repository")
    with open(os.path.join(sgit, "objects", "info", "alternates"), "w") as f:
        f.write(os.path.join(common, "objects") + "\n")
    base = ["--git-dir", sgit]
    _git(runner, base + ["update-ref", validate_ref("refs/heads/" + branch), sha], env, "staging the commit")
    _git(runner, base + ["symbolic-ref", "HEAD", "refs/heads/" + branch], env, "staging the commit")
    try:
        shutil.copyfile(os.path.join(git_dir, "index"), os.path.join(sgit, "index"))
    except OSError:
        raise Refused("the worktree has no index, so it is not clean")
    status = _git(runner, base + ["--work-tree", wt, "status", "--porcelain",
                                                              "--untracked-files=all", "--ignore-submodules=all"],
                  env, "checking the worktree")
    if status.strip():
        raise Refused("the worktree is not clean: commit or remove what is left")
    return branch, sha, sgit


# --- the whole job -----------------------------------------------------------------------------------

def push_and_open_pr(req: Request, cfg: Config, runner: Optional[Runner] = None,
                     clock: Callable[[], float] = time.time, tmp_root: Optional[str] = None) -> Result:
    runner = runner or Runner()
    scratch = None
    token = ""
    try:
        if req.issue < 1:
            raise Refused("the issue number must be positive")
        title = validate_title(req.title)
        model = validate_model(req.agent_model)
        body = read_body(req.body_file)
        check_key(cfg.key_path)
        scratch = tempfile.mkdtemp(prefix="push-wrapper-", dir=tmp_root)
        branch, sha, sgit = inspect_worktree(runner, req.worktree, req.issue, scratch)
        ref = validate_ref("refs/heads/" + branch)

        api = Api(cfg.api_url)
        jwt = make_jwt(cfg.app_id, cfg.key_path, int(clock()), runner)
        token = installation_token(api, cfg.repo, jwt)
        info = api.request("GET", "/repos/%s" % cfg.repo, token=token)
        base = info.get("default_branch") if isinstance(info, dict) else None
        if not isinstance(base, str) or not BRANCH_NAME_RE.fullmatch(base):
            raise Failed("the default branch could not be read")

        env = git_env(token, cfg.remote_url, scratch)
        _git(runner, ["--git-dir", sgit, "-c", "push.default=nothing", "push", cfg.remote_url,
                      "%s:%s" % (sha, ref)], env, "git push", secret=token)
        try:
            return Result(0, open_pr(api, cfg.repo, token, branch, base, title, body, req.issue, model))
        except (ApiError, Failed) as e:
            return Result(2, "the branch is pushed, but the pull request failed: %s" % e)
    except Refused as e:
        return Result(1, _scrub(str(e), token))
    except (Failed, ApiError) as e:
        return Result(2, _scrub(str(e), token))
    finally:
        if scratch:
            shutil.rmtree(scratch, ignore_errors=True)


def _scrub(text: str, token: str) -> str:
    return text.replace(token, "[token]") if token else text


def open_pr(api: Api, repo: str, token: str, branch: str, base: str, title: str, body: str, issue: int,
            model: Optional[str]) -> str:
    text = body.rstrip() + "\n\nRefs #%d\n" % issue
    if model:
        text += "Agent-Model: %s\n" % model
    try:
        pr = api.request("POST", "/repos/%s/pulls" % repo,
                         {"title": title, "head": branch, "base": base, "body": text}, token=token)
    except ApiError as e:
        if e.status != 422:
            raise
        # Usually "a pull request already exists" for this branch (a second push after a fix).
        owner = repo.split("/")[0]
        found = api.request("GET", "/repos/%s/pulls?head=%s&state=open&per_page=1"
                            % (repo, urllib.parse.quote("%s:%s" % (owner, branch), safe="")), token=token)
        if not isinstance(found, list) or not found:
            raise
        pr = found[0]
    url = pr.get("html_url") if isinstance(pr, dict) else None
    if not isinstance(url, str) or not url:
        raise Failed("GitHub gave no pull request URL")
    return url


def load_config(path: str) -> Config:
    try:
        with open(path) as f:
            raw = json.load(f)
    except OSError:
        raise Failed("the config file could not be read")
    except ValueError:
        raise Refused("the config file is not valid JSON")
    if not isinstance(raw, dict):
        raise Refused("the config file must be a JSON object")
    app_id, key_path, repo = raw.get("app_id"), raw.get("key_path"), raw.get("repo")
    if isinstance(app_id, int) and not isinstance(app_id, bool):
        app_id = str(app_id)
    if not isinstance(app_id, str) or not re.fullmatch(r"[A-Za-z0-9._-]{1,64}", app_id):
        raise Refused("config: app_id is missing or not an id")
    if not isinstance(key_path, str) or not key_path:
        raise Refused("config: key_path is missing")
    if not isinstance(repo, str) or not REPO_RE.match(repo):
        raise Refused("config: repo must be owner/name")
    api_url = raw.get("api_url") or API_URL
    remote = raw.get("remote_url") or "https://github.com/%s.git" % repo
    if not isinstance(api_url, str) or not isinstance(remote, str):
        raise Refused("config: api_url and remote_url must be strings")
    return Config(app_id, key_path, repo, api_url, remote)


def main(argv: List[str], env: Mapping[str, str], out: TextIO, err: TextIO) -> int:
    parser = argparse.ArgumentParser(prog="push_wrapper.py", add_help=False)
    parser.add_argument("--worktree", required=True)
    parser.add_argument("--issue", required=True, type=int)
    parser.add_argument("--title", required=True)
    parser.add_argument("--body-file", required=True)
    parser.add_argument("--agent-model")
    parser.add_argument("--config")
    try:
        args = parser.parse_args(argv)
    except SystemExit:
        return 2
    path = args.config or env.get("NOTCHLING_PUSH_WRAPPER_CONFIG") or os.path.expanduser(
        DEFAULT_CONFIG.replace("~", env.get("HOME", "~"), 1))
    try:
        cfg = load_config(path)
        res = push_and_open_pr(Request(args.worktree, args.issue, args.title, args.body_file,
                                       args.agent_model), cfg)
    except Refused as e:
        res = Result(1, str(e))
    except Failed as e:
        res = Result(2, str(e))
    if res.code == 0:
        out.write(res.message + "\n")
    else:
        err.write("refused: %s\n" % res.message if res.code == 1 else "error: %s\n" % res.message)
    return res.code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:], os.environ, sys.stdout, sys.stderr))
