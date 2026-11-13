from __future__ import annotations

import io
import os
import plistlib
import subprocess
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

import version


def frag(kind):
    return f"---\nkind: {kind}\n---\nSomething changed.\n"


def run_cli(argv):
    out, err = io.StringIO(), io.StringIO()
    with redirect_stdout(out), redirect_stderr(err):
        code = version.main(argv)
    return code, out.getvalue().strip(), err.getvalue()


class Env(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        self.changes = self.root / "changes"
        self.changes.mkdir()
        (self.changes / "README.md").write_text("About fragments, not a fragment.\n")
        self.info = self.root / "Info.plist"
        self.info.write_bytes(plistlib.dumps({"CFBundleShortVersionString": "1.2.0", "CFBundleVersion": "3"}))

    def git(self, *args):
        env = dict(os.environ, GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_SYSTEM="/dev/null")
        subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.com",
                        "-c", "commit.gpgsign=false", *args], cwd=self.root, env=env, check=True,
                       capture_output=True)

    def tag_repo(self, tags, released):
        if not (self.root / ".git").exists():
            self.git("init", "-q", "-b", "main")
        self.git("rm", "-r", "-q", "--cached", "--ignore-unmatch", "--", "changes")
        if released:
            self.git("add", "--", *[f"changes/f{i}.md" for i in released])
        self.git("commit", "-q", "--allow-empty", "-m", "stable")
        for t in tags:
            self.git("tag", "-f", t)

    def next(self, channel, tags, *extra, fragments=(), released=()):
        for i, kind in enumerate(fragments):
            (self.changes / f"f{i}.md").write_text(frag(kind))
        tags_file = self.root / "tags.txt"
        tags_file.write_text("\n".join(tags) + ("\n" if tags else ""))
        if channel == "beta" and any(v and v[3] is None for v in map(version.parse_tag, tags)):
            self.tag_repo(tags, released)
        return run_cli(["next", "--channel", channel, "--tags-file", str(tags_file), "--repo", str(self.root),
                        "--changes-dir", str(self.changes), "--info-plist", str(self.info), *extra])


class ParseTests(unittest.TestCase):
    def test_parse_tag(self):
        self.assertEqual(version.parse_tag("v1.4.1"), (1, 4, 1, None))
        self.assertEqual(version.parse_tag("v1.4.1-beta.12"), (1, 4, 1, 12))

    def test_malformed_tags(self):
        for tag in ("1.4.1", "v1.4", "v1.4.1.2", "v1.4.1-beta", "v1.4.1-beta.0", "v1.4.1-rc.1", "v01.4.1",
                    "v1.4.1-beta.01", "v1.4.1-beta.1-x", "v1.4.1 ", "vx.y.z", "", "v-1.0.0"):
            with self.subTest(tag):
                self.assertIsNone(version.parse_tag(tag))


