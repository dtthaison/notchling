"""Push wrapper: branch rules, the App JWT, the narrowed installation token, a real push to a local bare
repo, and the promise that the token is in no argv and no file. A fake HTTP server stands in for GitHub."""

from __future__ import annotations

import base64
import io
import json
import os
import shutil
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

import push_wrapper
from tests.test_preflight import FakeGitHub

TOKEN = "ghs_SECRETINSTALLATIONTOKEN0123456789"
HAVE_OPENSSL = shutil.which("openssl") is not None
GIT_ENV = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com",
           "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com",
           "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull}


def git(repo, *args):
    return subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True, text=True,
                          env=GIT_ENV).stdout.strip()


def b64url_decode(s):
    return base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))


_KEY = []


def throwaway_key():
    """A throwaway RSA key from `openssl genrsa`, made once per run (it takes a second)."""
    if not _KEY:
        _KEY.append(subprocess.run(["openssl", "genrsa", "2048"], check=True, capture_output=True).stdout)
    return _KEY[0]


class Recording(push_wrapper.Runner):
    """The real runner, remembering every call so a test can read the argv and env."""

    def __init__(self):
        self.calls = []

    def __call__(self, argv, env=None, input=None, cwd=None):
        self.calls.append({"argv": list(argv), "env": dict(env) if env is not None else None,
                           "input": input})
        return super().__call__(argv, env=env, input=input, cwd=cwd)


@unittest.skipUnless(HAVE_OPENSSL, "needs the openssl command line")
class Base(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="pw-test-")).resolve()
        self.addCleanup(shutil.rmtree, self.root, ignore_errors=True)
        self.tmp = self.root / "tmp"
        self.tmp.mkdir()
        self.key = self.root / "app.pem"
        self.key.write_bytes(throwaway_key())
        self.key.chmod(0o600)
        self.bare = self.root / "remote.git"
        subprocess.run(["git", "init", "-q", "--bare", str(self.bare)], check=True, env=GIT_ENV)
        self.wt = self.root / "wt"
        subprocess.run(["git", "init", "-q", "-b", "main", str(self.wt)], check=True, env=GIT_ENV)
        (self.wt / "a.txt").write_text("one\n")
        git(self.wt, "add", "a.txt")
        git(self.wt, "commit", "-q", "-m", "base")
        git(self.wt, "checkout", "-q", "-b", "agent/5-fix-thing")
        (self.wt / "a.txt").write_text("two\n")
        git(self.wt, "commit", "-q", "-am", "work")
        self.sha = git(self.wt, "rev-parse", "HEAD")
        self.body = self.root / "body.md"
        self.body.write_text("What changed and why.\n")

    def start(self, routes=None):
        r = {("GET", "/repos/o/r/installation"): (200, {"id": 77}),
             ("POST", "/app/installations/77/access_tokens"): (201, {"token": TOKEN,
                                                                      "expires_at": "2026-11-08T13:00:00Z"}),
             ("GET", "/repos/o/r"): (200, {"default_branch": "main"}),
             ("POST", "/repos/o/r/pulls"): (201, {"number": 9, "html_url": "https://github.com/o/r/pull/9"})}
        r.update(routes or {})
        self.srv = FakeGitHub(r)
        self.addCleanup(self.srv.close)
        self.cfg = push_wrapper.Config(app_id="12345", key_path=str(self.key), repo="o/r",
                                       api_url=self.srv.url, remote_url=str(self.bare))
        self.runner = Recording()

    def request(self, **over):
        kw = dict(worktree=str(self.wt), issue=5, title="Fix the thing", body_file=str(self.body),
                  agent_model=None)
        kw.update(over)
        return push_wrapper.Request(**kw)

    def go(self, **over):
        return push_wrapper.push_and_open_pr(self.request(**over), self.cfg, runner=self.runner,
                                             tmp_root=str(self.tmp))

    def api_paths(self):
        return [(r["method"], r["path"]) for r in self.srv.requests]

    def remote_branches(self):
        out = subprocess.run(["git", "-C", str(self.bare), "for-each-ref", "--format=%(refname) %(objectname)"],
                             capture_output=True, text=True, env=GIT_ENV).stdout
        return dict(line.split() for line in out.splitlines())


