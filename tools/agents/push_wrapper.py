#!/usr/bin/env python3

from __future__ import annotations

import argparse
import base64
import json
import os
import pwd
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time
import unicodedata
import urllib.parse
from typing import Any, BinaryIO, Callable, List, Mapping, NamedTuple, Optional, TextIO, Tuple

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from preflight import API_URL, REPO_RE, Api, ApiError  # noqa: E402

BRANCH_RE = re.compile(r"agent/([1-9][0-9]*)-([a-z0-9]+(?:-[a-z0-9]+)*)")
MODEL_RE = re.compile(r"[A-Za-z0-9._:/-]{1,64}")
SHA_RE = re.compile(r"[0-9a-f]{40}|[0-9a-f]{64}")
BRANCH_NAME_RE = re.compile(r"[A-Za-z0-9._/-]{1,100}")
SLUG_MAX = 60
TITLE_MAX = 256
BODY_MAX = 60000
INDEX_MAX = 512 * 1024 * 1024
PRIVATE_KEY_MARK = "PRIVATE KEY-----"
WORKFLOWS_DIR = ".github/workflows/"
BASE_REFS = ("refs/remotes/origin/HEAD", "refs/remotes/origin/main", "refs/remotes/origin/master",
             "refs/heads/main", "refs/heads/master")
ZWJ = "\u200d"
MENTION_RE = re.compile(r"@(?=[A-Za-z0-9_])")
_REPO = r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+"
CLOSING_RE = re.compile(
    r"(?<![A-Za-z0-9])(?P<kw>clos(?:e[sd]?)|fix(?:e[sd])?|resolv(?:e[sd]?))(?![A-Za-z0-9])[\s:*_\[(]*"
    r"(?:(?P<repo>%s)?#(?P<num>[0-9]+)|https?://github\.com/(?P<urepo>%s)/(?:issues|pull)/(?P<unum>[0-9]+))"
    % (_REPO, _REPO), re.IGNORECASE)
REJECT_RE = re.compile(r"\s*!\s*\[(?:remote )?rejected\]")
ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")
FAILURE_MAX = 600
GIT_TIMEOUT = 300
PERMISSIONS = {"contents": "write", "pull_requests": "write"}
CONFIG_MAX = 64 * 1024
DEFAULT_CONFIG = os.path.join(".config", "notchling", "push-wrapper.json")
TOOL_DIRS = ("/usr/bin", "/bin", "/usr/local/bin", "/opt/homebrew/bin")
SAFE_PATH = ":".join(TOOL_DIRS)
PROXY_ENV = ("HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "NO_PROXY", "no_proxy")
TESTING_VAR = "NOTCHLING_PUSH_WRAPPER_TESTING"


class Refused(Exception):
    pass


class Failed(Exception):
    pass


class ToolMissing(Failed):
    pass


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
    body: bytes
    agent_model: Optional[str] = None


class Runner:

    def __call__(self, argv: List[str], env: Optional[Mapping[str, str]] = None,
                 input: Optional[bytes] = None, cwd: Optional[str] = None) -> "subprocess.CompletedProcess[bytes]":
        extra = {"stdin": subprocess.DEVNULL} if input is None else {}
        try:
            return subprocess.run(argv, env=None if env is None else dict(env), input=input, cwd=cwd,
                                  capture_output=True, timeout=GIT_TIMEOUT, **extra)
        except (OSError, subprocess.TimeoutExpired) as e:
            raise Failed("%s could not run (%s)" % (argv[0], type(e).__name__))


def in_dir(runner: Runner, cwd: str) -> Runner:
    def run(argv: List[str], env: Optional[Mapping[str, str]] = None, input: Optional[bytes] = None,
            cwd_: Optional[str] = None) -> "subprocess.CompletedProcess[bytes]":
        return runner(argv, env=env, input=input, cwd=cwd_ or cwd)
    return run  # type: ignore[return-value]


def find_tool(name: str) -> str:
    for d in TOOL_DIRS:
        path = os.path.join(d, name)
        if os.path.isfile(path) and os.access(path, os.X_OK):
            return path
    raise ToolMissing("%s was not found in %s" % (name, ", ".join(TOOL_DIRS)))


def scrub_environment(environ: Mapping[str, str]) -> dict:
    out = {k: environ[k] for k in PROXY_ENV if k in environ}
    out.update({"PATH": SAFE_PATH, "LANG": "C"})
    return out


def validate_branch(branch: str, issue: int) -> str:
    m = BRANCH_RE.fullmatch(branch or "")
    if not m or int(m.group(1)) != issue or len(m.group(2)) > SLUG_MAX:
        raise Refused("the branch must be agent/%d-<lowercase-slug>" % issue)
    return branch


def validate_ref(ref: str) -> str:
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


def read_stdin_body(stream: BinaryIO) -> bytes:
    try:
        if stream.isatty():
            raise Refused("pipe the pull request body on stdin")
        return stream.read(BODY_MAX + 1)
    except (OSError, ValueError):
        raise Refused("the body could not be read from stdin")


def validate_body(raw: bytes) -> str:
    if len(raw) > BODY_MAX:
        raise Refused("the body is over %d bytes" % BODY_MAX)
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        raise Refused("the body is not UTF-8 text")
    if "\x00" in text:
        raise Refused("the body is not text")
    if PRIVATE_KEY_MARK in text:
        raise Refused("the body holds a private key, so nothing was sent")
    return text


def holds_secret(text: str, token: str) -> bool:
    basic = base64.b64encode(("x-access-token:" + token).encode()).decode()
    return bool(token) and (token in text or basic in text)


def open_regular(path: str, what: str, limit: int) -> Tuple[int, os.stat_result]:
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | getattr(os, "O_CLOEXEC", 0)
    try:
        fd = os.open(path, flags)
    except OSError:
        raise Refused("%s is missing or is not a plain file" % what)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode):
            raise Refused("%s is not a regular file" % what)
        if st.st_nlink != 1:
            raise Refused("%s has more than one hard link" % what)
        if st.st_size > limit:
            raise Refused("%s is over %d bytes" % (what, limit))
    except BaseException:
        os.close(fd)
        raise
    return fd, st


