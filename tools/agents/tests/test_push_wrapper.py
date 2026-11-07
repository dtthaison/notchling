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
import sys
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
                          env=GIT_ENV, stdin=subprocess.DEVNULL).stdout.strip()


def b64url_decode(s):
    return base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))


_KEY = []


def throwaway_key():
    """A throwaway RSA key from `openssl genrsa`, made once per run (it takes a second)."""
    if not _KEY:
        _KEY.append(subprocess.run(["openssl", "genrsa", "2048"], check=True, capture_output=True,
                                   stdin=subprocess.DEVNULL).stdout)
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
        subprocess.run(["git", "init", "-q", "--bare", str(self.bare)], check=True, env=GIT_ENV,
                       stdin=subprocess.DEVNULL)
        self.wt = self.root / "wt"
        subprocess.run(["git", "init", "-q", "-b", "main", str(self.wt)], check=True, env=GIT_ENV,
                       stdin=subprocess.DEVNULL)
        (self.wt / "a.txt").write_text("one\n")
        git(self.wt, "add", "a.txt")
        git(self.wt, "commit", "-q", "-m", "base")
        git(self.wt, "checkout", "-q", "-b", "agent/5-fix-thing")
        (self.wt / "a.txt").write_text("two\n")
        git(self.wt, "commit", "-q", "-am", "work")
        self.sha = git(self.wt, "rev-parse", "HEAD")
        self.body = b"What changed and why.\n"  # the PR body, as sd pipes it on stdin

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
        kw = dict(worktree=str(self.wt), issue=5, title="Fix the thing", body=self.body,
                  agent_model=None)
        kw.update(over)
        return push_wrapper.Request(**kw)

    def go(self, **over):
        return push_wrapper.push_and_open_pr(self.request(**over), self.cfg, runner=self.runner,
                                             tmp_root=str(self.tmp))

    def api_paths(self):
        return [(r["method"], r["path"]) for r in self.srv.requests]

    def assertRefused(self, res, fragment, code=1):
        self.assertEqual(res.code, code, res)
        self.assertIn(fragment, res.message)
        self.assertEqual(self.api_paths(), [], "refusals happen before any network call")
        self.assertEqual(self.remote_branches(), {})
        self.assertEqual(list(self.tmp.iterdir()), [], "scratch directory left behind")

    def remote_branches(self):
        out = subprocess.run(["git", "-C", str(self.bare), "for-each-ref", "--format=%(refname) %(objectname)"],
                             capture_output=True, text=True, env=GIT_ENV).stdout
        return dict(line.split() for line in out.splitlines())


AGENTS_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


class RunnerHygiene(unittest.TestCase):
    def test_children_never_inherit_stdin(self):
        """A child that reads stdin (`cat`, a git filter or fsmonitor) must see end-of-file at once, even
        when the wrapper's own stdin is a pipe that stays open."""
        code = ("import push_wrapper, sys\n"
                "r = push_wrapper.Runner()(['/bin/cat'])\n"
                "sys.exit(0 if r.returncode == 0 and r.stdout == b'' else 3)\n")
        proc = subprocess.Popen([sys.executable, "-c", code], cwd=AGENTS_DIR, stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        self.addCleanup(proc.stdin.close)
        try:
            self.assertEqual(proc.wait(timeout=10), 0, proc.stdout.read())
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
            self.fail("a child of Runner blocked reading the wrapper's stdin")
        finally:
            proc.stdout.close()


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
                       capture_output=True, stdin=subprocess.DEVNULL)
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
                       capture_output=True, stdin=subprocess.DEVNULL)
        git(other, "checkout", "-q", "-b", "agent/5-fix-thing")
        (other / "z.txt").write_text("z\n")
        git(other, "add", "z.txt")
        git(other, "commit", "-q", "-m", "diverged")
        git(other, "push", "-q", "origin", "agent/5-fix-thing")
        before = self.remote_branches()
        res = self.go()
        self.assertEqual(res.code, 1, "the remote rejecting the push is a refusal, not a transient error")
        self.assertIn("rejected", res.message)
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
        self.assertRefused(self.go(body=b"x" * 70000), "body")
        self.assertRefused(self.go(body=b"\xff\xfe not utf-8"), "body")
        self.assertRefused(self.go(body=b"a\x00b"), "body")

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