class BranchRules(unittest.TestCase):
    def test_valid_branches(self):
        for b in ("agent/5-fix-thing", "agent/5-a", "agent/5-fix-2-things"):
            self.assertEqual(push_wrapper.validate_branch(b, 5), b)

    def test_refused_branches(self):
        for b in ("main", "HEAD", "agent/5", "agent/5-", "agent/6-x", "agent/05-x", "agent/5-Fix",
                  "agent/5-fix/extra", "agent/5-fix_x", "agent/5--x", "agent/5-x-", "agent/5-../x",
                  "refs/heads/agent/5-x", "refs/tags/v1", "x/agent/5-x", "agent/5-fix thing",
                  "agent/5-x\n", "agent/5-" + "a" * 61, "", "agent/5-é", "-agent/5-x"):
            with self.assertRaises(push_wrapper.Refused, msg=repr(b)):
                push_wrapper.validate_branch(b, 5)

    def test_refused_refspecs(self):
        ok = push_wrapper.validate_ref("refs/heads/agent/5-x")
        self.assertEqual(ok, "refs/heads/agent/5-x")
        for ref in ("refs/heads/main", "refs/tags/agent/5-x", "refs/tags/v1", "+refs/heads/agent/5-x",
                    "refs/heads/agent/5-x:refs/heads/main", "refs/heads/agent/*", "", "agent/5-x",
                    "refs/heads/agent/", "refs/heads/agent/5-x ", "refs/heads/agent/../main",
                    ":refs/heads/agent/5-x"):
            with self.assertRaises(push_wrapper.Refused, msg=repr(ref)):
                push_wrapper.validate_ref(ref)


class InputRules(unittest.TestCase):
    def test_titles(self):
        self.assertEqual(push_wrapper.validate_title("Fix it"), "Fix it")
        for t in ("", "   ", "two\nlines", "x" * 257, "bell\x07"):
            with self.assertRaises(push_wrapper.Refused, msg=repr(t)):
                push_wrapper.validate_title(t)

    def test_models(self):
        self.assertEqual(push_wrapper.validate_model("claude-opus-5-5"), "claude-opus-5-5")
        self.assertIsNone(push_wrapper.validate_model(None))
        for m in ("a\nb", "x y", "m" * 65, "", "a;b"):
            with self.assertRaises(push_wrapper.Refused, msg=repr(m)):
                push_wrapper.validate_model(m)