class NextBetaTests(Env):
    def test_first_beta_ever_comes_from_the_plist(self):
        self.assertEqual(self.next("beta", []), (0, "v1.2.1-beta.1", ""))

    def test_no_tags_file_content_with_only_junk_tags(self):
        self.assertEqual(self.next("beta", ["nightly", "v1.4", "release-1"])[1], "v1.2.1-beta.1")

    def test_after_stable_is_a_patch_beta(self):
        self.assertEqual(self.next("beta", ["v1.0.0", "v1.1.0", "v1.4.0"], fragments=["fix", "internal"])[1],
                         "v1.4.1-beta.1")

    def test_no_fragments_still_patch(self):
        self.assertEqual(self.next("beta", ["v1.4.0"])[1], "v1.4.1-beta.1")

    def test_feature_fragment_bumps_minor(self):
        self.assertEqual(self.next("beta", ["v1.4.0"], fragments=["fix", "feature"])[1], "v1.5.0-beta.1")

    def test_further_betas_bump_n(self):
        tags = ["v1.4.0", "v1.4.1-beta.1", "v1.4.1-beta.2"]
        self.assertEqual(self.next("beta", tags, fragments=["fix"])[1], "v1.4.1-beta.3")

    def test_n_follows_the_highest_not_the_count(self):
        self.assertEqual(self.next("beta", ["v1.4.0", "v1.4.1-beta.7"])[1], "v1.4.1-beta.8")

    def test_betas_of_another_target_do_not_count(self):
        tags = ["v1.4.0", "v1.4.1-beta.1", "v1.4.1-beta.2"]
        self.assertEqual(self.next("beta", tags, fragments=["feature"])[1], "v1.5.0-beta.1")
        tags += ["v1.5.0-beta.1"]
        self.assertEqual(self.next("beta", tags, fragments=["feature"])[1], "v1.5.0-beta.2")

    def test_betas_of_older_released_versions_are_ignored(self):
        tags = ["v1.3.0-beta.4", "v1.3.0", "v1.4.0"]
        self.assertEqual(self.next("beta", tags)[1], "v1.4.1-beta.1")

    def test_highest_stable_wins_over_tag_order(self):
        self.assertEqual(self.next("beta", ["v1.10.0", "v1.9.0", "v1.2.0"])[1], "v1.10.1-beta.1")

    def test_malformed_tags_are_ignored(self):
        tags = ["v1.4.0", "v1.4", "v1.9.9-beta", "v2.0.0-rc.1", "foo", "v1.4.1-beta.0"]
        self.assertEqual(self.next("beta", tags)[1], "v1.4.1-beta.1")

    def test_betas_without_any_stable_tag_use_the_plist_base(self):
        self.assertEqual(self.next("beta", ["v1.2.1-beta.1"])[1], "v1.2.1-beta.2")

    def test_stable_tag_wins_over_a_stale_plist(self):
        self.assertEqual(self.next("beta", ["v1.0.0"])[1], "v1.0.1-beta.1")

    def test_malformed_plist_version_fails_when_it_is_needed(self):
        self.info.write_bytes(plistlib.dumps({"CFBundleShortVersionString": "one", "CFBundleVersion": "3"}))
        code, out, err = self.next("beta", [])
        self.assertEqual((code, out), (1, ""))
        self.assertIn("Info.plist", err)
        self.assertEqual(self.next("beta", ["v1.0.0"])[0], 0)

    def test_missing_plist_fails_when_needed(self):
        self.info.unlink()
        self.assertEqual(self.next("beta", [])[0], 1)

    def test_malformed_fragment_fails(self):
        (self.changes / "bad.md").write_text("nope")
        code, out, err = self.next("beta", ["v1.4.0"])
        self.assertEqual((code, out), (1, ""))
        self.assertIn("bad.md", err)

    def test_prints_only_the_tag(self):
        code, out, err = self.next("beta", ["v1.4.0"])
        self.assertEqual((code, out, err), (0, "v1.4.1-beta.1", ""))


class BreakingTests(Env):
    def test_refused_without_the_flag(self):
        code, out, err = self.next("beta", ["v1.4.0"], fragments=["fix", "breaking"])
        self.assertEqual((code, out), (1, ""))
        self.assertIn("breaking", err.lower())
        self.assertIn("--allow-breaking", err)

    def test_allowed_bumps_major(self):
        code, out, _ = self.next("beta", ["v1.4.0"], "--allow-breaking", fragments=["breaking", "feature"])
        self.assertEqual((code, out), (0, "v2.0.0-beta.1"))

    def test_allowed_betas_continue(self):
        tags = ["v1.4.0", "v2.0.0-beta.1"]
        self.assertEqual(self.next("beta", tags, "--allow-breaking", fragments=["breaking"])[1], "v2.0.0-beta.2")

    def test_flag_without_breaking_fragments_changes_nothing(self):
        self.assertEqual(self.next("beta", ["v1.4.0"], "--allow-breaking", fragments=["feature"])[1],
                         "v1.5.0-beta.1")

    def test_refused_on_further_betas_until_the_major_line_has_one(self):
        code, _, err = self.next("beta", ["v1.4.0", "v1.4.1-beta.1"], "--from-tag", "v1.4.1-beta.1",
                                 fragments=["breaking"])
        self.assertEqual(code, 1)
        self.assertIn("--allow-breaking", err)

    def test_betas_on_the_major_line_need_no_flag(self):
        tags = ["v1.4.0", "v2.0.0-beta.1"]
        self.assertEqual(self.next("beta", tags, fragments=["breaking", "fix"])[1:], ("v2.0.0-beta.2", ""))
        tags += ["v2.0.0-beta.2"]
        self.assertEqual(self.next("beta", tags, fragments=["breaking"])[1], "v2.0.0-beta.3")
        self.assertEqual(self.next("beta", tags, "--from-tag", "v2.0.0-beta.2", fragments=["breaking"])[1],
                         "v2.0.0-beta.3")

    def test_a_beta_on_another_line_does_not_open_the_major_line(self):
        tags = ["v1.4.0", "v1.5.0-beta.1"]
        code, out, err = self.next("beta", tags, fragments=["breaking"])
        self.assertEqual((code, out), (1, ""))
        self.assertIn("--allow-breaking", err)

    def test_the_major_line_is_counted_from_the_last_stable(self):
        tags = ["v2.0.0-beta.1", "v2.0.0"]
        code, out, err = self.next("beta", tags, fragments=["breaking"])
        self.assertEqual((code, out), (1, ""))
        self.assertIn("--allow-breaking", err)
        self.assertEqual(self.next("beta", tags, "--allow-breaking", fragments=["breaking"])[1], "v3.0.0-beta.1")

    def test_breaking_already_in_the_last_stable_does_not_count_again(self):
        code, out, err = self.next("beta", ["v2.0.0"], fragments=["breaking"], released=[0])
        self.assertEqual((code, out, err), (0, "v2.0.1-beta.1", ""))

    def test_new_breaking_fragment_next_to_released_ones_still_needs_the_flag(self):
        code, out, _ = self.next("beta", ["v2.0.0"], fragments=["breaking", "breaking"], released=[0])
        self.assertEqual((code, out), (1, ""))