class Rejections(Base):
    """A push the remote refuses is a refusal (exit 1) that says why; only trouble reaching it is an error."""

    def hook(self, script):
        hook = self.bare / "hooks" / "pre-receive"
        hook.parent.mkdir(exist_ok=True)
        hook.write_text("#!/bin/sh\n" + script)
        hook.chmod(0o755)

    def test_a_rejection_with_a_reason_is_exit_1_and_keeps_the_reason(self):
        self.start()
        self.hook("echo 'refusing to allow a GitHub App to create or update workflow "
                  ".github/workflows/x.yml without workflows permission' >&2\nexit 1\n")
        res = self.go()
        self.assertEqual(res.code, 1, res)
        self.assertIn("rejected", res.message)
        self.assertIn("without workflows permission", res.message)
        self.assertNotIn(("POST", "/repos/o/r/pulls"), self.api_paths())
        self.assertEqual(self.remote_branches(), {})

    def test_trouble_reaching_the_remote_stays_an_error(self):
        self.start()
        self.cfg = self.cfg._replace(remote_url=str(self.root / "no-such-remote.git"))
        res = self.go()
        self.assertEqual(res.code, 2, res)
        self.assertNotIn("rejected", res.message)

    def test_failure_text_keeps_rejection_lines_and_drops_noise_and_secrets(self):
        stderr = ("remote: Permission to o/r.git denied\x1b[31m to the app\n"
                  "remote: token %s leaked\n"
                  "To https://github.com/o/r.git\n"
                  " ! [remote rejected] abc -> refs/heads/agent/5-x (pre-receive hook declined)\n"
                  "error: failed to push some refs to 'https://github.com/o/r.git'\n" % TOKEN).encode()
        rejected, text = push_wrapper.push_failure(stderr, TOKEN)
        self.assertTrue(rejected)
        self.assertIn("Permission to o/r.git denied", text)
        self.assertIn("[remote rejected]", text)
        self.assertIn("pre-receive hook declined", text)
        self.assertNotIn(TOKEN, text)
        self.assertNotIn("\x1b", text)
        self.assertLessEqual(len(text), 700)
        rejected, text = push_wrapper.push_failure(b"fatal: unable to access 'https://x/': Could not resolve host\n",
                                                   TOKEN)
        self.assertFalse(rejected)
        self.assertIn("Could not resolve host", text)
        rejected, _ = push_wrapper.push_failure(b" ! [rejected]        abc -> refs/heads/agent/5-x (fetch first)\n", "")
        self.assertTrue(rejected)


class WorkflowFiles(Base):
    """The App has no `workflows` permission, so GitHub refuses any push that touches .github/workflows/.
    The wrapper says so itself, before it mints a token."""

    def commit_file(self, rel, text="on: push\n"):
        path = self.wt / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        git(self.wt, "add", rel)
        git(self.wt, "commit", "-q", "-m", "touch " + rel)

    def assertWorkflowRefusal(self, res):
        self.assertEqual(res.code, 1, res)
        self.assertIn(".github/workflows", res.message)
        self.assertIn("workflows permission", res.message)
        self.assertNotIn(("POST", "/app/installations/77/access_tokens"), self.api_paths())
        self.assertEqual(self.api_paths(), [])
        self.assertEqual(self.remote_branches(), {})

    def test_a_new_workflow_file_is_refused_before_any_call(self):
        self.start()
        self.commit_file(".github/workflows/ci.yml")
        self.assertWorkflowRefusal(self.go())

    def test_editing_deleting_and_nested_workflow_files_are_refused(self):
        self.start()
        git(self.wt, "checkout", "-q", "main")
        self.commit_file(".github/workflows/old.yml")
        git(self.wt, "checkout", "-q", "agent/5-fix-thing")
        git(self.wt, "rebase", "-q", "main")
        git(self.wt, "rm", "-q", ".github/workflows/old.yml")
        git(self.wt, "commit", "-q", "-m", "delete a workflow")
        self.assertWorkflowRefusal(self.go())
        self.commit_file(".github/workflows/sub/deep.yml")
        self.assertWorkflowRefusal(self.go())

    def test_a_case_variant_of_the_folder_is_refused(self):
        self.start()
        self.commit_file(".GitHub/Workflows/ci.yml")
        self.assertWorkflowRefusal(self.go())

    def test_other_github_files_and_lookalikes_are_fine(self):
        self.start()
        self.commit_file(".github/ISSUE_TEMPLATE/bug.md")
        self.commit_file(".github/workflows-notes.md")
        self.commit_file("docs/.github/workflows/example.yml")
        res = self.go()
        self.assertEqual(res.code, 0, res)

    def test_a_workflow_file_already_on_the_base_is_not_this_branchs_change(self):
        self.start()
        git(self.wt, "checkout", "-q", "main")
        self.commit_file(".github/workflows/ci.yml")
        git(self.wt, "checkout", "-q", "agent/5-fix-thing")
        git(self.wt, "rebase", "-q", "main")
        res = self.go()
        self.assertEqual(res.code, 0, res)

    def test_without_a_base_branch_to_compare_the_remote_is_left_to_decide(self):
        self.start()
        self.commit_file(".github/workflows/ci.yml")
        git(self.wt, "branch", "-q", "-D", "main")
        res = self.go()  # the local bare remote has no such rule, so this goes through
        self.assertEqual(res.code, 0, res)


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
                               env=GIT_ENV, stdin=subprocess.DEVNULL)
        self.assertTrue(marker.exists(), "the trap must fire on a naive status, or this test proves nothing")
        marker.unlink()

        res = self.go()
        self.assertEqual(res.code, 0, res)
        self.assertFalse(marker.exists(), "something in the agent's repo ran as the wrapper")
        self.assertEqual(self.remote_branches(), {"refs/heads/agent/5-fix-thing": self.sha})