def copy_regular(src: str, dst: str, what: str, limit: int) -> None:
    fd, _ = open_regular(src, what, limit)
    try:
        total = 0
        with open(dst, "wb") as out:
            while True:
                chunk = os.read(fd, 1 << 20)
                if not chunk:
                    break
                total += len(chunk)
                if total > limit:
                    raise Refused("%s is over %d bytes" % (what, limit))
                out.write(chunk)
    except OSError:
        raise Refused("%s could not be read" % what)
    finally:
        os.close(fd)


def check_object_store(common: str) -> str:
    objects = os.path.join(common, "objects")
    if os.path.islink(objects) or not os.path.isdir(objects):
        raise Refused("the worktree's repository has no plain objects folder")
    for name in ("alternates", "http-alternates"):
        if os.path.lexists(os.path.join(objects, "info", name)):
            raise Refused("the worktree's repository borrows objects from elsewhere (objects/info/%s); "
                          "give the agent a full clone, not --shared or --reference" % name)
    return objects


def check_key(path: str) -> None:
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


def _b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def make_jwt(app_id: str, key_path: str, now: int, runner: Runner) -> str:
    header = _b64url(json.dumps({"alg": "RS256", "typ": "JWT"}, separators=(",", ":")).encode())
    iss: Any = int(app_id) if str(app_id).isdigit() else str(app_id)
    payload = _b64url(json.dumps({"iat": now - 60, "exp": now + 540, "iss": iss},
                                 separators=(",", ":")).encode())
    signing_input = ("%s.%s" % (header, payload)).encode()
    proc = runner([find_tool("openssl"), "dgst", "-sha256", "-sign", key_path],
                  env={"PATH": SAFE_PATH, "LANG": "C"}, input=signing_input)
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