class FromTagTests(Env):
    def test_beta_from_a_beta_bumps_n(self):
        self.assertEqual(self.next("beta", ["v1.4.0", "v1.4.1-beta.2"], "--from-tag", "v1.4.1-beta.2")[1],
                         "v1.4.1-beta.3")

    def test_beta_from_a_beta_skips_taken_numbers(self):
        tags = ["v1.4.0", "v1.4.1-beta.2", "v1.4.1-beta.3"]
        self.assertEqual(self.next("beta", tags, "--from-tag", "v1.4.1-beta.2")[1], "v1.4.1-beta.4")

    def test_beta_from_a_stable_tag_is_refused(self):
        self.assertEqual(self.next("beta", ["v1.4.0"], "--from-tag", "v1.4.0")[0], 1)

    def test_malformed_from_tag(self):
        for tag in ("v1.4", "1.4.1-beta.1", "v1.4.1-beta.0", "", "latest"):
            for channel in ("beta", "stable"):
                with self.subTest((channel, tag)):
                    code, out, err = self.next(channel, ["v1.4.0"], "--from-tag", tag)
                    self.assertEqual((code, out), (1, ""))
                    self.assertIn("--from-tag", err)


class StableTests(Env):
    def test_promotes_the_given_beta(self):
        self.assertEqual(self.next("stable", ["v1.4.0", "v1.4.1-beta.1", "v1.4.1-beta.2"],
                                   "--from-tag", "v1.4.1-beta.1"), (0, "v1.4.1", ""))

    def test_without_from_tag_takes_the_newest_beta(self):
        tags = ["v1.4.0", "v1.4.1-beta.1", "v1.5.0-beta.1", "v1.5.0-beta.2"]
        self.assertEqual(self.next("stable", tags)[1], "v1.5.0")

    def test_without_from_tag_and_no_beta_is_refused(self):
        code, out, err = self.next("stable", ["v1.4.0", "v1.3.0-beta.1"])
        self.assertEqual((code, out), (1, ""))
        self.assertIn("beta", err.lower())

    def test_already_released_is_refused(self):
        code, _, err = self.next("stable", ["v1.4.0", "v1.4.1-beta.1", "v1.4.1"], "--from-tag", "v1.4.1-beta.1")
        self.assertEqual(code, 1)
        self.assertIn("v1.4.1", err)

    def test_older_than_the_latest_stable_is_refused(self):
        code, _, _ = self.next("stable", ["v1.5.0", "v1.4.1-beta.1"], "--from-tag", "v1.4.1-beta.1")
        self.assertEqual(code, 1)

    def test_from_a_stable_tag_is_refused(self):
        self.assertEqual(self.next("stable", ["v1.4.0"], "--from-tag", "v1.4.0")[0], 1)

    def test_from_tag_not_in_the_tag_list_is_refused(self):
        self.assertEqual(self.next("stable", ["v1.4.0"], "--from-tag", "v1.4.1-beta.1")[0], 1)

    def test_never_refuses_because_of_fragment_kinds(self):
        tags = ["v1.4.0", "v1.4.1-beta.1"]
        self.assertEqual(self.next("stable", tags, "--from-tag", "v1.4.1-beta.1", fragments=["breaking"]),
                         (0, "v1.4.1", ""))
        self.assertEqual(self.next("stable", tags, "--from-tag", "v1.4.1-beta.1", "--allow-breaking",
                                   fragments=["breaking"])[1], "v1.4.1")
        tags = ["v1.4.0", "v2.0.0-beta.1"]
        self.assertEqual(self.next("stable", tags, fragments=["breaking", "feature", "fix"])[1], "v2.0.0")

    def test_does_not_read_the_fragments_at_all(self):
        (self.changes / "bad.md").write_text("no front matter")
        self.assertEqual(self.next("stable", ["v1.4.0", "v1.4.1-beta.1"])[1], "v1.4.1")

    def test_stable_never_needs_the_plist(self):
        self.info.unlink()
        self.assertEqual(self.next("stable", ["v1.4.0", "v1.4.1-beta.1"], "--from-tag", "v1.4.1-beta.1")[1],
                         "v1.4.1")