class UntrustedFiles(Base):
    """What the wrapper reads by a path the agent can influence: the index of its worktree and the
    object store of its repository. And what it will not put in a pull request."""

    def make_plain_file(self, content=b"data"):
        p = self.root / "plain"
        p.write_bytes(content)
        return str(p)

    def test_open_regular_takes_a_plain_file(self):
        fd, st = push_wrapper.open_regular(self.make_plain_file(), "the file", 100)
        os.close(fd)
        self.assertEqual(st.st_nlink, 1)

    def test_open_regular_refuses_a_symlink(self):
        link = self.root / "link"
        link.symlink_to(self.key)
        with self.assertRaises(push_wrapper.Refused):
            push_wrapper.open_regular(str(link), "the file", 10_000)

    def test_open_regular_refuses_a_hard_link(self):
        plain = self.make_plain_file()
        os.link(plain, str(self.root / "second-name"))
        with self.assertRaises(push_wrapper.Refused):
            push_wrapper.open_regular(plain, "the file", 100)

    def test_open_regular_refuses_a_pipe_a_device_and_a_folder_without_blocking(self):
        fifo = self.root / "fifo"
        os.mkfifo(str(fifo))
        for path in (str(fifo), "/dev/zero", str(self.root)):
            with self.assertRaises(push_wrapper.Refused, msg=path):
                push_wrapper.open_regular(path, "the file", 100)

    def test_open_regular_refuses_a_big_file_and_a_missing_one(self):
        with self.assertRaises(push_wrapper.Refused):
            push_wrapper.open_regular(self.make_plain_file(b"x" * 101), "the file", 100)
        with self.assertRaises(push_wrapper.Refused):
            push_wrapper.open_regular(str(self.root / "nope"), "the file", 100)

    def test_a_symlinked_index_is_refused_and_the_key_is_not_copied(self):
        self.start()
        index = self.wt / ".git" / "index"
        index.unlink()
        index.symlink_to(self.key)
        self.assertRefused(self.go(), "index")

    def test_a_hard_linked_index_is_refused(self):
        self.start()
        os.link(str(self.wt / ".git" / "index"), str(self.root / "elsewhere"))
        self.assertRefused(self.go(), "index")

    def test_an_index_over_the_cap_is_refused(self):
        self.start()
        old = push_wrapper.INDEX_MAX
        push_wrapper.INDEX_MAX = 10
        self.addCleanup(setattr, push_wrapper, "INDEX_MAX", old)
        self.assertRefused(self.go(), "index")

    def test_an_index_that_is_a_device_is_refused_without_reading_it(self):
        self.start()
        index = self.wt / ".git" / "index"
        index.unlink()
        index.symlink_to("/dev/zero")  # a plain copyfile of this would fill the disk
        self.assertRefused(self.go(), "index")

    def test_nested_alternates_are_refused(self):
        for name in ("alternates", "http-alternates"):
            with self.subTest(name):
                self.start()
                info = self.wt / ".git" / "objects" / "info"
                info.mkdir(exist_ok=True)
                (info / name).write_text(str(self.root) + "\n")
                self.assertRefused(self.go(), "borrows objects")
                (info / name).unlink()

    def test_an_objects_folder_that_is_a_link_is_refused(self):
        self.start()
        objects = self.wt / ".git" / "objects"
        real = self.root / "real-objects"
        objects.rename(real)
        objects.symlink_to(real)
        self.assertRefused(self.go(), "objects")

    def test_a_clean_repository_with_no_alternates_still_passes(self):
        self.start()
        self.assertEqual(self.go().code, 0)

    def pr_bodies(self):
        return [json.loads(r["body"])["body"] for r in self.srv.requests if r["path"] == "/repos/o/r/pulls"]

    def test_a_body_with_a_private_key_is_refused_before_any_call(self):
        self.start()
        for marker in ("-----BEGIN PRIVATE KEY-----", "-----BEGIN RSA PRIVATE KEY-----",
                       "-----BEGIN OPENSSH PRIVATE KEY-----", "-----BEGIN EC PRIVATE KEY-----"):
            self.assertRefused(self.go(body=("notes\n%s\nabc\n" % marker).encode()), "private key")
        self.assertRefused(self.go(body=b"notes\n" + self.key.read_bytes()), "private key")
        self.assertEqual(self.pr_bodies(), [])

    def test_a_body_with_the_installation_token_is_refused_before_the_push(self):
        self.start()
        for text in (TOKEN, base64.b64encode(("x-access-token:" + TOKEN).encode()).decode()):
            res = self.go(body=("see %s here" % text).encode())
            self.assertEqual(res.code, 1, res)
            self.assertIn("token", res.message)
            self.assertNotIn(TOKEN, res.message)
            self.assertEqual(self.remote_branches(), {})
        self.assertNotIn(("POST", "/repos/o/r/pulls"), self.api_paths())


