import hashlib
import os
import re
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
BASHES = sorted({p for p in ("/bin/bash", shutil.which("bash")) if p and os.path.exists(p)})
GOOD_SIG = "A" * 86 + "=="


def read(path):
    return "".join(l for l in (ROOT / path).read_text().splitlines(keepends=True) if not l.lstrip().startswith("#"))


def job_texts(text):
    block = text.split("\njobs:\n", 1)[1]
    parts = re.split(r"(?m)^  ([A-Za-z0-9_-]+):\n", block)
    return {parts[i]: parts[i + 1] for i in range(1, len(parts), 2)}


def steps_of(job):
    body = job.split("\n    steps:\n", 1)[1]
    return [s for s in re.split(r"(?m)^(?=      - )", body) if s.strip()]


def step_named(job, name):
    found = [s for s in steps_of(job) if re.search(r"(?m)^      - name: %s$" % re.escape(name), s)]
    assert len(found) == 1, f"{len(found)} steps named {name!r}"
    return found[0]


RUN_KEY = re.compile(r"^( *)(- )?run:[ \t]*(.*?)[ \t]*$")
BLOCK_HEADER = re.compile(r"^[|>](?:[+-]?[1-9]?|[1-9][+-])(?:[ \t]+#.*)?$")
STEP_KEY_COLUMN = 8


def run_blocks(text):
    lines = text.split("\n")
    blocks = []
    for i, line in enumerate(lines):
        m = RUN_KEY.match(line)
        if not m or len(m.group(1)) + (2 if m.group(2) else 0) != STEP_KEY_COLUMN:
            continue
        body = []
        for nxt in lines[i + 1:]:
            if nxt.strip() and len(nxt) - len(nxt.lstrip(" ")) <= STEP_KEY_COLUMN:
                break
            body.append(nxt)
        while body and not body[-1].strip():
            body.pop()
        value = m.group(3)
        if BLOCK_HEADER.match(value) or not value:
            blocks.append(textwrap.dedent("\n".join(body)))
        else:
            blocks.append(" ".join([value.strip("'\"")] + [b.strip() for b in body]))
    return blocks


def script_of(job, name):
    blocks = run_blocks(step_named(job, name))
    assert len(blocks) == 1, name
    return blocks[0]


def commands(text):
    return re.sub(r"\\\n\s*", " ", text)


def permissions_of(job):
    m = re.search(r"(?m)^    permissions:\n((?:      .*\n)+)", job)
    return dict(re.findall(r"      ([a-z-]+): (\w+)", m.group(1))) if m else None


def job_head(job):
    return job.split("\n    steps:\n", 1)[0]


REPO_SCRIPT = re.compile(r"tools/|legacy/|build\.sh|\.py\b|\./(?!bin/sign_update)")
FAKE_GH = """#!/bin/sh
# Stand-in for gh: logs its arguments; `release download` copies $GH_FIXTURE into --dir.
echo "$*" >> "$GH_LOG"
if [ "$1 $2" = "release download" ]; then
  while [ $# -gt 0 ]; do
    if [ "$1" = "--dir" ]; then dir="$2"; fi
    shift
  done
  mkdir -p "$dir" && cp "$GH_FIXTURE"/* "$dir"/
fi
if [ "$1 $2" = "attestation verify" ] && [ -n "${GH_ATTEST_FAIL:-}" ]; then exit 1; fi
exit 0
"""
SHA256SUM = '#!/bin/sh\nexec shasum -a 256 "$@"\n'


def write_exe(path, text):
    Path(path).write_text(text)
    os.chmod(path, 0o755)


def sha256_of(data):
    return hashlib.sha256(data).hexdigest()


class ScriptCase(unittest.TestCase):

    @classmethod
    def setUpClass(cls):
        cls.bin = Path(tempfile.mkdtemp(prefix="wf-bin-"))
        cls.addClassCleanup(shutil.rmtree, cls.bin, True)
        if shutil.which("sha256sum") is None:
            write_exe(cls.bin / "sha256sum", SHA256SUM)
        write_exe(cls.bin / "gh", FAKE_GH)

    def run_script(self, script, cwd, bash, **env):
        full = {"PATH": "%s:%s" % (self.bin, os.environ.get("PATH", os.defpath)), "HOME": str(cwd), "LC_ALL": "C"}
        full.update(env)
        return subprocess.run([bash, "-e", "-c", script], cwd=cwd, env=full, capture_output=True, text=True,
                              timeout=120, stdin=subprocess.DEVNULL)

    def fail_msg(self, r):
        return f"rc={r.returncode}\nstdout:\n{r.stdout}\nstderr:\n{r.stderr}"