def git_env(token: Optional[str], remote_url: str, scratch: str, global_config: str = os.devnull) -> dict:
    env = {k: os.environ[k] for k in PROXY_ENV if k in os.environ}
    env.update({"PATH": SAFE_PATH, "HOME": scratch, "LANG": "C", "LC_ALL": "C", "GIT_TERMINAL_PROMPT": "0",
                "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": global_config, "GIT_ASKPASS": "true"})
    parts = urllib.parse.urlsplit(remote_url)
    if token and parts.scheme in ("http", "https") and parts.netloc:
        basic = base64.b64encode(("x-access-token:" + token).encode()).decode()
        env.update({"GIT_CONFIG_COUNT": "1",
                    "GIT_CONFIG_KEY_0": "http.%s://%s/.extraheader" % (parts.scheme, parts.netloc),
                    "GIT_CONFIG_VALUE_0": "AUTHORIZATION: basic " + basic})
    return env


def _tidy(line: str, secret: str) -> str:
    line = ANSI_RE.sub("", line)
    line = "".join(c for c in line if unicodedata.category(c) not in ("Cc", "Cf", "Zl", "Zp")).strip()
    if secret:
        basic = base64.b64encode(("x-access-token:" + secret).encode()).decode()
        line = line.replace(secret, "[token]").replace(basic, "[token]")
    return line


def push_failure(stderr: bytes, secret: str = "") -> Tuple[bool, str]:
    lines = [l for l in (_tidy(l, secret) for l in stderr.decode("utf-8", "replace").splitlines()) if l]
    marks = [l for l in lines if REJECT_RE.match(l)]
    if not marks:
        return False, (lines[-1] if lines else "")[:300]
    reasons = [l[len("remote:"):].strip() for l in lines if l.startswith("remote:")]
    text = "; ".join([r for r in reasons if r][:6] + marks[:3])
    return True, text[:FAILURE_MAX]


def _git(runner: Runner, args: List[str], env: Mapping[str, str], what: str, secret: str = "") -> bytes:
    proc = runner([find_tool("git")] + args, env=env)
    if proc.returncode != 0:
        rejected, text = push_failure(proc.stderr, secret)
        if rejected:
            raise Refused("the remote rejected the push: %s" % text)
        raise Failed("%s failed (exit %d)%s" % (what, proc.returncode, ": " + text if text else ""))
    return proc.stdout


def workflow_changes(runner: Runner, safe: List[str], env: Mapping[str, str], sgit: str, sha: str) -> List[str]:
    base_sha = ""
    for ref in BASE_REFS:
        try:
            out = _git(runner, safe + ["rev-parse", "--verify", "-q", ref + "^{commit}"], env,
                       "reading the base").decode().strip()
        except Failed:
            continue
        if SHA_RE.fullmatch(out):
            base_sha = out
            break
    if not base_sha:
        return []
    try:
        fork = _git(runner, ["--git-dir", sgit, "merge-base", base_sha, sha], env,
                    "comparing with the base").decode().strip()
        names = _git(runner, ["--git-dir", sgit, "diff-tree", "-r", "--name-only", "--no-renames", "-z",
                              fork, sha], env, "comparing with the base").decode("utf-8", "replace")
    except Failed:
        return []
    return [n for n in names.split("\0") if n.lower().startswith(WORKFLOWS_DIR)]


