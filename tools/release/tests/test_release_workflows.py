"""The Beta and Promote workflows' security contract, read as text (no YAML library, no network).

The build runs repository code and holds no secret; the signing job runs none and holds the secrets;
the publishing jobs hold a write token and run only the default branch's tools. Every job stops at its
first step unless the kill switch is on. Nothing here runs a workflow; the one behavioral test runs the
kill-switch script itself with a fake environment."""

import os
import re
import subprocess
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


def read(path):
    """The workflow without its comment lines, which may name what they forbid."""
    return "".join(l for l in (ROOT / path).read_text().splitlines(keepends=True) if not l.lstrip().startswith("#"))


def job_texts(text):
    """{job name: its text}; jobs sit at 2 spaces of indent under `jobs:`."""
    block = text.split("\njobs:\n", 1)[1]
    parts = re.split(r"(?m)^  ([A-Za-z0-9_-]+):\n", block)
    return {parts[i]: parts[i + 1] for i in range(1, len(parts), 2)}


def steps_of(job):
    """The job's steps, each as its text (steps start at 6 spaces of indent)."""
    body = job.split("\n    steps:\n", 1)[1]
    return [s for s in re.split(r"(?m)^(?=      - )", body) if s.strip()]


def run_blocks(text):
    """Every `run:` body (a `|` block or a one-line command), dedented."""
    blocks = []
    for m in re.finditer(r"(?m)^\s+run: \|\n((?:[ ]{10,}.*\n?|\n)+)", text):
        blocks.append(textwrap.dedent(m.group(1)))
    for m in re.finditer(r"(?m)^ {8}run: (?!\|)(.+)$", text):
        blocks.append(m.group(1))
    return blocks


def permissions_of(job):
    m = re.search(r"(?m)^    permissions:\n((?:      .*\n)+)", job)
    return dict(re.findall(r"      ([a-z-]+): (\w+)", m.group(1))) if m else None