@unittest.skipUnless(HAVE_OPENSSL, "needs the openssl command line")
class JwtTests(Base):
    def test_shape_and_signature(self):
        jwt = push_wrapper.make_jwt("12345", str(self.key), now=1_000_000, runner=push_wrapper.Runner())
        head, payload, sig = jwt.split(".")
        for part in (head, payload, sig):
            self.assertNotIn("=", part)
            self.assertRegex(part, r"^[A-Za-z0-9_-]+$")
        self.assertEqual(json.loads(b64url_decode(head)), {"alg": "RS256", "typ": "JWT"})
        self.assertEqual(json.loads(b64url_decode(payload)),
                         {"iat": 1_000_000 - 60, "exp": 1_000_000 + 540, "iss": 12345})
        pub = self.root / "pub.pem"
        subprocess.run(["openssl", "rsa", "-in", str(self.key), "-pubout", "-out", str(pub)], check=True,
                       capture_output=True)
        sigfile = self.root / "sig.bin"
        sigfile.write_bytes(b64url_decode(sig))
        res = subprocess.run(["openssl", "dgst", "-sha256", "-verify", str(pub), "-signature", str(sigfile)],
                             input=(head + "." + payload).encode(), capture_output=True)
        self.assertEqual(res.returncode, 0, res.stderr)

    def test_key_must_be_private(self):
        for mode in (0o644, 0o640, 0o604, 0o666):
            self.key.chmod(mode)
            with self.assertRaises(push_wrapper.Refused, msg=oct(mode)):
                push_wrapper.check_key(str(self.key))
        self.key.chmod(0o400)
        push_wrapper.check_key(str(self.key))

    def test_missing_key_and_non_file_are_refused(self):
        with self.assertRaises(push_wrapper.Refused):
            push_wrapper.check_key(str(self.root / "nope.pem"))
        with self.assertRaises(push_wrapper.Refused):
            push_wrapper.check_key(str(self.root))

    def test_symlinked_key_is_refused(self):
        link = self.root / "link.pem"
        link.symlink_to(self.key)
        with self.assertRaises(push_wrapper.Refused):
            push_wrapper.check_key(str(link))

    def test_key_owned_by_someone_else_is_refused(self):
        real = os.geteuid()
        orig = os.geteuid
        os.geteuid = lambda: real + 1
        self.addCleanup(setattr, os, "geteuid", orig)
        with self.assertRaises(push_wrapper.Refused):
            push_wrapper.check_key(str(self.key))