def inspect_worktree(runner: Runner, worktree: str, issue: int, scratch: str) -> tuple:
    wt = os.path.realpath(worktree)
    if not os.path.isdir(wt) or not os.path.exists(os.path.join(wt, ".git")):
        raise Refused("the worktree is not a git worktree")
    allow = os.path.join(scratch, "gitconfig")
    with open(allow, "w") as f:
        f.write("[safe]\n\tdirectory = *\n")
    env = git_env(None, "", scratch, global_config=allow)
    safe = ["-c", "core.fsmonitor=false", "-C", wt]
    try:
        git_dir = _git(runner, safe + ["rev-parse", "--path-format=absolute", "--absolute-git-dir"],
                       env, "reading the worktree").decode().strip()
        common = _git(runner, safe + ["rev-parse", "--path-format=absolute", "--git-common-dir"],
                      env, "reading the worktree").decode().strip()
        branch = _git(runner, safe + ["symbolic-ref", "-q", "--short", "HEAD"], env,
                      "reading the branch").decode().strip()
    except ToolMissing:
        raise
    except Failed:
        raise Refused("the worktree has no readable branch")
    validate_branch(branch, issue)
    sha = _git(runner, safe + ["rev-parse", "--verify", "-q", "HEAD^{commit}"], env,
               "reading the commit").decode().strip()
    if not SHA_RE.fullmatch(sha) or "\n" in common:
        raise Refused("the worktree's commit could not be read")
    objects = check_object_store(common)

    sgit = os.path.join(scratch, "s.git")
    _git(runner, ["init", "-q", "--bare", "--template=", sgit], env, "preparing the scratch repository")
    with open(os.path.join(sgit, "objects", "info", "alternates"), "w") as f:
        f.write(objects + "\n")
    base = ["--git-dir", sgit]
    _git(runner, base + ["update-ref", validate_ref("refs/heads/" + branch), sha], env, "staging the commit")
    _git(runner, base + ["symbolic-ref", "HEAD", "refs/heads/" + branch], env, "staging the commit")
    copy_regular(os.path.join(git_dir, "index"), os.path.join(sgit, "index"), "the worktree's index",
                 INDEX_MAX)
    status = _git(runner, base + ["--work-tree", wt, "status", "--porcelain",
                                                              "--untracked-files=all", "--ignore-submodules=all"],
                  env, "checking the worktree")
    if status.strip():
        raise Refused("the worktree is not clean: commit or remove what is left")
    touched = workflow_changes(runner, safe, env, sgit, sha)
    if touched:
        shown = ", ".join("".join(c if c.isprintable() else "?" for c in n) for n in touched[:3])
        raise Refused("the branch changes files under .github/workflows/ (%s). The App has no workflows "
                      "permission, so GitHub would refuse the push; a maintainer has to make that change"
                      % shown)
    return branch, sha, sgit


def push_and_open_pr(req: Request, cfg: Config, runner: Optional[Runner] = None,
                     clock: Callable[[], float] = time.time, tmp_root: Optional[str] = None) -> Result:
    runner = runner or Runner()
    scratch = None
    token = ""
    api: Optional[Api] = None
    try:
        if req.issue < 1:
            raise Refused("the issue number must be positive")
        title = validate_title(req.title)
        model = validate_model(req.agent_model)
        body = validate_body(req.body)
        check_key(cfg.key_path)
        scratch = tempfile.mkdtemp(prefix="push-wrapper-", dir=tmp_root)
        runner = in_dir(runner, scratch)
        branch, sha, sgit = inspect_worktree(runner, req.worktree, req.issue, scratch)
        ref = validate_ref("refs/heads/" + branch)

        api = Api(cfg.api_url)
        jwt = make_jwt(cfg.app_id, cfg.key_path, int(clock()), runner)
        token = installation_token(api, cfg.repo, jwt)
        if holds_secret(body, token):
            raise Refused("the body holds the installation token, so nothing was sent")
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
        if token and api is not None:
            revoke_token(api, token)
        if scratch:
            shutil.rmtree(scratch, ignore_errors=True)


def revoke_token(api: Api, token: str) -> None:
    try:
        api.request("DELETE", "/installation/token", token=token)
    except Exception:  # noqa: BLE001  (a failed revoke must never change or mask the result)
        pass


def _scrub(text: str, token: str) -> str:
    return text.replace(token, "[token]") if token else text


def neutralize_text(text: str, repo: str, issue: int) -> str:
    own_repo = repo.lower()

    def closing(m: "re.Match[str]") -> str:
        where = (m.group("repo") or m.group("urepo") or "").lower()
        number = int(m.group("num") or m.group("unum"))
        if number == issue and where in ("", own_repo):
            return m.group(0)
        kw = m.group("kw")
        return kw[0] + ZWJ + kw[1:] + m.group(0)[len(kw):]

    return CLOSING_RE.sub(closing, MENTION_RE.sub("@" + ZWJ, text))