class NewFragmentsTests(Env):

    def test_released_feature_fragment_does_not_bump_the_minor_again(self):
        code, out, err = self.next("beta", ["v1.4.0"], fragments=["feature"], released=[0])
        self.assertEqual((code, out, err), (0, "v1.4.1-beta.1", ""))

    def test_a_new_feature_next_to_a_released_one_does_count(self):
        self.assertEqual(self.next("beta", ["v1.4.0"], fragments=["feature", "feature"], released=[0])[1],
                         "v1.5.0-beta.1")

    def test_only_the_new_fragments_pick_the_kind(self):
        self.assertEqual(self.next("beta", ["v1.4.0"], fragments=["feature", "fix"], released=[0])[1],
                         "v1.4.1-beta.1")

    def test_nothing_released_means_everything_counts(self):
        self.assertEqual(self.next("beta", ["v1.4.0"], fragments=["feature", "fix"])[1], "v1.5.0-beta.1")

    def test_without_any_stable_tag_every_fragment_counts_and_git_is_not_asked(self):
        self.assertEqual(self.next("beta", [], fragments=["feature"])[1:], ("v1.3.0-beta.1", ""))
        code, out, err = self.next("beta", [], fragments=["breaking"])
        self.assertEqual((code, out), (1, ""))
        self.assertIn("--allow-breaking", err)

    def test_the_last_stable_not_the_newest_tag_is_the_reference(self):
        tags = ["v1.4.0", "v1.5.0-beta.1"]
        self.assertEqual(self.next("beta", tags, fragments=["feature"])[1], "v1.5.0-beta.2")

    def test_a_tag_the_repo_cannot_list_fails_instead_of_counting_everything(self):
        with tempfile.TemporaryDirectory() as d:
            env = dict(os.environ, GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_SYSTEM="/dev/null")
            subprocess.run(["git", "init", "-q", "-b", "main", d], env=env, check=True, capture_output=True)
            (self.changes / "f0.md").write_text(frag("feature"))
            tags_file = self.root / "tags.txt"
            tags_file.write_text("v1.4.0\n")
            code, out, err = run_cli(["next", "--channel", "beta", "--tags-file", str(tags_file), "--repo", d,
                                      "--changes-dir", str(self.changes), "--info-plist", str(self.info)])
        self.assertEqual((code, out), (1, ""))
        self.assertIn("v1.4.0", err)

    def test_a_relative_changes_dir_is_read_against_the_repo(self):
        self.tag_repo(["v1.4.0"], [])
        (self.changes / "f0.md").write_text(frag("feature"))
        (self.root / "tags.txt").write_text("v1.4.0\n")
        code, out, err = run_cli(["next", "--channel", "beta", "--tags-file", str(self.root / "tags.txt"),
                                  "--repo", str(self.root), "--changes-dir", "changes",
                                  "--info-plist", str(self.info)])
        self.assertEqual((code, out, err), (0, "v1.5.0-beta.1", ""))