class Cli(Base):
    def run_main(self, argv, env=None, stdin=None):
        out, err = io.StringIO(), io.StringIO()
        code = push_wrapper.main(argv, env or {}, out, err,
                                 stdin=io.BytesIO(self.body) if stdin is None else stdin)
        return code, out.getvalue(), err.getvalue()

    def test_usage_and_missing_config(self):
        self.assertEqual(self.run_main([])[0], 2)
        code, _, err = self.run_main(["--worktree", str(self.wt), "--issue", "5", "--title", "t",
                                      "--config", str(self.root / "no.json")])
        self.assertEqual(code, 2)
        self.assertIn("config", err)

    def test_the_body_is_read_from_stdin(self):
        self.start()
        path = self.root / "cfg.json"
        path.write_text(json.dumps({"app_id": 12345, "key_path": str(self.key), "repo": "o/r",
                                    "api_url": self.srv.url, "remote_url": str(self.bare)}))
        code, out, err = self.run_main(["--worktree", str(self.wt), "--issue", "5", "--title", "Fix",
                                        "--config", str(path)], stdin=io.BytesIO(b"piped words\n"))
        self.assertEqual(code, 0, err)
        pr = [r for r in self.srv.requests if r["path"] == "/repos/o/r/pulls"][0]
        self.assertIn("piped words", json.loads(pr["body"])["body"])

    def test_no_option_names_a_body_path(self):
        """The App key is a file this user can read; a `--body-file` that follows a link to it would post it.
        So there is no such option: the parser refuses it (exit 2) before anything is read or sent."""
        self.start()
        link = self.root / "body.md"
        link.symlink_to(self.key)
        path = self.root / "cfg.json"
        path.write_text(json.dumps({"app_id": 12345, "key_path": str(self.key), "repo": "o/r",
                                    "api_url": self.srv.url, "remote_url": str(self.bare)}))
        code, out, err = self.run_main(["--worktree", str(self.wt), "--issue", "5", "--title", "Fix",
                                        "--body-file", str(link), "--config", str(path)])
        self.assertEqual(code, 2)
        self.assertEqual(self.api_paths(), [])
        self.assertNotIn("PRIVATE KEY", out + err)

    def test_a_terminal_on_stdin_is_refused_not_waited_on(self):
        class Tty(io.BytesIO):
            def isatty(self):
                return True

        self.start()
        path = self.root / "cfg.json"
        path.write_text(json.dumps({"app_id": 12345, "key_path": str(self.key), "repo": "o/r",
                                    "api_url": self.srv.url, "remote_url": str(self.bare)}))
        code, _, err = self.run_main(["--worktree", str(self.wt), "--issue", "5", "--title", "Fix",
                                      "--config", str(path)], stdin=Tty(b"x"))
        self.assertEqual(code, 1)
        self.assertIn("stdin", err)
        self.assertEqual(self.api_paths(), [])

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
                                        "--agent-model", "m1", "--config", str(path)])
        self.assertEqual((code, out.strip()), (0, "https://github.com/o/r/pull/9"), err)
        self.assertNotIn(TOKEN, out + err)

    def test_refusal_exit_code_is_1(self):
        self.start()
        path = self.root / "cfg.json"
        path.write_text(json.dumps({"app_id": 12345, "key_path": str(self.key), "repo": "o/r",
                                    "api_url": self.srv.url, "remote_url": str(self.bare)}))
        git(self.wt, "checkout", "-q", "main")
        code, _, err = self.run_main(["--worktree", str(self.wt), "--issue", "5", "--title", "Fix",
                                      "--config", str(path)])
        self.assertEqual(code, 1)
        self.assertIn("refused", err)


if __name__ == "__main__":
    unittest.main()