class SuccessPath(Base):
    def test_pushes_the_branch_and_opens_the_pr(self):
        self.start()
        res = self.go(agent_model="claude-opus-5-5")
        self.assertEqual((res.code, res.message), (0, "https://github.com/o/r/pull/9"), res)
        self.assertEqual(self.remote_branches(), {"refs/heads/agent/5-fix-thing": self.sha})

        reqs = {(r["method"], r["path"]): r for r in self.srv.requests}
        lookup = reqs[("GET", "/repos/o/r/installation")]
        self.assertRegex(lookup["auth"], r"^Bearer [\w-]+\.[\w-]+\.[\w-]+$")
        mint = reqs[("POST", "/app/installations/77/access_tokens")]
        self.assertEqual(mint["auth"], lookup["auth"])
        self.assertEqual(json.loads(mint["body"]), {"repositories": ["r"], "permissions": {
            "contents": "write", "pull_requests": "write"}})
        pr = reqs[("POST", "/repos/o/r/pulls")]
        self.assertEqual(pr["auth"], "Bearer " + TOKEN)
        body = json.loads(pr["body"])
        self.assertEqual((body["title"], body["head"], body["base"]),
                         ("Fix the thing", "agent/5-fix-thing", "main"))
        self.assertIn("What changed and why.", body["body"])
        self.assertIn("Agent-Model: claude-opus-5-5", body["body"])
        self.assertIn("Refs #5", body["body"])

    def test_no_model_line_without_a_model(self):
        self.start()
        self.assertEqual(self.go().code, 0)
        pr = [r for r in self.srv.requests if r["path"] == "/repos/o/r/pulls"][0]
        self.assertNotIn("Agent-Model", json.loads(pr["body"])["body"])

    def test_push_command_is_exact(self):
        self.start()
        self.go()
        pushes = [c for c in self.runner.calls if "push" in c["argv"]]
        self.assertEqual(len(pushes), 1)
        argv = pushes[0]["argv"]
        self.assertEqual(argv[argv.index("push"):],
                         ["push", str(self.bare), "%s:refs/heads/agent/5-fix-thing" % self.sha])
        for bad in ("--force", "-f", "--force-with-lease", "--tags", "--mirror", "--all", "--delete"):
            self.assertNotIn(bad, argv)

    def test_token_is_in_no_argv_and_no_file(self):
        self.start()
        self.go()
        for call in self.runner.calls:
            self.assertNotIn(TOKEN, " ".join(call["argv"]))
            self.assertNotIn(base64.b64encode(("x-access-token:" + TOKEN).encode()).decode(),
                             " ".join(call["argv"]))
            self.assertNotIn(TOKEN.encode(), call["input"] or b"")
        needle = TOKEN.encode()
        encoded = base64.b64encode(("x-access-token:" + TOKEN).encode())
        for path in self.root.rglob("*"):
            if path.is_file():
                data = path.read_bytes()
                self.assertNotIn(needle, data, str(path))
                self.assertNotIn(encoded, data, str(path))
        self.assertEqual(list(self.tmp.iterdir()), [], "scratch directory left behind")

    def test_token_reaches_git_only_through_the_environment(self):
        self.start()
        self.cfg = push_wrapper.Config(app_id="12345", key_path=str(self.key), repo="o/r",
                                       api_url=self.srv.url, remote_url="https://github.com/o/r.git")
        env = push_wrapper.git_env(TOKEN, self.cfg.remote_url, "/scratch")
        self.assertEqual(env["GIT_CONFIG_COUNT"], "1")
        self.assertEqual(env["GIT_CONFIG_KEY_0"], "http.https://github.com/.extraheader")
        want = "AUTHORIZATION: basic " + base64.b64encode(("x-access-token:" + TOKEN).encode()).decode()
        self.assertEqual(env["GIT_CONFIG_VALUE_0"], want)
        self.assertEqual(env["GIT_TERMINAL_PROMPT"], "0")
        self.assertEqual(env["GIT_CONFIG_GLOBAL"], os.devnull)
        env = push_wrapper.git_env(TOKEN, str(self.bare), "/scratch")
        self.assertNotIn("GIT_CONFIG_COUNT", env)  # a local path takes no header

    def test_the_scratch_dir_is_removed_even_when_the_push_fails(self):
        self.start()
        # Make the remote refuse by pre-creating a diverged branch of the same name.
        other = self.root / "other"
        subprocess.run(["git", "clone", "-q", str(self.bare), str(other)], check=True, env=GIT_ENV,
                       capture_output=True)
        git(other, "checkout", "-q", "-b", "agent/5-fix-thing")
        (other / "z.txt").write_text("z\n")
        git(other, "add", "z.txt")
        git(other, "commit", "-q", "-m", "diverged")
        git(other, "push", "-q", "origin", "agent/5-fix-thing")
        before = self.remote_branches()
        res = self.go()
        self.assertEqual(res.code, 2)
        self.assertEqual(self.remote_branches(), before, "a diverged branch must not be overwritten")
        self.assertNotIn(("POST", "/repos/o/r/pulls"), self.api_paths())
        self.assertEqual(list(self.tmp.iterdir()), [])

    def test_existing_pr_is_reported_not_duplicated(self):
        self.start({("POST", "/repos/o/r/pulls"): (422, {"message": "A pull request already exists"}),
                    ("GET", "/repos/o/r/pulls"): (200, [{"number": 4, "html_url": "https://github.com/o/r/pull/4"}])})
        res = self.go()
        self.assertEqual((res.code, res.message), (0, "https://github.com/o/r/pull/4"), res)
        q = [r["query"] for r in self.srv.requests if r["method"] == "GET" and r["path"] == "/repos/o/r/pulls"][0]
        self.assertIn("head=o%3Aagent%2F5-fix-thing", q)