class FloorTests(Env):

    def above(self, tags, new):
        parsed = [v for v in map(version.parse_tag, tags) if v]
        last = max(v for v in parsed if v[3] is None)
        for b in (v for v in parsed if v[3] is not None and v[:3] > last[:3]):
            self.assertGreater(version.build_number(version.parse_tag(new)), version.build_number(b), (new, b))

    def test_promoting_an_older_beta_while_a_newer_line_exists(self):
        tags = ["v1.2.0", "v1.2.1-beta.1", "v1.3.0-beta.1", "v1.2.1"]
        for fragments in ((), ("fix",), ("internal",)):
            with self.subTest(fragments):
                code, out, err = self.next("beta", tags, fragments=fragments)
                self.assertEqual((code, out, err), (0, "v1.3.0-beta.2", ""))
                self.above(tags, out)

    def test_the_fragment_line_wins_when_it_is_higher(self):
        tags = ["v1.2.0", "v1.2.1-beta.1", "v1.3.0-beta.1", "v1.2.1"]
        self.assertEqual(self.next("beta", tags, fragments=["feature"], released=())[1], "v1.3.0-beta.2")
        tags = ["v1.2.0", "v1.2.1-beta.1", "v1.2.1"]
        self.assertEqual(self.next("beta", tags, fragments=["feature"])[1], "v1.3.0-beta.1")

    def test_a_reverted_feature_fragment_does_not_drop_the_line(self):
        tags = ["v1.2.0", "v1.3.0-beta.1", "v1.3.0-beta.2"]
        for fragments in ((), ("fix",)):
            with self.subTest(fragments):
                code, out, err = self.next("beta", tags, fragments=fragments)
                self.assertEqual((code, out, err), (0, "v1.3.0-beta.3", ""))
                self.above(tags, out)

    def test_a_reverted_breaking_fragment_does_not_drop_the_major_line(self):
        tags = ["v1.4.0", "v2.0.0-beta.1"]
        self.assertEqual(self.next("beta", tags, fragments=["fix"])[1], "v2.0.0-beta.2")

    def test_the_highest_line_is_continued_not_the_highest_number(self):
        tags = ["v1.2.0", "v1.3.0-beta.7", "v1.2.1-beta.9"]
        self.assertEqual(self.next("beta", tags)[1], "v1.3.0-beta.8")

    def test_betas_of_released_versions_set_no_floor(self):
        tags = ["v1.3.0-beta.4", "v1.3.0", "v1.4.0"]
        self.assertEqual(self.next("beta", tags)[1], "v1.4.1-beta.1")

    def test_without_a_stable_tag_the_plist_version_is_the_floor_base(self):
        self.assertEqual(self.next("beta", ["v1.2.1-beta.1", "v1.3.0-beta.1"])[1], "v1.3.0-beta.2")
        self.assertEqual(self.next("beta", ["v1.1.0-beta.3"])[1], "v1.2.1-beta.1")

    def test_from_tag_below_the_highest_line_is_refused(self):
        tags = ["v1.2.0", "v1.2.1-beta.1", "v1.3.0-beta.1"]
        code, out, err = self.next("beta", tags, "--from-tag", "v1.2.1-beta.1")
        self.assertEqual((code, out), (1, ""))
        self.assertIn("v1.3.0-beta.1", err)

    def test_from_tag_of_a_released_line_is_refused(self):
        tags = ["v1.2.0", "v1.2.1-beta.1", "v1.2.1"]
        code, out, err = self.next("beta", tags, "--from-tag", "v1.2.1-beta.1")
        self.assertEqual((code, out), (1, ""))
        self.assertIn("v1.2.1", err)

    def test_from_tag_on_the_highest_line_continues_it(self):
        tags = ["v1.2.0", "v1.2.1-beta.1", "v1.3.0-beta.1"]
        self.assertEqual(self.next("beta", tags, "--from-tag", "v1.3.0-beta.1")[1], "v1.3.0-beta.2")