class RunBlockReader(unittest.TestCase):
    SAMPLE = textwrap.dedent("""\
        jobs:
          j:
            outputs:
              run: ${{ steps.a.outputs.run }}
            steps:
              - name: a
                run: |
                  echo literal
                    echo indented

                  echo after blank
              - name: b
                run: |-
                  echo stripped
              - name: c
                run: >
                  echo folded
              - name: d
                run: >-
                  echo folded stripped
              - name: e
                run: echo one-liner
              - name: f
                run: "echo quoted"
              - run: |+
                  echo kept

              - name: g
                run: |2
                    echo explicit indent
              - name: h
                run: echo plain
                  continued
              - name: i
                uses: x/y@v1
                with:
                  script: not a run
        """)

    def test_every_style_is_captured_and_the_job_output_is_not(self):
        blocks = run_blocks(self.SAMPLE)
        self.assertEqual(len(blocks), 9, blocks)
        joined = "\n".join(blocks)
        for text in ("echo literal", "echo indented", "echo after blank", "echo stripped", "echo folded",
                     "echo folded stripped", "echo one-liner", "echo quoted", "echo kept", "echo explicit indent",
                     "echo plain continued"):
            self.assertIn(text, joined)
        self.assertNotIn("not a run", joined)
        self.assertNotIn("steps.a.outputs.run", joined)

    def test_a_chomped_block_is_still_scanned_for_expressions_and_scripts(self):
        text = self.SAMPLE.replace("echo stripped", "python3 tools/evil.py ${{ github.event.inputs.x }}")
        hits = [b for b in run_blocks(text) if "evil" in b]
        self.assertEqual(len(hits), 1)
        self.assertIn("${{", hits[0])
        self.assertIsNotNone(REPO_SCRIPT.search(hits[0]))