class Refusals(Base):
    def assertRefused(self, res, fragment, code=1):
        self.assertEqual(res.code, code, res)
        self.assertIn(fragment, res.message)
        self.assertEqual(self.api_paths(), [], "refusals happen before any network call")
        self.assertEqual(self.remote_branches(), {})

    def test_main_branch(self):
        self.start()
        git(self.wt, "checkout", "-q", "main")
        self.assertRefused(self.go(), "branch")

    def test_wrong_issue_number(self):
        self.start()
        self.assertRefused(self.go(issue=6), "branch")

    def test_detached_head(self):
        self.start()
        git(self.wt, "checkout", "-q", "--detach")
        self.assertRefused(self.go(), "branch")

    def test_dirty_tracked_file(self):
        self.start()
        (self.wt / "a.txt").write_text("changed\n")
        self.assertRefused(self.go(), "not clean")

    def test_untracked_file(self):
        self.start()
        (self.wt / "new.txt").write_text("x\n")
        self.assertRefused(self.go(), "not clean")

    def test_staged_file(self):
        self.start()
        (self.wt / "new.txt").write_text("x\n")
        git(self.wt, "add", "new.txt")
        self.assertRefused(self.go(), "not clean")

    def test_not_a_repository(self):
        self.start()
        self.assertRefused(self.go(worktree=str(self.root / "missing")), "worktree")
        plain = self.root / "plain"
        plain.mkdir()
        self.assertRefused(self.go(worktree=str(plain)), "worktree")

    def test_bad_title_model_and_body(self):
        self.start()
        self.assertRefused(self.go(title="a\nb"), "title")
        self.assertRefused(self.go(agent_model="x\ny"), "model")
        self.assertRefused(self.go(body_file=str(self.root / "nope.md")), "body")
        big = self.root / "big.md"
        big.write_text("x" * 70000)
        self.assertRefused(self.go(body_file=str(big)), "body")

    def test_loose_key_is_refused_before_any_call(self):
        self.start()
        self.key.chmod(0o644)
        self.assertRefused(self.go(), "key")

    def test_issue_must_be_positive(self):
        self.start()
        self.assertRefused(self.go(issue=0), "issue")


class ApiErrors(Base):
    def assertFailedBeforePush(self, res, fragment):
        self.assertEqual(res.code, 2, res)
        self.assertIn(fragment, res.message)
        self.assertEqual(self.remote_branches(), {})
        self.assertNotIn(TOKEN, res.message)

    def test_app_not_installed(self):
        self.start({("GET", "/repos/o/r/installation"): (404, {"message": "Not Found"})})
        self.assertFailedBeforePush(self.go(), "404")

    def test_token_refused(self):
        self.start({("POST", "/app/installations/77/access_tokens"): (401, {"message": "Bad credentials"})})
        self.assertFailedBeforePush(self.go(), "401")

    def test_token_response_without_token(self):
        self.start({("POST", "/app/installations/77/access_tokens"): (201, {"nope": 1})})
        self.assertFailedBeforePush(self.go(), "token")

    def test_default_branch_unreadable(self):
        self.start({("GET", "/repos/o/r"): (500, {})})
        self.assertFailedBeforePush(self.go(), "500")

    def test_pr_creation_error_after_push_is_code_2(self):
        self.start({("POST", "/repos/o/r/pulls"): (403, {"message": "no"})})
        res = self.go()
        self.assertEqual(res.code, 2)
        self.assertIn("pushed", res.message)  # the branch is up; only the PR is missing


class HostileWorktree(Base):
    """The worktree belongs to the agent. Nothing in its git config or attributes may run as the wrapper."""

    def test_config_and_attributes_do_not_execute(self):
        self.start()
        marker = self.root / "pwned"
        script = self.root / "evil.sh"
        script.write_text("#!/bin/sh\necho hit >> %s\ncat\n" % marker)
        script.chmod(0o755)
        (self.wt / ".git" / "info").mkdir(exist_ok=True)
        (self.wt / ".git" / "info" / "attributes").write_text("* filter=evil\n")
        with open(self.wt / ".git" / "config", "a") as f:
            f.write("[core]\n\tfsmonitor = %s\n\thooksPath = %s\n" % (script, self.root))
            f.write("[filter \"evil\"]\n\tclean = %s\n\tsmudge = %s\n" % (script, script))
            f.write("[credential]\n\thelper = !%s\n" % script)
            f.write("[url \"%s\"]\n\tinsteadOf = %s\n" % (self.root / "elsewhere", self.bare))
        hook = self.wt / ".git" / "hooks" / "pre-push"
        hook.parent.mkdir(exist_ok=True)
        hook.write_text("#!/bin/sh\necho hook >> %s\n" % marker)
        hook.chmod(0o755)
        # Make every file look modified by stat so a naive `git status` has to run the clean filter.
        os.utime(self.wt / "a.txt", (1, 1))
        naive = subprocess.run(["git", "-C", str(self.wt), "status", "--porcelain"], capture_output=True,
                               env=GIT_ENV)
        self.assertTrue(marker.exists(), "the trap must fire on a naive status, or this test proves nothing")
        marker.unlink()

        res = self.go()
        self.assertEqual(res.code, 0, res)
        self.assertFalse(marker.exists(), "something in the agent's repo ran as the wrapper")
        self.assertEqual(self.remote_branches(), {"refs/heads/agent/5-fix-thing": self.sha})