def open_pr(api: Api, repo: str, token: str, branch: str, base: str, title: str, body: str, issue: int,
            model: Optional[str]) -> str:
    title = neutralize_text(title, repo, issue)
    text = neutralize_text(body, repo, issue).rstrip() + "\n\nRefs #%d\n" % issue
    if model:
        text += "Agent-Model: %s\n" % model
    try:
        pr = api.request("POST", "/repos/%s/pulls" % repo,
                         {"title": title, "head": branch, "base": base, "body": text}, token=token)
    except ApiError as e:
        if e.status != 422:
            raise
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


def default_config_path() -> str:
    try:
        home = pwd.getpwuid(os.geteuid()).pw_dir
    except KeyError:
        raise Failed("this account has no entry in the user database, so it has no default config path")
    return os.path.join(home, DEFAULT_CONFIG)


def config_path(arg: Optional[str]) -> str:
    return arg or default_config_path()


def read_config_file(path: str) -> Any:
    if not os.path.lexists(path):
        raise Failed("the config file %s does not exist" % path)
    fd, st = open_regular(path, "the config file", CONFIG_MAX)
    try:
        if st.st_uid != os.geteuid():
            raise Refused("the config file is not owned by the user running the wrapper")
        if st.st_mode & 0o022:
            raise Refused("the config file must not be writable by group or others")
        with os.fdopen(fd, "rb", closefd=False) as f:
            return json.loads(f.read(CONFIG_MAX + 1))
    except OSError:
        raise Failed("the config file could not be read")
    except ValueError:
        raise Refused("the config file is not valid JSON")
    finally:
        os.close(fd)


def check_endpoints(api_url: str, remote_url: str, repo: str, env: Optional[Mapping[str, str]]) -> None:
    if env is not None and env.get(TESTING_VAR) == "1":
        return
    if api_url != API_URL:
        raise Refused("config: api_url must be %s" % API_URL)
    if remote_url not in ("https://github.com/%s.git" % repo, "https://github.com/%s" % repo):
        raise Refused("config: remote_url must be https://github.com/%s.git" % repo)


def load_config(path: str, env: Optional[Mapping[str, str]] = None) -> Config:
    raw = read_config_file(path)
    if not isinstance(raw, dict):
        raise Refused("the config file must be a JSON object")
    app_id, key_path, repo = raw.get("app_id"), raw.get("key_path"), raw.get("repo")
    if isinstance(app_id, int) and not isinstance(app_id, bool):
        app_id = str(app_id)
    if not isinstance(app_id, str) or not re.fullmatch(r"[A-Za-z0-9._-]{1,64}", app_id):
        raise Refused("config: app_id is missing or not an id")
    if not isinstance(key_path, str) or not key_path:
        raise Refused("config: key_path is missing")
    if not isinstance(repo, str) or not REPO_RE.fullmatch(repo):
        raise Refused("config: repo must be owner/name")
    api_url = raw.get("api_url") or API_URL
    remote = raw.get("remote_url") or "https://github.com/%s.git" % repo
    if not isinstance(api_url, str) or not isinstance(remote, str):
        raise Refused("config: api_url and remote_url must be strings")
    check_endpoints(api_url, remote, repo, env)
    return Config(app_id, key_path, repo, api_url, remote)


def main(argv: List[str], env: Mapping[str, str], out: TextIO, err: TextIO,
         stdin: Optional[BinaryIO] = None) -> int:
    parser = argparse.ArgumentParser(prog="push_wrapper.py", add_help=False)
    parser.add_argument("--worktree", required=True)
    parser.add_argument("--issue", required=True, type=int)
    parser.add_argument("--title", required=True)
    parser.add_argument("--agent-model")
    parser.add_argument("--config")
    try:
        args = parser.parse_args(argv)
    except SystemExit:
        return 2
    try:
        cfg = load_config(config_path(args.config), env)
        body = read_stdin_body(stdin if stdin is not None else sys.stdin.buffer)
        res = push_and_open_pr(Request(args.worktree, args.issue, args.title, body, args.agent_model), cfg)
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
    scrubbed = scrub_environment(os.environ)
    os.environ.clear()
    os.environ.update(scrubbed)
    sys.exit(main(sys.argv[1:], os.environ, sys.stdout, sys.stderr))