RELEASE = ".github/workflows/release.yml"
RUST = ".github/workflows/rust.yml"
FAKE_RELEASE_GH = """#!/bin/sh
echo "$*" >> "$GH_LOG"
case "$1 $2" in
  "api repos/owner/repo/git/ref/tags/v2.0.0")
    if [ -n "${GH_TAG_EXISTS:-}" ]; then echo '{}'; exit 0; fi
    echo "gh: Not Found (HTTP ${GH_API_STATUS:-404})" >&2
    exit 1;;
  "release view")
    case " ${GH_MISSING:-} " in *" $3 "*) exit 1;; esac
    exit 0;;
  "release download")
    while [ $# -gt 0 ]; do
      if [ "$1" = "--dir" ]; then dir="$2"; fi
      shift
    done
    mkdir -p "$dir" && cp "$GH_FIXTURE"/* "$dir"/
    exit 0;;
esac
exit 0
"""
GIT_ENV = {"GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com", "GIT_COMMITTER_NAME": "t",
           "GIT_COMMITTER_EMAIL": "t@example.com", "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull}
CHANGELOG = """# Changelog

## [Unreleased]

## [2.0.0] - 2026-11-20

### Added

- The Rust app.

## [1.2.0] - 2026-11-07

- Old.
"""
NODE_FILES = ("notchling-linux-x86_64", "notchling-linux-aarch64")


def workflow_env(path):
    pins = {}
    for block in re.finditer(r"(?m)^( *)env:\n((?:\1  .*\n)+)", read(path)):
        pins.update(re.findall(r'(?m)^\s+([A-Z_0-9]+): "([^"]*)"', block.group(2)))
    return pins



class ReleaseWorkflow(unittest.TestCase):

    def setUp(self):
        self.text = read(RELEASE)
        self.jobs = job_texts(self.text)

    def test_jobs(self):
        self.assertEqual(list(self.jobs), ["gate", "app", "nodes", "publish"])

    def test_trigger_is_a_merged_release_pull_request_into_main(self):
        raw = (ROOT / RELEASE).read_text()
        self.assertIn("name: Release\n", raw)
        self.assertRegex(raw, r"(?m)^on:\n  pull_request:\n    types: \[closed\]\n    branches: \[main\]\n")
        self.assertNotRegex(raw, r"(?m)^  (push|workflow_dispatch|schedule):")
        guard = re.search(r"(?m)^    if: (.*)$", self.jobs["gate"]).group(1)
        self.assertIn("github.event.pull_request.merged", guard)
        self.assertIn("startsWith(github.event.pull_request.head.ref, 'release/')", guard)
        self.assertIn("github.event.pull_request.head.repo.full_name == github.repository", guard)
        for name in ("app", "nodes", "publish"):
            self.assertNotRegex(job_head(self.jobs[name]), r"(?m)^    if:")
            self.assertIn("gate", re.search(r"(?m)^    needs: (.*)$", self.jobs[name]).group(1))

    def test_publish_waits_for_every_job(self):
        needs = re.search(r"(?m)^    needs: (.*)$", self.jobs["publish"]).group(1)
        self.assertEqual(sorted(re.findall(r"\w+", needs)), ["app", "gate", "nodes"])

    def test_top_level_permissions_are_empty_and_only_publish_writes(self):
        self.assertRegex(self.text, r"(?m)^permissions: \{\}\s*$")
        for name, job in self.jobs.items():
            self.assertEqual(permissions_of(job), {"contents": "write" if name == "publish" else "read"}, name)

    def test_one_concurrency_group_and_nothing_is_cancelled(self):
        self.assertIn("group: notchling-release", self.text)
        self.assertIn("cancel-in-progress: false", self.text)

    def test_no_secrets_no_environment(self):
        self.assertNotIn("secrets.", self.text)
        self.assertNotRegex(self.text, r"(?m)^\s+environment:")
        self.assertEqual(sorted(set(re.findall(r"\$\{\{\s*([a-z]+)\.", self.text))),
                         ["github", "needs", "steps"])

    def test_the_old_swift_flow_is_gone(self):
        self.assertNotIn("./build.sh", self.text)
        self.assertNotIn("build.sh --dmg", self.text)
        self.assertNotIn("legacy/macos", self.text)
        self.assertNotIn("NOTCHLING_RELEASES", self.text)

    def test_the_app_job_builds_with_bundle_next_and_no_flags(self):
        app = self.jobs["app"]
        self.assertIn("runs-on: macos-latest", app)
        blocks = run_blocks(app)
        self.assertIn("tools/bundle-next.sh", [b.strip() for b in blocks])
        self.assertNotIn("--install", app)
        self.assertNotIn("--nodes", app)

    def test_the_app_job_checks_the_bundle_version_and_a_clean_tree(self):
        script = script_of(self.jobs["app"], "Check and package")
        self.assertIn("CFBundleShortVersionString", script)
        self.assertIn('[ "$short" = "$VERSION" ]', script)
        self.assertIn("git diff --quiet HEAD", script)
        self.assertIn("git rev-parse --short=12 HEAD", script)
        self.assertIn("hdiutil create -volname Notchling", script)
        self.assertIn("-format UDZO", script)
        self.assertIn("ln -s /Applications", script)
        self.assertIn("ditto -c -k --keepParent", script)
        for key in ("build", "dmg_sha256", "zip_sha256"):
            self.assertRegex(self.jobs["app"], r"(?m)^      %s: \$\{\{ steps\.package\.outputs\.%s \}\}$" % (key, key))

    def test_the_nodes_job_pins_what_rust_yml_pins(self):
        rust = workflow_env(RUST)
        mine = workflow_env(RELEASE)
        for key in ("ZIG_VERSION", "ZIG_SHA256", "CARGO_ZIGBUILD_VERSION"):
            self.assertIn(key, rust)
            self.assertEqual(mine.get(key), rust[key], key)
        nodes = self.jobs["nodes"]
        self.assertIn("runs-on: ubuntu-latest", nodes)
        zig = script_of(nodes, "Install zig")
        rust_zig = script_of(job_texts(read(RUST))["node-builds"], "Install zig")
        self.assertEqual(zig, rust_zig)
        singles = [b.rstrip('"') for b in run_blocks(nodes)]
        self.assertIn('cargo install cargo-zigbuild --locked --version "$CARGO_ZIGBUILD_VERSION', singles)
        self.assertIn('test "$(zig version)" = "$ZIG_VERSION', singles)
        self.assertIn('NOTCHLING_DATA_ROOT="$RUNNER_TEMP/data" tools/build-node.sh', run_blocks(nodes))
        self.assertIn("rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl", run_blocks(nodes))

    def test_publish_adds_an_unsigned_stable_item_and_verifies_the_feed(self):
        script = commands(script_of(self.jobs["publish"], "Update the feed"))
        add = re.search(r"python3 tools/release/appcast\.py add [^\n]*", script).group(0)
        self.assertIn("--unsigned", add)
        self.assertNotIn("--ed-signature", add)
        self.assertNotIn("--channel", add)
        for flag in ('--version "$number"', '--short "$VERSION"', "--min-system 13.0", "--pub-date now", "--rollout 0",
                     '--length "$length"'):
            self.assertIn(flag, add)
        self.assertIn("releases/download/v$VERSION/Notchling-$VERSION.dmg", add)
        self.assertIn("releases/tag/v$VERSION", add)
        self.assertLess(script.index("appcast.py add"), script.index("appcast.py verify"))
        self.assertLess(script.index("appcast.py verify"), script.index("gh release upload appcast"))
        self.assertIn("--prerelease", script)
        self.assertIn('--title "Update feed"', script)

    def test_publish_creates_a_latest_release_on_the_gated_commit_with_every_asset(self):
        script = commands(script_of(self.jobs["publish"], "Create the release"))
        create = re.search(r"gh release create [^\n]*", script).group(0)
        for item in ("Notchling-$VERSION.dmg", "Notchling-$VERSION.zip", "notchling-linux-x86_64",
                     "notchling-linux-aarch64", "manifest.json", "SHA256SUMS", '--target "$SHA"', "--latest",
                     '--notes-file notes.md'):
            self.assertIn(item, create)
        self.assertNotIn("--prerelease", create)

    def test_every_file_is_checked_against_the_job_outputs_and_the_builds_must_match(self):
        script = script_of(self.jobs["publish"], "Verify the files")
        for name in ("DMG_SHA256", "ZIP_SHA256", "X86_64_SHA256", "AARCH64_SHA256"):
            self.assertIn("%s: ${{ needs." % name, self.jobs["publish"])
            self.assertIn('= "$%s"' % name, script)
        self.assertIn('[ "$APP_BUILD" = "$NODES_BUILD" ]', script)
        self.assertIn('case "$SHA" in "$APP_BUILD"*)', script)
        self.assertIn('manifest["build"] == build', script)
        self.assertIn("sha256sum \"Notchling-$VERSION.dmg\"", script)

    def test_every_action_is_pinned_to_a_commit_and_allowlisted(self):
        allow = (ROOT / "tools/guard/actions-allowlist.json").read_text()
        used = 0
        for line in self.text.splitlines():
            m = re.match(r"^\s*(?:-\s+)?uses:\s*(\S+)(.*)$", line)
            if not m:
                continue
            used += 1
            self.assertRegex(m.group(1), r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+@[0-9a-f]{40}$", line)
            self.assertRegex(m.group(2), r"^\s+# v\d+\s*$", line)
            self.assertIn('"%s"' % m.group(1).split("@")[0], allow)
        self.assertGreaterEqual(used, 8)

    def test_every_checkout_is_the_gated_commit_without_credentials(self):
        for name, job in self.jobs.items():
            for step in steps_of(job):
                if "actions/checkout@" not in step:
                    continue
                refs = re.findall(r"(?m)^\s+ref: (.*?)\s*$", step)
                expected = "${{ steps.event.outputs.sha }}" if name == "gate" else "${{ needs.gate.outputs.sha }}"
                self.assertEqual(refs, [expected], name)
                self.assertIn("persist-credentials: false", step, name)

    def test_event_fields_never_reach_a_script_and_appear_only_in_env_or_if(self):
        for block in run_blocks(self.text):
            self.assertNotIn("${{", block)
        for line in self.text.splitlines():
            if "github.event" in line:
                self.assertTrue(re.match(r"^\s+(if: |[A-Z_]+: \$\{\{ )", line), line.strip())

    def test_artifacts_travel_under_two_names(self):
        self.assertEqual(re.findall(r"(?m)^          name: (\w+)$", self.jobs["app"]), ["mac"])
        self.assertEqual(re.findall(r"(?m)^          name: (\w+)$", self.jobs["nodes"]), ["nodes"])
        self.assertEqual(re.findall(r"(?m)^          name: (\w+)$", self.jobs["publish"]), ["mac", "nodes"])

    def test_the_build_jobs_get_no_token_at_all(self):
        for name in ("app", "nodes"):
            self.assertNotIn("github.token", self.jobs[name])
            self.assertNotIn("GH_TOKEN", self.jobs[name])


class ReleaseScripts(ScriptCase):
    SHA_LEN = 40

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        write_exe(cls.bin / "gh", FAKE_RELEASE_GH)

    def setUp(self):
        self.jobs = job_texts(read(RELEASE))
        self.event = script_of(self.jobs["gate"], "Read the event")
        self.check = script_of(self.jobs["gate"], "Check the version")
        self.verify = script_of(self.jobs["publish"], "Verify the files")
        raw = job_texts((ROOT / RELEASE).read_text())["publish"]
        self.notes = script_of(raw, "Write the release notes")
        self.create = script_of(self.jobs["publish"], "Create the release")
        self.feed = script_of(self.jobs["publish"], "Update the feed")

    def git(self, repo, *args):
        r = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True, env=dict(os.environ, **GIT_ENV),
                           stdin=subprocess.DEVNULL)
        self.assertEqual(r.returncode, 0, r.stderr)
        return r.stdout.strip()

    def make_repo(self, tmp, cargo=None, changelog=CHANGELOG):
        repo = Path(tmp) / "repo"
        repo.mkdir()
        if cargo is None:
            cargo = '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "2.0.0"\nedition = "2021"\n\n[workspace.dependencies]\nversion = "9.9.9"\n'
        (repo / "Cargo.toml").write_text(cargo)
        (repo / "CHANGELOG.md").write_text(changelog)
        self.git(repo, "init", "-q", "-b", "main")
        self.git(repo, "add", "-A")
        self.git(repo, "commit", "-q", "-m", "x")
        return repo, self.git(repo, "rev-parse", "HEAD")

    def run_gh_script(self, script, bash, cwd, **env):
        log = Path(cwd) / "gh.log"
        base = {"GH_LOG": str(log), "GH_REPO": "owner/repo", "GH_TOKEN": "t", "GITHUB_OUTPUT": str(Path(cwd) / "out")}
        base.update(env)
        Path(base["GITHUB_OUTPUT"]).write_text("")
        r = self.run_script(script, cwd, bash, **base)
        return r, Path(base["GITHUB_OUTPUT"]).read_text(), (log.read_text().splitlines() if log.exists() else [])

    def run_check(self, bash, tmp, repo, sha, **over):
        env = {"SHA": sha, "HEAD_REF": "release/2.0.0"}
        env.update(over)
        return self.run_gh_script(self.check, bash, repo, **env)

    def test_the_event_step_takes_a_40_hex_commit_and_a_release_branch_only(self):
        good = "a" * 40
        for bash in BASHES:
            for label, over, ok in (("good", {}, True),
                                    ("short sha", {"MERGE_SHA": "a" * 39}, False),
                                    ("upper sha", {"MERGE_SHA": "A" * 40}, False),
                                    ("two lines", {"MERGE_SHA": good + "\n" + good}, False),
                                    ("empty sha", {"MERGE_SHA": ""}, False),
                                    ("wrong branch", {"HEAD_REF": "release/2.0"}, False),
                                    ("not release", {"HEAD_REF": "feature/2.0.0"}, False),
                                    ("injected", {"HEAD_REF": "release/2.0.0$(id)"}, False),
                                    ("two-line branch", {"HEAD_REF": "release/2.0.0\nrelease/9.9.9"}, False)):
                with self.subTest(bash=bash, case=label), tempfile.TemporaryDirectory() as tmp:
                    env = {"MERGE_SHA": good, "HEAD_REF": "release/2.0.0"}
                    env.update(over)
                    r, out, _ = self.run_gh_script(self.event, bash, tmp, **env)
                    self.assertEqual(r.returncode == 0, ok, self.fail_msg(r))
                    self.assertEqual(out, "sha=%s\n" % good if ok else "")

    def test_a_matching_branch_changelog_and_a_missing_tag_pass_and_print_the_version(self):
        for bash in BASHES:
            with self.subTest(bash=bash), tempfile.TemporaryDirectory() as tmp:
                repo, sha = self.make_repo(tmp)
                r, out, log = self.run_check(bash, tmp, repo, sha)
                self.assertEqual(r.returncode, 0, self.fail_msg(r))
                self.assertEqual(out, "version=2.0.0\n")
                self.assertEqual(log, ["api repos/owner/repo/git/ref/tags/v2.0.0"])

    def test_the_gate_refuses_what_does_not_agree_with_cargo_toml(self):
        no_ws = '[package]\nversion = "2.0.0"\n'
        bad_ver = '[workspace.package]\nversion = "2.0"\n'
        for bash in BASHES:
            cases = (
                ("branch for another version", {}, {"HEAD_REF": "release/2.0.1"}),
                ("version not in the workspace table", {"cargo": no_ws}, {}),
                ("two part version", {"cargo": bad_ver}, {}),
                ("no changelog section", {"changelog": CHANGELOG.replace("## [2.0.0]", "## [2.0.1]")}, {}),
                ("changelog section only a prefix", {"changelog": CHANGELOG.replace("## [2.0.0]", "## [2.0.0-beta.1]")}, {}),
                ("changelog section with a wildcard match", {"changelog": CHANGELOG.replace("## [2.0.0]", "## [2x0x0]")}, {}),
                ("tag exists", {}, {"GH_TAG_EXISTS": "1"}),
                ("tag lookup fails for another reason", {}, {"GH_API_STATUS": "500"}),
                ("checkout is another commit", {}, {"SHA": "b" * 40}),
            )
            for label, repo_args, over in cases:
                with self.subTest(bash=bash, case=label), tempfile.TemporaryDirectory() as tmp:
                    repo, sha = self.make_repo(tmp, **repo_args)
                    r, out, _ = self.run_check(bash, tmp, repo, over.pop("SHA", sha), **over)
                    self.assertNotEqual(r.returncode, 0, self.fail_msg(r))
                    self.assertEqual(out, "")

    def test_the_workspace_version_is_the_one_read_not_another_table_s(self):
        cargo = '[workspace.dependencies]\nversion = "9.9.9"\n\n[workspace.package]\nversion = "2.0.0"\n\n[profile.release]\nversion = "8.8.8"\n'
        for bash in BASHES:
            with self.subTest(bash=bash), tempfile.TemporaryDirectory() as tmp:
                repo, sha = self.make_repo(tmp, cargo=cargo)
                r, out, _ = self.run_check(bash, tmp, repo, sha)
                self.assertEqual(r.returncode, 0, self.fail_msg(r))
                self.assertEqual(out, "version=2.0.0\n")

    def test_the_notes_are_the_changelog_section_then_the_install_text(self):
        for bash in BASHES:
            with self.subTest(bash=bash), tempfile.TemporaryDirectory() as tmp:
                repo, _ = self.make_repo(tmp)
                r = self.run_script(self.notes, repo, bash, VERSION="2.0.0")
                self.assertEqual(r.returncode, 0, self.fail_msg(r))
                notes = (repo / "notes.md").read_text()
                self.assertIn("- The Rust app.", notes)
                self.assertNotIn("Old.", notes)
                self.assertNotIn("## [", notes)
                head, install = notes.split("## Install")
                self.assertLess(head.index("The Rust app."), len(head))
                for needle in ("Notchling-2.0.0.dmg", "Apple silicon, macOS 13 or later", "Open Anyway",
                               "xattr -dr com.apple.quarantine /Applications/Notchling.app", "isn't notarized",
                               "The zip holds the same app.", "Add Machine and Update Machines", "manifest.json",
                               "SHA256SUMS lists every file"):
                    self.assertIn(needle, install)

    def test_an_empty_changelog_section_is_refused(self):
        for bash in BASHES:
            with self.subTest(bash=bash), tempfile.TemporaryDirectory() as tmp:
                repo, _ = self.make_repo(tmp, changelog="## [Unreleased]\n\n## [2.0.0] - 2026-11-20\n\n## [1.2.0] - x\n\n- Old.\n")
                r = self.run_script(self.notes, repo, bash, VERSION="2.0.0")
                self.assertNotEqual(r.returncode, 0)
                self.assertFalse((repo / "notes.md").exists())

    def build_files(self, tmp, build="abcdef012345", tamper=None, manifest_build=None, extra=None):
        root = Path(tmp)
        (root / "mac").mkdir()
        (root / "nodes").mkdir()
        data = {"mac/Notchling-2.0.0.dmg": b"dmg", "mac/Notchling-2.0.0.zip": b"zip",
                "nodes/notchling-linux-x86_64": b"x86", "nodes/notchling-linux-aarch64": b"arm"}
        for name, content in data.items():
            (root / name).write_bytes(content)
        files = {n: {"sha256": sha256_of(data["nodes/" + n]), "size": len(data["nodes/" + n])} for n in NODE_FILES}
        import json
        (root / "nodes" / "manifest.json").write_text(json.dumps({"build": manifest_build or build, "files": files}))
        if extra:
            (root / extra).write_bytes(b"x")
        env = {"VERSION": "2.0.0", "SHA": ("abcdef012345" + "0" * 28), "APP_BUILD": build, "NODES_BUILD": build,
               "DMG_SHA256": sha256_of(b"dmg"), "ZIP_SHA256": sha256_of(b"zip"),
               "X86_64_SHA256": sha256_of(b"x86"), "AARCH64_SHA256": sha256_of(b"arm")}
        if tamper:
            (root / tamper).write_bytes(b"tampered")
        return env

    def test_verified_files_are_listed_in_sha256sums_with_all_five_assets(self):
        for bash in BASHES:
            with self.subTest(bash=bash), tempfile.TemporaryDirectory() as tmp:
                env = self.build_files(tmp)
                (Path(tmp) / "assets" / "models").mkdir(parents=True)
                r = self.run_script(self.verify, tmp, bash, **env)
                self.assertEqual(r.returncode, 0, self.fail_msg(r))
                listed = [l.split("  ")[1] for l in (Path(tmp) / "upload" / "SHA256SUMS").read_text().splitlines()]
                self.assertEqual(sorted(listed), sorted(["Notchling-2.0.0.dmg", "Notchling-2.0.0.zip", "manifest.json",
                                                         *NODE_FILES]))
                self.assertEqual(sorted(p.name for p in (Path(tmp) / "upload").iterdir()), sorted(listed + ["SHA256SUMS"]))
                check = subprocess.run(["sha256sum" if shutil.which("sha256sum") else "shasum", *([] if shutil.which("sha256sum") else ["-a", "256"]), "-c", "SHA256SUMS"],
                                       cwd=Path(tmp) / "upload", capture_output=True, text=True)
                self.assertEqual(check.returncode, 0, check.stderr)

    def test_changed_files_mismatched_builds_and_odd_values_stop_the_release(self):
        sha_other = "1" * 40
        cases = (
            ("changed dmg", {"tamper": "mac/Notchling-2.0.0.dmg"}, {}),
            ("changed zip", {"tamper": "mac/Notchling-2.0.0.zip"}, {}),
            ("changed x86_64 binary", {"tamper": "nodes/notchling-linux-x86_64"}, {}),
            ("changed aarch64 binary", {"tamper": "nodes/notchling-linux-aarch64"}, {}),
            ("manifest of another build", {"manifest_build": "fedcba543210"}, {}),
            ("nodes of another build", {}, {"NODES_BUILD": "fedcba543210"}),
            ("build is not the start of the commit", {}, {"SHA": sha_other}),
            ("dirty build id", {"build": "abcdef012345-dirty"}, {}),
            ("extra file in an artifact", {"extra": "mac/extra.sh"}, {}),
            ("bad hash output", {}, {"DMG_SHA256": "xyz"}),
            ("bad version", {}, {"VERSION": "2.0"}),
        )
        for bash in BASHES:
            for label, build_args, over in cases:
                with self.subTest(bash=bash, case=label), tempfile.TemporaryDirectory() as tmp:
                    env = self.build_files(tmp, **build_args)
                    env.update(over)
                    r = self.run_script(self.verify, tmp, bash, **env)
                    self.assertNotEqual(r.returncode, 0, self.fail_msg(r))
                    self.assertFalse((Path(tmp) / "upload").exists(), label)

    def release_assets(self, tmp, dmg=b"dmg"):
        assets = Path(tmp) / "upload"
        assets.mkdir()
        (assets / "Notchling-2.0.0.dmg").write_bytes(dmg)
        for name in ("Notchling-2.0.0.zip", "notchling-linux-x86_64", "notchling-linux-aarch64", "manifest.json", "SHA256SUMS"):
            (assets / name).write_bytes(b"x")
        (Path(tmp) / "notes.md").write_text("notes")

    def test_a_new_release_is_created_stable_on_the_gated_commit(self):
        for bash in BASHES:
            with self.subTest(bash=bash), tempfile.TemporaryDirectory() as tmp:
                self.release_assets(tmp)
                sha = "c" * 40
                r, _, log = self.run_gh_script(self.create, bash, tmp, VERSION="2.0.0", SHA=sha, GH_MISSING="v2.0.0")
                self.assertEqual(r.returncode, 0, self.fail_msg(r))
                create = [l for l in log if l.startswith("release create")]
                self.assertEqual(len(create), 1)
                self.assertIn("create v2.0.0 upload/Notchling-2.0.0.dmg upload/Notchling-2.0.0.zip", create[0])
                self.assertIn("--target %s --latest --title Notchling 2.0.0 --notes-file notes.md" % sha, create[0])

    def test_a_rerun_carries_on_only_when_the_release_holds_the_same_disk_image(self):
        for bash in BASHES:
            for same, ok in ((b"dmg", True), (b"other", False)):
                with self.subTest(bash=bash, same=ok), tempfile.TemporaryDirectory() as tmp:
                    self.release_assets(tmp)
                    fixture = Path(tmp) / "fixture"
                    fixture.mkdir()
                    (fixture / "Notchling-2.0.0.dmg").write_bytes(same)
                    r, _, log = self.run_gh_script(self.create, bash, tmp, VERSION="2.0.0", SHA="c" * 40,
                                                   GH_FIXTURE=str(fixture))
                    self.assertEqual(r.returncode == 0, ok, self.fail_msg(r))
                    self.assertEqual([l for l in log if l.startswith("release create")], [])

    def feed_setup(self, tmp):
        tools = Path(tmp) / "tools" / "release"
        tools.mkdir(parents=True)
        shutil.copy(ROOT / "tools" / "release" / "appcast.py", tools / "appcast.py")
        assets = Path(tmp) / "upload"
        assets.mkdir()
        (assets / "Notchling-2.0.0.dmg").write_bytes(b"d" * 123)

    def test_the_stable_feed_item_is_unsigned_unchanneled_and_numbered_like_version_py(self):
        sys.path.insert(0, str(ROOT / "tools" / "release"))
        import version
        want = version.build_number(version.parse_tag("v2.0.0"))
        self.assertEqual(want, 2000099)
        for bash in BASHES:
            with self.subTest(bash=bash), tempfile.TemporaryDirectory() as tmp:
                self.feed_setup(tmp)
                r, _, log = self.run_gh_script(self.feed, bash, tmp, VERSION="2.0.0", SHA="c" * 40, GH_MISSING="appcast")
                self.assertEqual(r.returncode, 0, self.fail_msg(r))
                text = (Path(tmp) / "feed" / "appcast.xml").read_text()
                self.assertIn("<sparkle:version>%d</sparkle:version>" % want, text)
                self.assertIn("<sparkle:shortVersionString>2.0.0</sparkle:shortVersionString>", text)
                self.assertIn('url="https://github.com/owner/repo/releases/download/v2.0.0/Notchling-2.0.0.dmg"', text)
                self.assertIn('length="123"', text)
                self.assertIn("<sparkle:minimumSystemVersion>13.0</sparkle:minimumSystemVersion>", text)
                self.assertIn("releases/tag/v2.0.0</sparkle:releaseNotesLink>", text)
                self.assertNotIn("edSignature", text)
                self.assertNotIn("sparkle:channel", text)
                self.assertNotIn("phasedRolloutInterval", text)
                self.assertIn("<pubDate>", text)
                self.assertEqual([l for l in log if l.startswith("release create")],
                                 ["release create appcast feed/appcast.xml --target %s --prerelease --title Update feed "
                                  "--notes The update feed (appcast.xml). Edited by the release workflows; do not delete."
                                  % ("c" * 40)])

    def test_a_rerun_leaves_a_feed_that_has_the_version_alone_and_uploads_over_the_old_asset(self):
        for bash in BASHES:
            with self.subTest(bash=bash), tempfile.TemporaryDirectory() as tmp:
                self.feed_setup(tmp)
                fixture = Path(tmp) / "fixture"
                fixture.mkdir()
                first, _, _ = self.run_gh_script(self.feed, bash, tmp, VERSION="2.0.0", SHA="c" * 40, GH_MISSING="appcast")
                self.assertEqual(first.returncode, 0, self.fail_msg(first))
                shutil.copy(Path(tmp) / "feed" / "appcast.xml", fixture / "appcast.xml")
                shutil.rmtree(Path(tmp) / "feed")
                (Path(tmp) / "gh.log").unlink()
                before = (fixture / "appcast.xml").read_text()
                r, _, log = self.run_gh_script(self.feed, bash, tmp, VERSION="2.0.0", SHA="c" * 40, GH_FIXTURE=str(fixture))
                self.assertEqual(r.returncode, 0, self.fail_msg(r))
                self.assertEqual((Path(tmp) / "feed" / "appcast.xml").read_text(), before)
                self.assertIn("release upload appcast feed/appcast.xml --clobber", log)
                self.assertEqual([l for l in log if l.startswith("release create")], [])

    def test_a_new_version_is_added_on_top_of_an_existing_feed(self):
        for bash in BASHES:
            with self.subTest(bash=bash), tempfile.TemporaryDirectory() as tmp:
                self.feed_setup(tmp)
                fixture = Path(tmp) / "fixture"
                fixture.mkdir()
                feed = Path(tmp) / "old.xml"
                subprocess.run([sys.executable, str(ROOT / "tools/release/appcast.py"), "add", "--feed", str(feed),
                                "--version", "1020099", "--short", "1.2.0", "--url", "https://github.com/o/r/d.dmg",
                                "--length", "5", "--ed-signature", GOOD_SIG, "--min-system", "13.0",
                                "--notes-url", "https://github.com/o/r/t"], check=True, capture_output=True)
                shutil.copy(feed, fixture / "appcast.xml")
                r, _, _ = self.run_gh_script(self.feed, bash, tmp, VERSION="2.0.0", SHA="c" * 40, GH_FIXTURE=str(fixture))
                self.assertEqual(r.returncode, 0, self.fail_msg(r))
                text = (Path(tmp) / "feed" / "appcast.xml").read_text()
                self.assertLess(text.index("2000099"), text.index("1020099"))
                self.assertEqual(text.count("edSignature"), 1)

    def test_the_feed_step_refuses_a_version_version_py_would_refuse(self):
        for bash in BASHES:
            for bad in ("2.0", "2.0.0\n2.0.1", "2.0.100", "$(id)", "2.0.0-beta.1"):
                with self.subTest(bash=bash, version=bad), tempfile.TemporaryDirectory() as tmp:
                    self.feed_setup(tmp)
                    r, _, log = self.run_gh_script(self.feed, bash, tmp, VERSION=bad, SHA="c" * 40, GH_MISSING="appcast")
                    self.assertNotEqual(r.returncode, 0, self.fail_msg(r))
                    self.assertEqual([l for l in log if l.startswith("release")], [])


if __name__ == "__main__":
    unittest.main()