class Cli(Base):
    def run_main(self, argv, env=None):
        out, err = io.StringIO(), io.StringIO()
        code = push_wrapper.main(argv, env or {}, out, err)
        return code, out.getvalue(), err.getvalue()

    def test_usage_and_missing_config(self):
        self.assertEqual(self.run_main([])[0], 2)
        code, _, err = self.run_main(["--worktree", str(self.wt), "--issue", "5", "--title", "t",
                                      "--body-file", str(self.body), "--config", str(self.root / "no.json")])
        self.assertEqual(code, 2)
        self.assertIn("config", err)

    def test_config_loading(self):
        self.start()
        path = self.root / "cfg.json"
        path.write_text(json.dumps({"app_id": 12345, "key_path": str(self.key), "repo": "o/r",
                                    "api_url": self.srv.url, "remote_url": str(self.bare)}))
        cfg = push_wrapper.load_config(str(path))
        self.assertEqual((cfg.app_id, cfg.repo, cfg.api_url), ("12345", "o/r", self.srv.url))
        path.write_text(json.dumps({"app_id": 1, "key_path": "k", "repo": "not a repo"}))
        with self.assertRaises(push_wrapper.Refused):
            push_wrapper.load_config(str(path))
        path.write_text("{")
        with self.assertRaises(push_wrapper.Refused):
            push_wrapper.load_config(str(path))

    def test_defaults_point_at_github(self):
        path = self.root / "cfg.json"
        path.write_text(json.dumps({"app_id": 1, "key_path": "k", "repo": "o/r"}))
        cfg = push_wrapper.load_config(str(path))
        self.assertEqual((cfg.api_url, cfg.remote_url), ("https://api.github.com", "https://github.com/o/r.git"))

    def test_end_to_end_through_main(self):
        self.start()
        path = self.root / "cfg.json"
        path.write_text(json.dumps({"app_id": 12345, "key_path": str(self.key), "repo": "o/r",
                                    "api_url": self.srv.url, "remote_url": str(self.bare)}))
        code, out, err = self.run_main(["--worktree", str(self.wt), "--issue", "5", "--title", "Fix",
                                        "--body-file", str(self.body), "--agent-model", "m1",
                                        "--config", str(path)])
        self.assertEqual((code, out.strip()), (0, "https://github.com/o/r/pull/9"), err)
        self.assertNotIn(TOKEN, out + err)

    def test_refusal_exit_code_is_1(self):
        self.start()
        path = self.root / "cfg.json"
        path.write_text(json.dumps({"app_id": 12345, "key_path": str(self.key), "repo": "o/r",
                                    "api_url": self.srv.url, "remote_url": str(self.bare)}))
        git(self.wt, "checkout", "-q", "main")
        code, _, err = self.run_main(["--worktree", str(self.wt), "--issue", "5", "--title", "Fix",
                                      "--body-file", str(self.body), "--config", str(path)])
        self.assertEqual(code, 1)
        self.assertIn("refused", err)


if __name__ == "__main__":
    unittest.main()