class LimitsTests(Env):
    def test_beta_number_past_98_is_refused(self):
        code, out, err = self.next("beta", ["v1.4.0", "v1.4.1-beta.98"])
        self.assertEqual((code, out), (1, ""))
        self.assertIn("98", err)

    def test_patch_or_minor_past_99_is_refused(self):
        self.assertEqual(self.next("beta", ["v1.4.99"])[0], 1)
        self.assertEqual(self.next("beta", ["v1.99.0"], fragments=["feature"])[0], 1)


class GitTagsTests(unittest.TestCase):
    def test_reads_git_tag_list_when_no_tags_file(self):
        with tempfile.TemporaryDirectory() as d:
            env = dict(os.environ, GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_SYSTEM="/dev/null")
            git = ["git", "-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"]

            def g(*a):
                subprocess.run([*git, *a], cwd=d, env=env, check=True, capture_output=True)

            g("init", "-q", "-b", "main")
            g("commit", "-q", "--allow-empty", "-m", "x")
            for t in ("v1.4.0", "v1.4.1-beta.1", "other"):
                g("tag", t)
            (Path(d) / "changes").mkdir()
            old = os.getcwd()
            os.chdir(d)
            try:
                code, out, err = run_cli(["next", "--channel", "beta"])
            finally:
                os.chdir(old)
            self.assertEqual((code, out, err), (0, "v1.4.1-beta.2", ""))


    def test_fragments_in_the_last_stable_tags_tree_do_not_count_again(self):
        with tempfile.TemporaryDirectory() as d:
            env = dict(os.environ, GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_SYSTEM="/dev/null")
            git = ["git", "-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"]

            def g(*a):
                subprocess.run([*git, *a], cwd=d, env=env, check=True, capture_output=True)

            def put(name, kind):
                (Path(d) / "changes").mkdir(exist_ok=True)
                (Path(d) / "changes" / name).write_text(frag(kind))

            g("init", "-q", "-b", "main")
            put("old.md", "feature")
            g("add", "-A")
            g("commit", "-q", "-m", "shipped")
            g("tag", "-a", "-m", "stable", "v1.4.0")
            put("fix.md", "fix")
            g("add", "-A")
            g("commit", "-q", "-m", "after")
            self.assertEqual(run_cli(["next", "--channel", "beta", "--repo", d])[:2], (0, "v1.4.1-beta.1"))
            old = os.getcwd()
            os.chdir(d)
            try:
                self.assertEqual(run_cli(["next", "--channel", "beta"])[:2], (0, "v1.4.1-beta.1"))
            finally:
                os.chdir(old)
            put("new.md", "feature")
            self.assertEqual(run_cli(["next", "--channel", "beta", "--repo", d])[:2], (0, "v1.5.0-beta.1"))


class BuildNumberTests(unittest.TestCase):
    def test_formula(self):
        self.assertEqual(version.build_number((1, 4, 1, 3)), 1 * 1000000 + 4 * 10000 + 1 * 100 + 3)
        self.assertEqual(version.build_number((1, 4, 1, None)), 1 * 1000000 + 4 * 10000 + 1 * 100 + 99)

    def test_ordering_matches_release_order(self):
        order = [(1, 2, 0, None), (1, 2, 1, 1), (1, 2, 1, 2), (1, 2, 1, 98), (1, 2, 1, None),
                 (1, 2, 2, 1), (1, 3, 0, 1), (1, 3, 0, None), (1, 10, 0, 1), (2, 0, 0, 1), (2, 0, 0, None)]
        numbers = [version.build_number(v) for v in order]
        self.assertEqual(numbers, sorted(numbers))
        self.assertEqual(len(set(numbers)), len(numbers))

    def test_stable_is_above_every_beta_of_it(self):
        self.assertGreater(version.build_number((3, 7, 5, None)), version.build_number((3, 7, 5, 98)))

    def test_out_of_range_is_refused(self):
        for v in ((1, 100, 0, None), (1, 0, 100, None), (1, 0, 0, 99), (1, 0, 0, 0)):
            with self.subTest(v):
                with self.assertRaises(ValueError):
                    version.build_number(v)

    def test_above_the_legacy_counter(self):
        self.assertGreater(version.build_number((1, 2, 0, None)), 3)


class PlistTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.app = Path(self._tmp.name) / "Notchling.app"
        (self.app / "Contents").mkdir(parents=True)
        self.path = self.app / "Contents" / "Info.plist"
        self.original = {
            "CFBundleIdentifier": "local.notchling",
            "CFBundleShortVersionString": "1.2.0",
            "CFBundleVersion": "3",
            "LSMinimumSystemVersion": "13.0",
            "LSUIElement": True,
        }

    def write(self, fmt):
        self.path.write_bytes(plistlib.dumps(self.original, fmt=fmt))

    def read(self):
        return plistlib.loads(self.path.read_bytes())

    def test_stable_tag(self):
        self.write(plistlib.FMT_XML)
        self.assertEqual(run_cli(["plist", str(self.app), "v1.4.1"])[0], 0)
        got = self.read()
        self.assertEqual(got["CFBundleShortVersionString"], "1.4.1")
        self.assertEqual(got["CFBundleVersion"], "1040199")
        self.assertEqual({k: v for k, v in got.items() if not k.startswith("CFBundleVersion")
                          and k != "CFBundleShortVersionString"},
                         {k: v for k, v in self.original.items() if not k.startswith("CFBundleVersion")
                          and k != "CFBundleShortVersionString"})

    def test_beta_tag_and_bare_version(self):
        self.write(plistlib.FMT_XML)
        self.assertEqual(run_cli(["plist", str(self.app), "1.4.1-beta.3"])[0], 0)
        got = self.read()
        self.assertEqual(got["CFBundleShortVersionString"], "1.4.1-beta.3")
        self.assertEqual(got["CFBundleVersion"], "1040103")

    def test_build_numbers_are_strings(self):
        self.write(plistlib.FMT_XML)
        run_cli(["plist", str(self.app), "v1.4.1"])
        self.assertIsInstance(self.read()["CFBundleVersion"], str)

    def test_binary_plist_stays_binary(self):
        self.write(plistlib.FMT_BINARY)
        self.assertEqual(run_cli(["plist", str(self.app), "v1.4.1-beta.1"])[0], 0)
        self.assertTrue(self.path.read_bytes().startswith(b"bplist"))
        self.assertEqual(self.read()["CFBundleShortVersionString"], "1.4.1-beta.1")

    def test_xml_plist_stays_xml(self):
        self.write(plistlib.FMT_XML)
        run_cli(["plist", str(self.app), "v1.4.1"])
        self.assertTrue(self.path.read_bytes().lstrip().startswith(b"<?xml"))

    def test_beta_sorts_before_its_stable_in_the_bundle(self):
        self.write(plistlib.FMT_XML)
        run_cli(["plist", str(self.app), "v1.4.1-beta.2"])
        beta = int(self.read()["CFBundleVersion"])
        run_cli(["plist", str(self.app), "v1.4.1"])
        self.assertGreater(int(self.read()["CFBundleVersion"]), beta)

    def test_refusals_leave_the_file_alone(self):
        self.write(plistlib.FMT_XML)
        before = self.path.read_bytes()
        for arg in ("v1.4", "latest", "", "v1.4.1-beta.0", "v1.100.0", "v1.4.1-beta.99"):
            with self.subTest(arg):
                code, out, err = run_cli(["plist", str(self.app), arg])
                self.assertEqual((code, out), (1, ""))
                self.assertTrue(err)
                self.assertEqual(self.path.read_bytes(), before)

    def test_missing_bundle_or_plist(self):
        code, _, err = run_cli(["plist", str(self.app.parent / "Nope.app"), "v1.4.1"])
        self.assertEqual(code, 1)
        self.assertTrue(err)
        code, _, _ = run_cli(["plist", str(self.app), "v1.4.1"])
        self.assertEqual(code, 1)

    def test_garbage_plist(self):
        self.path.write_text("not a plist")
        self.assertEqual(run_cli(["plist", str(self.app), "v1.4.1"])[0], 1)

    def test_does_not_touch_the_source_plist_path_given_as_a_non_bundle(self):
        src = Path(self._tmp.name) / "src"
        src.mkdir()
        (src / "Info.plist").write_bytes(plistlib.dumps(self.original))
        before = (src / "Info.plist").read_bytes()
        self.assertEqual(run_cli(["plist", str(src), "v1.4.1"])[0], 1)
        self.assertEqual((src / "Info.plist").read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