REPO_SCRIPT = re.compile(r"tools/|legacy/|build\.sh|\.py\b|\./(?!bin/sign_update)")


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
        for value, ok in (("on", True), ("", False), ("off", False), ("ON", False), ("on ", False), ("true", False)):
            with self.subTest(value=value):
                with tempfile.TemporaryDirectory() as tmp:
                    path = Path(tmp) / "kill.sh"
                    path.write_text(script)
                    env = {k: v for k, v in os.environ.items() if k != "NOTCHLING_RELEASES"}
                    env["NOTCHLING_RELEASES"] = value
                    r = subprocess.run(["bash", str(path)], env=env, capture_output=True, text=True, timeout=30,
                                       stdin=subprocess.DEVNULL)
                    self.assertEqual(r.returncode == 0, ok, r.stderr)
        with tempfile.TemporaryDirectory() as tmp:  # unset counts as off
            path = Path(tmp) / "kill.sh"
            path.write_text(script)
            env = {k: v for k, v in os.environ.items() if k != "NOTCHLING_RELEASES"}
            r = subprocess.run(["bash", "-u", str(path)], env=env, capture_output=True, text=True, timeout=30,
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
                if re.search(r"\$\{\{\s*(github\.event|github\.ref|github\.triggering_actor|inputs\.)", line):
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

    def test_every_checkout_is_the_default_branch_or_the_gated_commit(self):
        for f in BOTH:
            for name, job in job_texts(read(f)).items():
                for step in steps_of(job):
                    if "actions/checkout@" in step:
                        refs = re.findall(r"(?m)^\s+ref: (.*?)\s*$", step)
                        self.assertEqual(len(refs), 1, f"{f} {name}")
                        self.assertIn(refs[0], ("${{ github.event.repository.default_branch }}",
                                                "${{ needs.gate.outputs.sha }}"), f"{f} {name}")
                        self.assertIn("persist-credentials: false", step, f"{f} {name}")


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
            head = job.split("\n    steps:\n", 1)[0]
            self.assertNotIn("secrets.", head, name)
        for secret in SIGNING_SECRETS:
            self.assertTrue([s for s in steps_of(self.jobs["sign"]) if f"secrets.{secret}" in s], secret)
        # Third-party actions get no secret: only a step with `run:` may name one.
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
        self.assertEqual(len(re.findall("SPARKLE_ED_PRIVATE_KEY", sign)), 6)  # check step: 3; sign step: 3

    def test_publish_job_has_a_write_token_and_no_signing_secret(self):
        publish = self.jobs["publish"]
        self.assertEqual(permissions_of(publish), {"contents": "write"})
        self.assertNotIn("secrets.", publish)
        self.assertNotIn("id-token", publish)
        for secret in SIGNING_SECRETS:
            self.assertNotIn(secret, publish)

    def test_publish_job_runs_only_the_default_branchs_appcast_tool(self):
        publish = self.jobs["publish"]
        self.assertEqual(publish.count("actions/checkout@"), 1)
        self.assertIn("ref: ${{ github.event.repository.default_branch }}", publish)
        scripts = set()
        for block in run_blocks(publish):
            scripts.update(re.findall(r"tools/[\w/.-]+", block))
        self.assertEqual(scripts, {"tools/release/appcast.py"})

    def test_publish_job_checks_the_hash_list_and_the_values_it_uses(self):
        publish = self.jobs["publish"]
        self.assertIn("sha256sum -c SHA256SUMS", publish)
        for key in ("BUNDLE_VERSION", "ED_SIGNATURE", "DMG_LENGTH", "MIN_SYSTEM"):
            self.assertRegex(publish, r'Bad %s' % key)
        self.assertLess(publish.index("sha256sum -c SHA256SUMS"), publish.index("gh release create"))

    def test_the_feed_is_the_appcast_release_asset(self):
        publish = self.jobs["publish"]
        self.assertIn("gh release download appcast --pattern appcast.xml", publish)
        self.assertIn("gh release upload appcast feed/appcast.xml --clobber", publish)
        self.assertIn("--channel beta", publish)
        self.assertIn("appcast.py verify", publish)
        self.assertIn("--prerelease", publish)

    def test_gate_refuses_breaking_without_a_maintainer(self):
        gate = self.jobs["gate"]
        self.assertIn("--allow-breaking", gate)
        self.assertIn("from guard.gate import MAINTAINERS", gate)
        self.assertIn("version.py next --channel beta", gate)

    def test_version_goes_into_the_built_copy_only(self):
        build = self.jobs["build"]
        self.assertIn("version.py plist build/Notchling.app", build)
        self.assertNotIn("legacy/macos/Info.plist", self.text)


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
        self.assertIn(r"'^v[0-9]+\.[0-9]+\.[0-9]+-beta\.[0-9]+$'", self.jobs["authorize"])
        self.assertIn(r"'^v[0-9]+\.[0-9]+\.[0-9]+$'", self.jobs["authorize"])

    def test_actor_must_be_a_maintainer_before_anything_else_runs(self):
        auth = self.jobs["authorize"]
        self.assertIn("ACTOR: ${{ github.triggering_actor }}", auth)
        self.assertIn("from guard.gate import MAINTAINERS", auth)
        self.assertLess(auth.index("MAINTAINERS"), auth.index("version.py next"))
        self.assertNotIn("secrets.", auth)
        self.assertEqual(permissions_of(auth), {"contents": "read"})

    def test_the_other_jobs_depend_on_authorize(self):
        for name in ("release", "appcast", "fold"):
            self.assertRegex(self.jobs[name], r"(?m)^    needs: .*authorize", name)

    def test_release_job_runs_no_repository_code(self):
        job = self.jobs["release"]
        self.assertNotIn("actions/checkout", job)
        for block in run_blocks(job):
            self.assertIsNone(REPO_SCRIPT.search(block), block)
        self.assertEqual(permissions_of(job), {"contents": "write"})
        self.assertNotIn("secrets.", job)

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
        # The Stable files are copies of the Beta's and each is compared with the Beta's list entry.
        self.assertIn('cp "beta/Notchling-$bv.dmg" "stable/Notchling-$sv.dmg"', job)
        self.assertIn('cp "beta/Notchling-$bv.zip" "stable/Notchling-$sv.zip"', job)
        self.assertIn('grep -F "Notchling-$bv.$ext" beta/SHA256SUMS', job)
        self.assertIn('[ -n "$want" ] && [ "$want" = "$got" ]', job)
        self.assertLess(job.index("sha256sum -c SHA256SUMS"), job.index("gh release create"))
        self.assertLess(job.index('[ "$want" = "$got" ]'), job.index("gh release create"))

    def test_promote_creates_the_stable_release_at_the_betas_commit(self):
        job = self.jobs["release"]
        self.assertIn('gh release create "$STABLE_TAG"', job)
        self.assertIn('--target "$SHA"', job)
        self.assertIn("SHA: ${{ needs.authorize.outputs.sha }}", job)
        self.assertIn('gh api "repos/$GH_REPO/commits/$BETA_TAG" --jq .sha', self.jobs["authorize"])

    def test_appcast_job_runs_only_the_default_branchs_appcast_tool(self):
        job = self.jobs["appcast"]
        self.assertEqual(job.count("actions/checkout@"), 1)
        self.assertNotIn("secrets.", job)
        self.assertEqual(permissions_of(job), {"contents": "write"})
        scripts = set()
        for block in run_blocks(job):
            scripts.update(re.findall(r"tools/[\w/.-]+", block))
        self.assertEqual(scripts, {"tools/release/appcast.py"})
        self.assertIn("appcast.py promote --feed feed/appcast.xml", job)
        self.assertIn("appcast.py verify", job)

    def test_fold_opens_a_pull_request_and_never_pushes_to_the_default_branch(self):
        job = self.jobs["fold"]
        self.assertNotIn("secrets.", job)
        self.assertEqual(permissions_of(job), {"contents": "write", "pull-requests": "write"})
        self.assertIn('branch="promote/$STABLE_TAG"', job)
        self.assertNotRegex(job, r"branch=\"release/|--head \"release/")  # merging a release/ branch starts the old release.yml
        pushes = [l for l in job.splitlines() if " push " in l]
        self.assertEqual(len(pushes), 1, pushes)
        self.assertIn('push origin "$branch"', pushes[0])
        self.assertNotIn("DEFAULT_BRANCH", pushes[0])
        self.assertIn("gh pr create", job)
        self.assertIn('--base "$DEFAULT_BRANCH" --head "$branch"', job)
        self.assertIn("fragments.py fold", job)
        self.assertNotRegex(job, r"push .*(main|--force|-f )")


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
