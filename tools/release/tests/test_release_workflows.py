import hashlib
import os
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
BETA = ".github/workflows/beta.yml"
PROMOTE = ".github/workflows/promote.yml"
BOTH = (BETA, PROMOTE)
SIGNING_SECRETS = ("APPLE_CERT_P12", "APPLE_CERT_PASSWORD", "APPLE_ID", "APPLE_TEAM_ID", "APPLE_APP_PASSWORD",
                   "SPARKLE_ED_PRIVATE_KEY")
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
SOURCING = (re.compile(r"(?<![\w-])(source|eval)(?=\s)"),
            re.compile(r"(?m)(?:^|[;&|(])[ \t]*\.[ \t]+\S"),
            re.compile(r"(?m)(?:^|[;&|(])[ \t]*(?:ba)?sh[ \t]+(?!-)\S"))
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


def sums_line(path):
    return "%s  %s\n" % (hashlib.sha256(Path(path).read_bytes()).hexdigest(), Path(path).name)


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


class WorkflowContract(unittest.TestCase):
    def test_top_level_permissions_are_empty(self):
        for f in BOTH:
            self.assertRegex(read(f), r"(?m)^permissions: \{\}\s*$", f)

    def test_every_job_sets_its_own_permissions(self):
        for f in BOTH:
            for name, job in job_texts(read(f)).items():
                self.assertIsNotNone(permissions_of(job), f"{f} {name}")

    def test_kill_switch_is_the_first_step_of_every_job(self):
        for f in BOTH:
            for name, job in job_texts(read(f)).items():
                first = steps_of(job)[0]
                where = f"{f} {name}"
                self.assertRegex(first, r"- name: Kill switch\n", where)
                self.assertIn("NOTCHLING_RELEASES: ${{ vars.NOTCHLING_RELEASES }}", first, where)
                self.assertIn('[ "$NOTCHLING_RELEASES" = "on" ] ||', first, where)
                self.assertIn("exit 1", first, where)
                self.assertNotIn("uses:", first, where)

    def test_kill_switch_script_passes_only_on_exactly_on(self):
        script = run_blocks(steps_of(job_texts(read(BETA))["gate"])[0])[0]
        for bash in BASHES:
            for value, ok in (("on", True), ("", False), ("off", False), ("ON", False), ("on ", False),
                              ("true", False)):
                with self.subTest(bash=bash, value=value):
                    env = {k: v for k, v in os.environ.items() if k != "NOTCHLING_RELEASES"}
                    env["NOTCHLING_RELEASES"] = value
                    r = subprocess.run([bash, "-c", script], env=env, capture_output=True, text=True, timeout=30,
                                       stdin=subprocess.DEVNULL)
                    self.assertEqual(r.returncode == 0, ok, r.stderr)
            with self.subTest(bash=bash, value="unset"):
                env = {k: v for k, v in os.environ.items() if k != "NOTCHLING_RELEASES"}
                r = subprocess.run([bash, "-u", "-c", script], env=env, capture_output=True, text=True, timeout=30,
                                   stdin=subprocess.DEVNULL)
                self.assertNotEqual(r.returncode, 0)

    def test_event_fields_never_appear_inside_run(self):
        for f in BOTH:
            text = read(f)
            for block in run_blocks(text):
                self.assertNotIn("${{", block, f)

    def test_event_and_input_fields_reach_the_shell_only_as_env_values(self):
        for f in BOTH:
            for line in read(f).splitlines():
                if re.search(r"\$\{\{\s*(github\.event|github\.ref|github\.sha|github\.triggering_actor|inputs\.)", line):
                    ok = re.match(r"^\s+[A-Z_]+: \$\{\{ ", line) or re.match(r"^\s+ref: \$\{\{ ", line) \
                        or re.match(r"^\s+(if|run): ", line) is None and re.match(r"^\s+[a-z_]+: \$\{\{ ", line)
                    self.assertTrue(ok, f"{f}: {line.strip()}")

    def test_actions_are_pinned_with_a_version_comment(self):
        for f in BOTH:
            for line in read(f).splitlines():
                m = re.match(r"^\s*(?:-\s+)?uses:\s*(\S+)(.*)$", line)
                if m:
                    self.assertRegex(m.group(1), r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+@[0-9a-f]{40}$", f)
                    self.assertRegex(m.group(2), r"^\s+# v\d+\s*$", f"{f}: {line.strip()}")

    def test_new_actions_are_allowlisted(self):
        allow = (ROOT / "tools/guard/actions-allowlist.json").read_text()
        for repo in ("actions/upload-artifact", "actions/download-artifact", "actions/attest-build-provenance",
                     "actions/checkout"):
            self.assertIn('"%s"' % repo, allow)

    def test_both_workflows_share_one_concurrency_group_and_never_cancel(self):
        for f in BOTH:
            text = read(f)
            self.assertIn("group: notchling-release", text, f)
            self.assertIn("cancel-in-progress: false", text, f)

    def test_runs_only_from_the_default_branch(self):
        for f, job in ((BETA, "gate"), (PROMOTE, "authorize")):
            text = job_texts(read(f))[job]
            self.assertIn('[ "$RUN_REF" = "refs/heads/$DEFAULT_BRANCH" ]', text, f)
            self.assertIn("RUN_REF: ${{ github.ref }}", text, f)

    def test_the_ref_is_checked_before_the_run_commit_is_checked_out(self):
        for f, job in ((BETA, "gate"), (PROMOTE, "authorize")):
            steps = steps_of(job_texts(read(f))[job])
            ref = next(i for i, s in enumerate(steps) if '[ "$RUN_REF" = "refs/heads/$DEFAULT_BRANCH" ]' in s)
            checkout = next(i for i, s in enumerate(steps) if "actions/checkout@" in s)
            self.assertLess(ref, checkout, f)
            self.assertNotIn("uses:", steps[ref], f)
            self.assertIn("ref: ${{ github.sha }}", steps[checkout], f)
            later = "".join(steps[checkout + 1:])
            self.assertIn("RUN_SHA: ${{ github.sha }}", later, f)
            self.assertIn('[ "$(git rev-parse HEAD)" = "$RUN_SHA" ]', later, f)

    def test_every_checkout_is_pinned_to_a_commit_never_to_a_branch_name(self):
        expected = {
            (BETA, "gate"): "${{ github.sha }}",
            (BETA, "build"): "${{ needs.gate.outputs.sha }}",
            (BETA, "publish"): "${{ needs.gate.outputs.sha }}",
            (PROMOTE, "authorize"): "${{ github.sha }}",
            (PROMOTE, "appcast"): "${{ needs.authorize.outputs.run_sha }}",
            (PROMOTE, "fold"): "${{ needs.authorize.outputs.run_sha }}",
        }
        seen = set()
        for f in BOTH:
            for name, job in job_texts(read(f)).items():
                for step in steps_of(job):
                    if "actions/checkout@" in step:
                        seen.add((f, name))
                        refs = re.findall(r"(?m)^\s+ref: (.*?)\s*$", step)
                        self.assertEqual(refs, [expected[(f, name)]], f"{f} {name}")
                        self.assertIn("persist-credentials: false", step, f"{f} {name}")
        self.assertEqual(seen, set(expected))

    def test_jobs_that_read_tags_or_history_check_out_all_of_both(self):
        needs_history = re.compile(r"version\.py next|fragments\.py (fold|notes)|git describe|git tag --list|git worktree")
        pinned = {(BETA, "gate"), (BETA, "build"), (PROMOTE, "authorize"), (PROMOTE, "fold")}
        seen = set()
        for f in BOTH:
            for name, job in job_texts(read(f)).items():
                if any(needs_history.search(b) for b in run_blocks(job)):
                    seen.add((f, name))
                    checkout = next(s for s in steps_of(job) if "actions/checkout@" in s)
                    self.assertRegex(checkout, r"(?m)^\s+fetch-depth: 0\s*$", f"{f} {name}")
        self.assertEqual(seen, pinned)

    def test_no_job_that_checks_out_repository_code_runs_in_the_release_environment(self):
        for f in BOTH:
            for name, job in job_texts(read(f)).items():
                if "actions/checkout@" in job:
                    self.assertNotRegex(job_head(job), r"(?m)^    environment:", f"{f} {name}")
                    self.assertNotRegex(job, r"(?m)^\s+environment:", f"{f} {name}")

    def test_the_run_block_reader_sees_every_run_key(self):
        for f in BOTH:
            text = read(f)
            keys = len(re.findall(r"(?m)^ *(?:- )?run:", text))
            outputs = len(re.findall(r"(?m)^ {6}run: \$\{\{ steps\.\w+\.outputs\.run \}\}$", text))
            blocks = run_blocks(text)
            self.assertEqual(len(blocks), keys - outputs, f)
            for b in blocks:
                self.assertTrue(b.strip(), f)

    def test_validation_is_a_bash_regex_not_a_grep_over_printf(self):
        for f in BOTH:
            blocks = run_blocks(read(f))
            for block in blocks:
                self.assertNotRegex(block, r"printf\s+'%s'[^\n]*\|\s*grep", f)
            self.assertTrue(any("=~" in b for b in blocks), f)

    def test_nothing_sources_or_evaluates_what_it_reads(self):
        for f in BOTH:
            for name, job in job_texts(read(f)).items():
                for block in run_blocks(job):
                    for rx in SOURCING:
                        self.assertIsNone(rx.search(block), f"{f} {name}: {rx.pattern}")

    def test_the_sourcing_check_catches_what_it_is_for(self):
        for bad in ("source release-info.env", ". ./release-info.env", "eval \"$(cat release-info.env)\"",
                    "x=1; . signed/release-info.env", "bash signed/release-info.env", "echo hi && source f"):
            self.assertTrue(any(rx.search(bad) for rx in SOURCING), bad)
        for fine in ("gh attestation verify f --source-ref refs/heads/main --source-digest abc", "echo done.",
                     "bash -c true", "cp ./a b"):
            self.assertFalse(any(rx.search(fine) for rx in SOURCING), fine)


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


class BetaWorkflow(unittest.TestCase):
    def setUp(self):
        self.text = read(BETA)
        self.jobs = job_texts(self.text)

    def test_jobs(self):
        self.assertEqual(list(self.jobs), ["gate", "build", "sign", "publish"])

    def test_triggers_are_a_schedule_and_a_click(self):
        on = self.text.split("\non:\n", 1)[1].split("\npermissions:", 1)[0]
        self.assertIn("schedule:", on)
        self.assertIn("workflow_dispatch:", on)
        self.assertNotIn("pull_request", on)
        self.assertNotIn("push:", on)

    def test_build_job_has_no_secrets_and_a_read_only_token(self):
        build = self.jobs["build"]
        self.assertNotIn("secrets.", build)
        self.assertNotIn("environment:", build)
        self.assertNotIn("github.token", build)
        self.assertEqual(permissions_of(build), {"contents": "read"})
        self.assertIn("./build.sh", build)

    def test_gate_has_no_secrets_and_reads_only(self):
        gate = self.jobs["gate"]
        self.assertNotIn("secrets.", gate)
        self.assertEqual(permissions_of(gate), {"contents": "read"})

    def test_gate_outputs_the_commit_it_checked_out(self):
        gate = self.jobs["gate"]
        self.assertIn("sha: ${{ steps.decide.outputs.sha }}", gate)
        self.assertIn('sha="$RUN_SHA"', script_of(gate, "Decide"))

    def test_sign_job_runs_no_repository_code(self):
        sign = self.jobs["sign"]
        self.assertNotIn("actions/checkout", sign)
        for block in run_blocks(sign):
            self.assertIsNone(REPO_SCRIPT.search(block), block)
        for step in steps_of(sign):
            for m in re.finditer(r"uses: (\S+)", step):
                self.assertTrue(re.match(r"actions/[a-z-]+@[0-9a-f]{40}$", m.group(1)), m.group(1))

    def test_sign_job_holds_no_repository_write_token(self):
        sign = self.jobs["sign"]
        self.assertEqual(permissions_of(sign), {"contents": "read", "id-token": "write", "attestations": "write"})
        self.assertNotIn("github.token", sign)
        self.assertNotIn("GH_TOKEN", sign)
        self.assertNotIn("contents: write", sign)
        self.assertRegex(sign, r"(?m)^    environment: release\s*$")

    def test_sign_job_fails_closed_on_every_signing_secret(self):
        sign = self.jobs["sign"]
        step = next(s for s in steps_of(sign) if "- name: Require the signing secrets" in s)
        for name in SIGNING_SECRETS:
            self.assertIn(f"{name}: ${{{{ secrets.{name} }}}}", step)
        self.assertIn("exit 1", step)
        self.assertIn('[ -z "${!name}" ]', step)
        self.assertLess(sign.index("Require the signing secrets"), sign.index("actions/download-artifact"))

    def test_secrets_are_in_step_env_only_never_job_env(self):
        for name, job in self.jobs.items():
            self.assertNotIn("secrets.", job_head(job), name)
        for secret in SIGNING_SECRETS:
            self.assertTrue([s for s in steps_of(self.jobs["sign"]) if f"secrets.{secret}" in s], secret)
        for step in steps_of(self.jobs["sign"]):
            if "secrets." in step:
                self.assertIn("run:", step)
                self.assertNotIn("uses:", step)

    def test_secrets_exist_in_no_other_job(self):
        for name in ("gate", "build", "publish"):
            self.assertNotIn("secrets.", self.jobs[name], name)

    def test_sign_job_verifies_the_hash_before_signing(self):
        sign = self.jobs["sign"]
        self.assertIn("shasum -a 256 -c SHA256SUMS", sign)
        self.assertIn("EXPECTED_ZIP_SHA256: ${{ needs.build.outputs.zip_sha256 }}", sign)
        self.assertLess(sign.index("shasum -a 256 -c SHA256SUMS"), sign.index("codesign --force"))
        self.assertIn("zip_sha256: ${{ steps.package.outputs.zip_sha256 }}", self.jobs["build"])

    def test_sign_job_signs_notarizes_staples_and_attests_in_order(self):
        sign = self.jobs["sign"]
        order = ["codesign --force --timestamp --options runtime", "xcrun notarytool submit", "xcrun stapler staple",
                 "sign_update", "actions/attest-build-provenance@", "actions/upload-artifact@"]
        positions = [sign.index(s) for s in order]
        self.assertEqual(positions, sorted(positions))
        self.assertIn('[ "$status" != "Accepted" ]', sign)

    def test_sparkle_signing_key_goes_through_stdin_and_the_tool_is_pinned(self):
        sign = self.jobs["sign"]
        self.assertIn("--ed-key-file -", sign)
        self.assertNotRegex(sign, r"sign_update\"? .* -s ")
        version = re.search(r'SPARKLE_VERSION: "(\d+)\.(\d+)\.(\d+)"', sign)
        self.assertTrue(version)
        self.assertGreaterEqual(tuple(int(x) for x in version.groups()), (2, 6, 4))
        self.assertRegex(sign, r"SPARKLE_SHA256: [0-9a-f]{64}\b")
        self.assertIn('shasum -a 256 -c -', sign)
        self.assertLess(sign.index("shasum -a 256 -c -"), sign.index("tar -xJf"))
        self.assertEqual(len(re.findall("SPARKLE_ED_PRIVATE_KEY", sign)), 6)

    def test_sign_job_imports_the_certificate_for_codesign_only(self):
        sign = self.jobs["sign"]
        self.assertRegex(sign, r"security import [^\n]* -T /usr/bin/codesign\b")
        self.assertNotRegex(sign, r"security import [^\n]* -A\b")

    def test_sign_job_runs_python_isolated(self):
        sign = self.jobs["sign"]
        self.assertIn("python3", sign)
        self.assertNotRegex(sign, r"python3(?! -I )")

    def test_sign_job_checks_the_app_against_the_tag_and_the_fields_it_publishes(self):
        script = script_of(self.jobs["sign"], "Sign, notarize and staple")
        self.assertIn('[ "$VERSION" = "$SHORT" ]', script)
        self.assertNotRegex(script, r'case "\$VERSION" in "\$SHORT"')
        self.assertIn("[[ $BUNDLE =~ ^[0-9]+$ ]]", script)
        self.assertIn(r"[[ $MINSYS =~ ^[0-9]+\.[0-9]+$ ]]", script)
        for check in ('[ "$VERSION" = "$SHORT" ]', "[[ $BUNDLE =~", "[[ $MINSYS =~"):
            self.assertLess(script.index(check), script.index("info.env"), check)
            self.assertLess(script.index(check), script.index("codesign --force"), check)

    def test_the_version_field_checks_reject_a_value_with_a_second_line(self):
        script = script_of(self.jobs["sign"], "Sign, notarize and staple")
        lines = "\n".join(l.strip() for l in script.splitlines() if re.match(r"\s*\[\[ \$(BUNDLE|MINSYS) =~", l))
        self.assertEqual(lines.count("=~"), 2)
        for bash in BASHES:
            for bundle, minsys, ok in (("1030002", "13.0", True), ("1030002\nx", "13.0", False),
                                       ("12ab", "13.0", False), ("", "13.0", False),
                                       ("1030002", "13.0\n14", False), ("1030002", "13", False),
                                       ("1030002", "13.0.1", False), ("1030002", "$(id).0", False)):
                with self.subTest(bash=bash, bundle=bundle, minsys=minsys):
                    r = subprocess.run([bash, "-e", "-c", lines], capture_output=True, text=True, timeout=30,
                                       env={"PATH": os.defpath, "BUNDLE": bundle, "MINSYS": minsys},
                                       stdin=subprocess.DEVNULL)
                    self.assertEqual(r.returncode == 0, ok, r.stderr)

    def test_the_app_is_signed_with_the_one_entitlement_it_needs(self):
        script = script_of(self.jobs["sign"], "Sign, notarize and staple")
        body = re.search(r"<<'PLIST'\n(.*?)\nPLIST\n", script, re.S)
        self.assertIsNotNone(body)
        self.assertEqual(plistlib.loads(body.group(1).encode()), {"com.apple.security.automation.apple-events": True})
        sign_lines = [l for l in commands(script).splitlines() if l.startswith("codesign --force")]
        by_target = {l.rsplit(" ", 1)[1]: l for l in sign_lines}
        self.assertEqual(set(by_target), {'"$APP/Contents/Helpers/agent-console-hostd"', '"$APP"', '"$DMG"'})
        self.assertIn('--entitlements "$ENTITLEMENTS"', by_target['"$APP"'])
        self.assertIn("--options runtime", by_target['"$APP"'])
        self.assertNotIn("--entitlements", by_target['"$APP/Contents/Helpers/agent-console-hostd"'])
        self.assertNotIn("--entitlements", by_target['"$DMG"'])
        self.assertLess(script.index("PLIST\n"), script.index("codesign --force"))

    def test_the_entitlements_snippet_writes_a_valid_plist_under_every_bash(self):
        script = script_of(self.jobs["sign"], "Sign, notarize and staple")
        start = script.index("ENTITLEMENTS=")
        snippet = script[start:script.index("\nPLIST\n", start) + len("\nPLIST\n")]
        for bash in BASHES:
            with self.subTest(bash=bash), tempfile.TemporaryDirectory() as tmp:
                r = subprocess.run([bash, "-e", "-c", snippet], capture_output=True, text=True, timeout=30,
                                   env={"PATH": os.defpath, "RUNNER_TEMP": tmp}, stdin=subprocess.DEVNULL)
                self.assertEqual(r.returncode, 0, r.stderr)
                data = plistlib.loads((Path(tmp) / "app.entitlements").read_bytes())
                self.assertEqual(data, {"com.apple.security.automation.apple-events": True})
                if shutil.which("plutil"):
                    lint = subprocess.run(["plutil", "-lint", str(Path(tmp) / "app.entitlements")],
                                          capture_output=True, text=True, timeout=30)
                    self.assertEqual(lint.returncode, 0, lint.stdout + lint.stderr)

    def test_the_missing_usage_description_is_written_down_where_the_entitlement_is(self):
        raw = (ROOT / BETA).read_text()
        self.assertIn("NSAppleEventsUsageDescription", raw)

    def test_sign_job_hands_the_hashes_and_the_signature_to_publish_as_job_outputs(self):
        sign, publish = self.jobs["sign"], self.jobs["publish"]
        head = job_head(sign)
        for name in ("dmg_sha256", "zip_sha256", "ed_signature"):
            self.assertIn(f"{name}: ${{{{ steps.update.outputs.{name} }}}}", head)
        update = step_named(sign, "Sign the update")
        self.assertRegex(update, r"(?m)^        id: update$")
        for name in ("dmg_sha256", "zip_sha256", "ed_signature"):
            self.assertRegex(update, r'echo "%s=' % name)
        self.assertIn(">> \"$GITHUB_OUTPUT\"", update)
        verify = step_named(publish, "Verify the signed files")
        self.assertIn("EXPECTED_DMG_SHA256: ${{ needs.sign.outputs.dmg_sha256 }}", verify)
        self.assertIn("EXPECTED_ZIP_SHA256: ${{ needs.sign.outputs.zip_sha256 }}", verify)
        read_info = step_named(publish, "Read the release info")
        self.assertIn("EXPECTED_ED_SIGNATURE: ${{ needs.sign.outputs.ed_signature }}", read_info)

    def test_publish_job_has_a_write_token_and_no_signing_secret(self):
        publish = self.jobs["publish"]
        self.assertEqual(permissions_of(publish), {"contents": "write"})
        self.assertNotIn("secrets.", publish)
        self.assertNotIn("id-token", publish)
        for secret in SIGNING_SECRETS:
            self.assertNotIn(secret, publish)

    def test_publish_job_runs_only_the_pinned_commits_appcast_tool(self):
        publish = self.jobs["publish"]
        self.assertEqual(publish.count("actions/checkout@"), 1)
        self.assertIn("ref: ${{ needs.gate.outputs.sha }}", publish)
        scripts = set()
        for block in run_blocks(publish):
            scripts.update(re.findall(r"tools/[\w/.-]+", block))
        self.assertEqual(scripts, {"tools/release/appcast.py"})

    def test_publish_job_checks_the_hash_list_and_the_values_it_uses(self):
        publish = self.jobs["publish"]
        self.assertIn("sha256sum -c SHA256SUMS", publish)
        for key in ("BUNDLE_VERSION", "SHORT_VERSION", "ED_SIGNATURE", "DMG_LENGTH", "MIN_SYSTEM"):
            self.assertRegex(publish, r'Bad %s' % key)
        self.assertLess(publish.index("sha256sum -c SHA256SUMS"), publish.index("gh release create"))

    def test_the_feed_is_the_appcast_release_asset(self):
        publish = self.jobs["publish"]
        self.assertIn("gh release download appcast --pattern appcast.xml", publish)
        self.assertIn("gh release upload appcast feed/appcast.xml --clobber", publish)
        self.assertIn("--channel beta", publish)
        self.assertIn("appcast.py verify", publish)
        self.assertIn("--prerelease", publish)

    def test_a_beta_item_has_a_date_and_no_phased_rollout(self):
        add = next(l for l in commands(script_of(self.jobs["publish"], "Update the feed")).splitlines()
                   if "appcast.py add" in l)
        self.assertIn("--pub-date now", add)
        self.assertIn("--rollout 0", add)
        self.assertIn("--channel beta", add)

    def test_gate_refuses_breaking_without_a_maintainer(self):
        gate = self.jobs["gate"]
        self.assertIn("--allow-breaking", gate)
        self.assertIn("from guard.gate import MAINTAINERS", gate)
        self.assertIn("version.py next --channel beta", gate)

    def test_version_goes_into_the_built_copy_only(self):
        build = self.jobs["build"]
        self.assertIn("version.py plist build/Notchling.app", build)
        self.assertNotIn("legacy/macos/Info.plist", self.text)


class BetaScripts(ScriptCase):

    TAG = "v1.3.0-beta.2"
    DMG = b"dmg-bytes"

    def setUp(self):
        self.publish = job_texts(read(BETA))["publish"]
        self.verify = script_of(self.publish, "Verify the signed files")
        self.build_number = script_of(self.publish, "Work out the build number")
        self.read_info = script_of(self.publish, "Read the release info")

    def lines(self, **over):
        base = {"BUNDLE_VERSION": "1030002", "SHORT_VERSION": "1.3.0-beta.2", "MIN_SYSTEM": "13.0", "TAG": self.TAG,
                "DMG_NAME": "Notchling-1.3.0-beta.2.dmg", "DMG_LENGTH": str(len(self.DMG)), "ED_SIGNATURE": GOOD_SIG}
        base.update(over)
        return ["%s=%s" % kv for kv in base.items() if kv[1] is not None]

    def info(self, tmp, lines, trailing_newline=True):
        signed = Path(tmp) / "signed"
        signed.mkdir()
        (signed / "Notchling-1.3.0-beta.2.dmg").write_bytes(self.DMG)
        (signed / "release-info.env").write_text("\n".join(lines) + ("\n" if trailing_newline else ""))
        env_file = Path(tmp) / "github_env"
        env_file.write_text("")
        return env_file

    def run_info(self, bash, lines, trailing_newline=True, **env):
        with tempfile.TemporaryDirectory() as tmp:
            env_file = self.info(tmp, lines, trailing_newline)
            args = {"TAG": self.TAG, "BUILD_NUMBER": "1030002", "EXPECTED_ED_SIGNATURE": GOOD_SIG,
                    "GITHUB_ENV": str(env_file)}
            args.update(env)
            r = self.run_script(self.read_info, tmp, bash, **args)
            planted = [p for p in Path(tmp).rglob("*") if p.name in ("pwned", "evil")]
            return r, env_file.read_text(), planted

    def test_good_release_info_is_read(self):
        for bash in BASHES:
            for newline in (True, False):
                with self.subTest(bash=bash, trailing_newline=newline):
                    r, env, planted = self.run_info(bash, self.lines(), newline)
                    self.assertEqual(r.returncode, 0, self.fail_msg(r))
                    self.assertEqual(sorted(env.splitlines()), sorted([
                        "BUNDLE_VERSION=1030002", "SHORT_VERSION=1.3.0-beta.2", "MIN_SYSTEM=13.0",
                        "ED_SIGNATURE=" + GOOD_SIG, "DMG_LENGTH=9", "DMG_NAME=Notchling-1.3.0-beta.2.dmg"]))

    def test_release_info_that_repeats_a_key_is_refused(self):
        for bash in BASHES:
            for extra in ("BUNDLE_VERSION=1030002", "BUNDLE_VERSION=1", "MIN_SYSTEM=13.0", "ED_SIGNATURE=" + GOOD_SIG,
                          "TAG=" + self.TAG):
                with self.subTest(bash=bash, extra=extra):
                    r, env, _ = self.run_info(bash, self.lines() + [extra])
                    self.assertNotEqual(r.returncode, 0, self.fail_msg(r))
                    self.assertEqual(env, "")
                    first = self.lines()
                    r, env, _ = self.run_info(bash, [extra] + first)
                    self.assertNotEqual(r.returncode, 0, self.fail_msg(r))

    def test_release_info_with_anything_but_the_seven_known_keys_is_refused(self):
        for bash in BASHES:
            for extra in ("BASH_ENV=/tmp/evil", "LD_PRELOAD=/tmp/evil", "GITHUB_ENV=/tmp/evil", "PATH=/tmp",
                          "# a comment", "", " ", "export BUNDLE_VERSION=1", "bundle_version=1", "just text",
                          "=x", "X Y=1"):
                with self.subTest(bash=bash, extra=extra):
                    r, env, planted = self.run_info(bash, self.lines() + [extra])
                    self.assertNotEqual(r.returncode, 0, self.fail_msg(r))
                    self.assertEqual(env, "")
                    self.assertEqual(planted, [])

    def test_release_info_with_a_missing_key_is_refused(self):
        for bash in BASHES:
            for key in ("BUNDLE_VERSION", "SHORT_VERSION", "MIN_SYSTEM", "TAG", "DMG_NAME", "DMG_LENGTH", "ED_SIGNATURE"):
                with self.subTest(bash=bash, missing=key):
                    r, env, _ = self.run_info(bash, self.lines(**{key: None}))
                    self.assertNotEqual(r.returncode, 0, self.fail_msg(r))
                    self.assertEqual(env, "")

    def test_injected_values_are_refused_and_never_run(self):
        hostile = {
            "BUNDLE_VERSION": ["1030002$(touch pwned)", "`touch pwned`", "1030002;touch pwned", "1030002 ", "0x1f",
                               "1030002\r", "$(touch pwned)"],
            "SHORT_VERSION": ["1.3.0-beta.2$(touch pwned)", "1.3.0-beta.2;touch pwned", "1.3.0-beta.2 "],
            "MIN_SYSTEM": ["13.0$(touch pwned)", "13.0;touch pwned", "13.0 ", "13.0\r", "13", "13.0.1", "x"],
            "ED_SIGNATURE": [GOOD_SIG + "$(touch pwned)", GOOD_SIG[:-2] + "$(", "A" * 85 + "==", GOOD_SIG + " "],
            "DMG_LENGTH": ["9$(touch pwned)", "09", "0", "-9", "9 ", "9.0"],
            "DMG_NAME": ["Notchling-1.3.0-beta.2.dmg$(touch pwned)", "../Notchling-1.3.0-beta.2.dmg",
                         "Notchling-1.3.0-beta.2.dmg;touch pwned", "Notchling-9.9.9.dmg"],
            "TAG": ["v1.3.0-beta.3", "v1.3.0-beta.2$(touch pwned)", "v1.3.0"],
        }
        for bash in BASHES:
            for key, values in hostile.items():
                for value in values:
                    with self.subTest(bash=bash, key=key, value=value):
                        r, env, planted = self.run_info(bash, self.lines(**{key: value}))
                        self.assertNotEqual(r.returncode, 0, self.fail_msg(r))
                        self.assertEqual(env, "")
                        self.assertEqual(planted, [])

    def test_a_value_is_checked_against_the_tag_the_sign_output_and_the_file(self):
        cases = (
            ("BUNDLE_VERSION", "1030003", {}),
            ("SHORT_VERSION", "1.3.0-beta.3", {}),
            ("SHORT_VERSION", "1.3.0", {}),
            ("ED_SIGNATURE", "B" * 86 + "==", {}),
            ("DMG_LENGTH", "10", {}),
            ("TAG", "v1.3.0-beta.2", {"TAG": "v1.3.0-beta.1"}),
        )
        for bash in BASHES:
            for key, value, env_over in cases:
                with self.subTest(bash=bash, key=key, value=value, env=env_over):
                    r, env, _ = self.run_info(bash, self.lines(**{key: value}), **env_over)
                    self.assertNotEqual(r.returncode, 0, self.fail_msg(r))
                    self.assertEqual(env, "")

    def test_build_number_is_the_formula_version_py_uses(self):
        sys.path.insert(0, str(ROOT / "tools" / "release"))
        import version
        for bash in BASHES:
            for tag in ("v1.3.0-beta.2", "v0.0.1-beta.1", "v1.0.0-beta.98", "v12.34.56-beta.7", "v1.99.99-beta.98",
                        "v0.1.0-beta.10", "v3.0.0-beta.1"):
                with self.subTest(bash=bash, tag=tag), tempfile.TemporaryDirectory() as tmp:
                    out = Path(tmp) / "out"
                    out.write_text("")
                    r = self.run_script(self.build_number, tmp, bash, TAG=tag, GITHUB_OUTPUT=str(out))
                    self.assertEqual(r.returncode, 0, self.fail_msg(r))
                    want = version.build_number(version.parse_tag(tag))
                    self.assertEqual(out.read_text(), "number=%d\n" % want)

    def test_build_number_refuses_what_version_py_refuses_and_what_is_not_a_beta_tag(self):
        for bash in BASHES:
            for tag in ("v1.3.0", "v1.100.0-beta.1", "v1.3.100-beta.1", "v1.3.0-beta.99", "v1.3.0-beta.0",
                        "v01.3.0-beta.1", "v1.3.0-beta.1\nx", "v1.3.0-beta.1 ", "1.3.0-beta.1", "v1.3-beta.1",
                        "v99999.0.0-beta.1", "$(id)", ""):
                with self.subTest(bash=bash, tag=tag), tempfile.TemporaryDirectory() as tmp:
                    out = Path(tmp) / "out"
                    out.write_text("")
                    r = self.run_script(self.build_number, tmp, bash, TAG=tag, GITHUB_OUTPUT=str(out))
                    self.assertNotEqual(r.returncode, 0, self.fail_msg(r))
                    self.assertEqual(out.read_text(), "")

    def signed_dir(self, tmp, names=("dmg", "zip", "notes", "info"), extra=()):
        signed = Path(tmp) / "signed"
        signed.mkdir()
        files = {"dmg": ("Notchling-1.3.0-beta.2.dmg", self.DMG), "zip": ("Notchling-1.3.0-beta.2.zip", b"zip-bytes"),
                 "notes": ("notes.md", b"notes"), "info": ("release-info.env", b"info")}
        for n in names:
            (signed / files[n][0]).write_bytes(files[n][1])
        for name, data in extra:
            (signed / name).write_bytes(data)
        return signed

    def write_sums(self, signed, names=None):
        names = names or sorted(p.name for p in signed.iterdir())
        (signed / "SHA256SUMS").write_text("".join(sums_line(signed / n) for n in names))

    def run_verify(self, bash, tmp, **over):
        env = {"TAG": self.TAG, "EXPECTED_DMG_SHA256": sha256_of(self.DMG), "EXPECTED_ZIP_SHA256": sha256_of(b"zip-bytes")}
        env.update(over)
        return self.run_script(self.verify, tmp, bash, **env)

    def test_the_signed_files_must_be_the_ones_the_sign_job_hashed(self):
        for bash in BASHES:
            with self.subTest(bash=bash, case="good"), tempfile.TemporaryDirectory() as tmp:
                signed = self.signed_dir(tmp)
                self.write_sums(signed)
                r = self.run_verify(bash, tmp)
                self.assertEqual(r.returncode, 0, self.fail_msg(r))
            def change(name, data, relist):
                def tamper(s):
                    (s / name).write_bytes(data)
                    if relist:
                        self.write_sums(s)
                return tamper

            cases = {
                "dmg changed with a matching list": change("Notchling-1.3.0-beta.2.dmg", b"other", True),
                "zip changed with a matching list": change("Notchling-1.3.0-beta.2.zip", b"other", True),
                "dmg changed after the list": change("Notchling-1.3.0-beta.2.dmg", b"other", False),
                "an extra file in the list": change("extra.sh", b"x", True),
                "a file missing from the list": lambda s: self.write_sums(
                    s, ["Notchling-1.3.0-beta.2.dmg", "notes.md", "release-info.env"]),
            }
            for label, tamper in cases.items():
                with self.subTest(bash=bash, case=label), tempfile.TemporaryDirectory() as tmp:
                    signed = self.signed_dir(tmp)
                    self.write_sums(signed)
                    tamper(signed)
                    r = self.run_verify(bash, tmp)
                    self.assertNotEqual(r.returncode, 0, self.fail_msg(r))
            for label, over in {"a bad tag": {"TAG": "v1.3.0"}, "a bad dmg hash": {"EXPECTED_DMG_SHA256": "xyz"},
                                "a bad zip hash": {"EXPECTED_ZIP_SHA256": "A" * 64},
                                "a two-line hash": {"EXPECTED_DMG_SHA256": sha256_of(self.DMG) + "\nx"}}.items():
                with self.subTest(bash=bash, case=label), tempfile.TemporaryDirectory() as tmp:
                    signed = self.signed_dir(tmp)
                    self.write_sums(signed)
                    r = self.run_verify(bash, tmp, **over)
                    self.assertNotEqual(r.returncode, 0, self.fail_msg(r))


class PromoteWorkflow(unittest.TestCase):
    def setUp(self):
        self.text = read(PROMOTE)
        self.jobs = job_texts(self.text)

    def test_jobs(self):
        self.assertEqual(list(self.jobs), ["authorize", "release", "appcast", "fold"])

    def test_only_a_click_with_a_strictly_validated_beta_tag(self):
        on = self.text.split("\non:\n", 1)[1].split("\npermissions:", 1)[0]
        self.assertIn("workflow_dispatch:", on)
        for other in ("schedule:", "push:", "pull_request"):
            self.assertNotIn(other, on)
        self.assertIn("BETA_TAG: ${{ inputs.beta_tag }}", self.jobs["authorize"])
        self.assertIn(r"^v[0-9]+\.[0-9]+\.[0-9]+-beta\.[0-9]+$", self.jobs["authorize"])
        self.assertIn(r"^v[0-9]+\.[0-9]+\.[0-9]+$", self.jobs["authorize"])

    def test_actor_must_be_a_maintainer_before_anything_else_runs(self):
        auth = self.jobs["authorize"]
        self.assertIn("ACTOR: ${{ github.triggering_actor }}", auth)
        self.assertIn("from guard.gate import MAINTAINERS", auth)
        self.assertLess(auth.index("MAINTAINERS"), auth.index("version.py next"))
        self.assertNotIn("secrets.", auth)
        self.assertEqual(permissions_of(auth), {"contents": "read"})

    def test_authorize_outputs_the_betas_commit_and_the_commit_the_tools_come_from(self):
        head = job_head(self.jobs["authorize"])
        for name in ("beta_tag", "stable_tag", "sha", "run_sha", "bundle_version"):
            self.assertRegex(head, r"(?m)^      %s: \$\{\{ steps\.\w+\.outputs\.\w+ \}\}$" % name)
        check = script_of(self.jobs["authorize"], "Check the actor and the tags")
        self.assertIn('echo "run_sha=$RUN_SHA"', check)

    def test_the_other_jobs_depend_on_authorize(self):
        for name in ("release", "appcast", "fold"):
            self.assertRegex(self.jobs[name], r"(?m)^    needs: .*authorize", name)

    def test_release_job_runs_no_repository_code(self):
        job = self.jobs["release"]
        self.assertNotIn("actions/checkout", job)
        for block in run_blocks(job):
            self.assertIsNone(REPO_SCRIPT.search(block), block)
        self.assertNotIn("secrets.", job)

    def test_release_job_may_read_attestations_and_write_releases_and_nothing_more(self):
        self.assertEqual(permissions_of(self.jobs["release"]), {"contents": "write", "attestations": "read"})

    def test_release_job_never_rebuilds_or_signs(self):
        for name, job in self.jobs.items():
            for word in ("build.sh", "xcodebuild", "swift build", "hdiutil create", "codesign", "notarytool",
                         "sign_update", "SPARKLE_ED_PRIVATE_KEY", "APPLE_"):
                self.assertNotIn(word, job, f"{name}: {word}")

    def test_promote_verifies_the_same_hash_as_the_beta_published(self):
        job = self.jobs["release"]
        self.assertIn("gh release download \"$BETA_TAG\" --dir beta", job)
        self.assertIn("sha256sum -c SHA256SUMS", job)
        self.assertIn("gh attestation verify", job)
        self.assertIn("--signer-workflow \"$GH_REPO/.github/workflows/beta.yml\"", job)
        self.assertIn('cp "beta/Notchling-$bv.dmg" "stable/Notchling-$sv.dmg"', job)
        self.assertIn('cp "beta/Notchling-$bv.zip" "stable/Notchling-$sv.zip"', job)
        self.assertIn('grep -F "Notchling-$bv.$ext" beta/SHA256SUMS', job)
        self.assertIn('[ -n "$want" ] && [ "$want" = "$got" ]', job)
        self.assertLess(job.index("sha256sum -c SHA256SUMS"), job.index("gh release create"))
        self.assertLess(job.index('[ "$want" = "$got" ]'), job.index("gh release create"))

    def test_promote_pins_the_attestation_to_the_default_branch_and_the_betas_commit(self):
        job = self.jobs["release"]
        verify = [l for l in commands(job).splitlines() if "gh attestation verify" in l]
        self.assertEqual(len(verify), 1, verify)
        for flag in ('--repo "$GH_REPO"', '--signer-workflow "$GH_REPO/.github/workflows/beta.yml"',
                     '--source-ref "refs/heads/$DEFAULT_BRANCH"', '--source-digest "$SHA"',
                     "--deny-self-hosted-runners"):
            self.assertIn(flag, verify[0])
        step = step_named(job, "Download and verify the Beta's files")
        self.assertIn("SHA: ${{ needs.authorize.outputs.sha }}", step)
        self.assertIn("DEFAULT_BRANCH: ${{ github.event.repository.default_branch }}", step)

    def test_promote_creates_the_stable_release_at_the_betas_commit(self):
        job = self.jobs["release"]
        self.assertIn('gh release create "$STABLE_TAG"', job)
        self.assertIn('--target "$SHA"', job)
        self.assertIn("SHA: ${{ needs.authorize.outputs.sha }}", job)
        self.assertIn('gh api "repos/$GH_REPO/commits/$BETA_TAG" --jq .sha', self.jobs["authorize"])

    def test_the_stable_notes_come_from_the_betas_commit_not_the_default_branch(self):
        script = script_of(self.jobs["authorize"], "Write the Stable notes")
        self.assertIn('git worktree add --detach "$RUNNER_TEMP/beta" "$SHA"', script)
        self.assertIn('fragments.py notes --repo "$RUNNER_TEMP/beta" --since "$last"', script)
        for line in commands(script).splitlines():
            if "fragments.py notes" in line:
                self.assertIn('--repo "$RUNNER_TEMP/beta"', line)
        self.assertLess(script.index("git worktree add"), script.index("fragments.py notes"))

    def test_authorize_dry_runs_the_promotion_before_anything_is_published(self):
        auth = self.jobs["authorize"]
        step = step_named(auth, "Check the feed")
        cmd = commands(step)
        self.assertIn("gh release download appcast --pattern appcast.xml --dir feedcheck", cmd)
        self.assertIn("appcast.py verify --feed feedcheck/appcast.xml", cmd)
        promote = [l for l in cmd.splitlines() if "appcast.py promote" in l]
        self.assertEqual(len(promote), 1, promote)
        self.assertIn("--feed feedcheck/appcast.xml", promote[0])
        for part in ("--expect-url", "--expect-short", "--notes-url", "--pub-date now", "--rollout 86400"):
            self.assertIn(part, promote[0])
        self.assertNotIn("gh release upload", auth)
        self.assertNotIn("gh release create", auth)
        later = [l for l in commands(step_named(self.jobs["appcast"], "Move the feed item to Stable")).splitlines()
                 if "appcast.py promote" in l]
        self.assertEqual(len(later), 1, later)
        tail = lambda l: l.split("--feed ", 1)[1].split(" ", 1)[1]  # noqa: E731
        self.assertEqual(tail(promote[0]), tail(later[0]))

    def test_appcast_job_runs_only_the_pinned_commits_appcast_tool(self):
        job = self.jobs["appcast"]
        self.assertEqual(job.count("actions/checkout@"), 1)
        self.assertIn("ref: ${{ needs.authorize.outputs.run_sha }}", job)
        self.assertNotIn("secrets.", job)
        self.assertEqual(permissions_of(job), {"contents": "write"})
        scripts = set()
        for block in run_blocks(job):
            scripts.update(re.findall(r"tools/[\w/.-]+", block))
        self.assertEqual(scripts, {"tools/release/appcast.py"})
        self.assertIn("appcast.py promote --feed feed/appcast.xml", job)
        self.assertIn("appcast.py verify", job)

    def test_the_feed_item_is_promoted_with_the_stable_notes_a_date_a_rollout_and_the_betas_enclosure(self):
        cmd = commands(script_of(self.jobs["appcast"], "Move the feed item to Stable"))
        promote = [l for l in cmd.splitlines() if "appcast.py promote" in l]
        self.assertEqual(len(promote), 1, promote)
        line = promote[0]
        self.assertIn('--version "$BUNDLE_VERSION"', line)
        self.assertIn('--short "${STABLE_TAG#v}"', line)
        self.assertIn('--notes-url "https://github.com/$GH_REPO/releases/tag/$STABLE_TAG"', line)
        self.assertIn("--pub-date now", line)
        self.assertIn("--rollout 86400", line)
        self.assertIn('--expect-url "https://github.com/$GH_REPO/releases/download/$BETA_TAG/Notchling-$bv.dmg"', line)
        self.assertIn('--expect-short "$bv"', line)
        self.assertIn("BUNDLE_VERSION: ${{ needs.authorize.outputs.bundle_version }}",
                      step_named(self.jobs["appcast"], "Move the feed item to Stable"))

    def test_fold_opens_a_pull_request_and_never_pushes_to_the_default_branch(self):
        job = self.jobs["fold"]
        self.assertNotIn("secrets.", job)
        self.assertEqual(permissions_of(job), {"contents": "write", "pull-requests": "write"})
        self.assertIn('branch="promote/$STABLE_TAG"', job)
        self.assertNotRegex(job, r"branch=\"release/|--head \"release/")
        pushes = [l for l in job.splitlines() if " push " in l]
        self.assertEqual(len(pushes), 1, pushes)
        self.assertIn('push origin "$branch"', pushes[0])
        self.assertNotIn("DEFAULT_BRANCH", pushes[0])
        self.assertIn("gh pr create", job)
        self.assertIn('--base "$DEFAULT_BRANCH" --head "$branch"', job)
        self.assertIn("fragments.py fold", job)
        self.assertNotRegex(job, r"push .*(main|--force|-f )")

    def test_fold_folds_only_the_fragments_the_beta_was_cut_with(self):
        job = self.jobs["fold"]
        self.assertIn('fragments.py fold "${STABLE_TAG#v}" --only-at "$SHA"', job)
        self.assertIn("SHA: ${{ needs.authorize.outputs.sha }}", job)
        checkout = next(s for s in steps_of(job) if "actions/checkout@" in s)
        self.assertIn("fetch-depth: 0", checkout)


class PromoteScripts(ScriptCase):
    BETA_TAG = "v1.3.0-beta.2"
    STABLE_TAG = "v1.3.0"
    SHA = "a" * 40

    def setUp(self):
        self.jobs = job_texts(read(PROMOTE))
        self.release = script_of(self.jobs["release"], "Download and verify the Beta's files")
        self.build_number = script_of(self.jobs["authorize"], "Work out the build number")
        self.notes = script_of(self.jobs["authorize"], "Write the Stable notes")

    def beta_fixture(self, tmp, tamper=None, extra_in_list=None):
        fixture = Path(tmp) / "fixture"
        fixture.mkdir()
        files = {"Notchling-1.3.0-beta.2.dmg": b"dmg", "Notchling-1.3.0-beta.2.zip": b"zip", "notes.md": b"n",
                 "release-info.env": b"i"}
        for name, data in files.items():
            (fixture / name).write_bytes(data)
        names = sorted(files) + ([extra_in_list] if extra_in_list else [])
        if extra_in_list:
            (fixture / extra_in_list).write_bytes(b"x")
        (fixture / "SHA256SUMS").write_text("".join(sums_line(fixture / n) for n in names))
        if tamper:
            (fixture / tamper).write_bytes(b"tampered")
        return fixture

    def run_release(self, bash, tmp, fixture, **over):
        log = Path(tmp) / "gh.log"
        env = {"BETA_TAG": self.BETA_TAG, "STABLE_TAG": self.STABLE_TAG, "SHA": self.SHA, "DEFAULT_BRANCH": "main",
               "GH_REPO": "owner/repo", "GH_TOKEN": "t", "GH_LOG": str(log), "GH_FIXTURE": str(fixture)}
        env.update(over)
        r = self.run_script(self.release, tmp, bash, **env)
        return r, (log.read_text().splitlines() if log.exists() else [])

    def test_both_files_are_verified_against_the_attestation_with_the_pinned_flags(self):
        for bash in BASHES:
            with self.subTest(bash=bash), tempfile.TemporaryDirectory() as tmp:
                r, log = self.run_release(bash, tmp, self.beta_fixture(tmp))
                self.assertEqual(r.returncode, 0, self.fail_msg(r))
                verifies = [l.split() for l in log if l.startswith("attestation verify")]
                self.assertEqual([v[2] for v in verifies], ["Notchling-1.3.0-beta.2.dmg", "Notchling-1.3.0-beta.2.zip"])
                for tokens in verifies:
                    flags = {t: tokens[i + 1] for i, t in enumerate(tokens) if t.startswith("--") and i + 1 < len(tokens)
                             and not tokens[i + 1].startswith("--")}
                    self.assertEqual(flags["--repo"], "owner/repo")
                    self.assertEqual(flags["--signer-workflow"], "owner/repo/.github/workflows/beta.yml")
                    self.assertEqual(flags["--source-ref"], "refs/heads/main")
                    self.assertEqual(flags["--source-digest"], self.SHA)
                    self.assertIn("--deny-self-hosted-runners", tokens)

    def test_a_failed_attestation_stops_the_promotion(self):
        for bash in BASHES:
            with self.subTest(bash=bash), tempfile.TemporaryDirectory() as tmp:
                r, _ = self.run_release(bash, tmp, self.beta_fixture(tmp), GH_ATTEST_FAIL="1")
                self.assertNotEqual(r.returncode, 0, self.fail_msg(r))

    def test_a_changed_file_or_an_unexpected_list_stops_the_promotion_before_the_attestation_check(self):
        for bash in BASHES:
            for label, kwargs in (("a changed dmg", {"tamper": "Notchling-1.3.0-beta.2.dmg"}),
                                  ("a changed list entry", {"tamper": "notes.md"}),
                                  ("an extra file in the list", {"extra_in_list": "extra.sh"})):
                with self.subTest(bash=bash, case=label), tempfile.TemporaryDirectory() as tmp:
                    r, log = self.run_release(bash, tmp, self.beta_fixture(tmp, **kwargs))
                    self.assertNotEqual(r.returncode, 0, self.fail_msg(r))
                    self.assertEqual([l for l in log if l.startswith("attestation")], [])
            for label, over in (("a bad beta tag", {"BETA_TAG": "v1.3.0"}), ("a bad stable tag", {"STABLE_TAG": "v1.3.0-beta.2"}),
                                ("a two-line beta tag", {"BETA_TAG": "v1.3.0-beta.2\nv1.3.0-beta.9"})):
                with self.subTest(bash=bash, case=label), tempfile.TemporaryDirectory() as tmp:
                    r, log = self.run_release(bash, tmp, self.beta_fixture(tmp), **over)
                    self.assertNotEqual(r.returncode, 0, self.fail_msg(r))
                    self.assertEqual(log, [])

    def test_build_number_for_the_feed_item_is_the_betas(self):
        sys.path.insert(0, str(ROOT / "tools" / "release"))
        import version
        for bash in BASHES:
            for tag in ("v1.3.0-beta.2", "v0.0.1-beta.1", "v12.34.56-beta.98"):
                with self.subTest(bash=bash, tag=tag), tempfile.TemporaryDirectory() as tmp:
                    out = Path(tmp) / "out"
                    out.write_text("")
                    r = self.run_script(self.build_number, tmp, bash, BETA_TAG=tag, GITHUB_OUTPUT=str(out))
                    self.assertEqual(r.returncode, 0, self.fail_msg(r))
                    self.assertEqual(out.read_text(), "number=%d\n" % version.build_number(version.parse_tag(tag)))
            for tag in ("v1.3.0", "v1.3.0-beta.99", "v1.3.0-beta.1\nx", "$(id)"):
                with self.subTest(bash=bash, refused=tag), tempfile.TemporaryDirectory() as tmp:
                    out = Path(tmp) / "out"
                    out.write_text("")
                    r = self.run_script(self.build_number, tmp, bash, BETA_TAG=tag, GITHUB_OUTPUT=str(out))
                    self.assertNotEqual(r.returncode, 0, self.fail_msg(r))
                    self.assertEqual(out.read_text(), "")

    def test_the_stable_notes_hold_the_fragments_of_the_betas_commit_and_not_later_ones(self):
        env = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@example.com", GIT_COMMITTER_NAME="t",
                   GIT_COMMITTER_EMAIL="t@example.com", GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_SYSTEM=os.devnull)

        def git(repo, *args):
            r = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True, env=env,
                               stdin=subprocess.DEVNULL)
            self.assertEqual(r.returncode, 0, r.stderr)
            return r.stdout.strip()

        def fragment(repo, name, kind, text):
            (Path(repo) / "changes").mkdir(exist_ok=True)
            (Path(repo) / "changes" / name).write_text("---\nkind: %s\n---\n%s\n" % (kind, text))
            git(repo, "add", "-A")
            git(repo, "commit", "-q", "-m", name)

        for bash in BASHES:
            with self.subTest(bash=bash), tempfile.TemporaryDirectory() as tmp:
                repo = Path(tmp) / "repo"
                (repo / "tools" / "release").mkdir(parents=True)
                shutil.copy(ROOT / "tools" / "release" / "fragments.py", repo / "tools" / "release" / "fragments.py")
                git(repo, "init", "-q", "-b", "main")
                fragment(repo, "old.md", "fix", "Old change.")
                git(repo, "tag", "v1.0.0")
                fragment(repo, "beta.md", "feature", "Beta change.")
                beta_sha = git(repo, "rev-parse", "HEAD")
                git(repo, "tag", "v1.1.0-beta.1")
                fragment(repo, "late.md", "fix", "Late change.")
                r = self.run_script(self.notes, repo, bash, BETA_TAG="v1.1.0-beta.1", SHA=beta_sha,
                                    RUNNER_TEMP=tmp)
                self.assertEqual(r.returncode, 0, self.fail_msg(r))
                notes = (repo / "notes" / "notes.md").read_text()
                self.assertIn("Beta change.", notes)
                self.assertNotIn("Late change.", notes)
                self.assertNotIn("Old change.", notes)
                self.assertIn("v1.1.0-beta.1", notes)
                self.assertIn("without a rebuild", notes)


class OldFlowStays(unittest.TestCase):
    def test_release_yml_is_still_the_old_flow(self):
        text = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertIn("name: Release", text)
        self.assertIn("startsWith(github.event.pull_request.head.ref, 'release/')", text)
        self.assertIn("./build.sh --dmg", text)
        self.assertIn('gh release create "v$VERSION"', text)
        self.assertNotIn("NOTCHLING_RELEASES", text)

    def test_new_workflows_do_not_touch_the_source_plist(self):
        for f in BOTH:
            self.assertNotIn("legacy/macos/Info.plist", read(f))


if __name__ == "__main__":
    unittest.main()
