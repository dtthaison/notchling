"""Changelog fragments: parse, check (PR needs one), notes and fold. Temp dirs and temp git repos only."""

from __future__ import annotations

import io
import os
import subprocess
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

import fragments

CHANGELOG = """# Changelog

Notable changes to Notchling.

## [Unreleased]

## [1.2.0] - 2026-11-07

### Added

- Old feature.

[Unreleased]: https://github.com/dtthaison/notchling/compare/v1.2.0...HEAD
[1.2.0]: https://github.com/dtthaison/notchling/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/dtthaison/notchling/releases/tag/v1.1.0
"""


def frag(kind, text="Something changed."):
    return f"---\nkind: {kind}\n---\n{text}\n"


def run_cli(argv):
    out, err = io.StringIO(), io.StringIO()
    with redirect_stdout(out), redirect_stderr(err):
        code = fragments.main(argv)
    return code, out.getvalue(), err.getvalue()


class Dir(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        self.changes = self.root / "changes"
        self.changes.mkdir()

    def put(self, name, text):
        (self.changes / name).write_text(text)


class ParseTests(unittest.TestCase):
    def test_parses_kind_and_one_paragraph(self):
        f = fragments.parse("a.md", "---\nkind: fix\n---\nThe reply\nno longer drops.\n")
        self.assertEqual((f.name, f.kind, f.text), ("a.md", "fix", "The reply no longer drops."))

    def test_every_kind_is_accepted(self):
        for kind in ("fix", "feature", "breaking", "internal"):
            self.assertEqual(fragments.parse("a.md", frag(kind)).kind, kind)

    def test_errors(self):
        bad = {
            "no front matter": "Just text.\n",
            "unterminated": "---\nkind: fix\nText\n",
            "missing kind": "---\n---\nText.\n",
            "unknown kind": frag("bugfix"),
            "empty kind": "---\nkind:\n---\nText.\n",
            "unknown key": "---\nkind: fix\nowner: me\n---\nText.\n",
            "duplicate key": "---\nkind: fix\nkind: feature\n---\nText.\n",
            "empty body": "---\nkind: fix\n---\n\n",
            "two paragraphs": "---\nkind: fix\n---\nOne.\n\nTwo.\n",
            "line without colon": "---\nkind fix\n---\nText.\n",
        }
        for label, text in bad.items():
            with self.subTest(label):
                with self.assertRaises(fragments.FragmentError) as cm:
                    fragments.parse("a.md", text)
                self.assertIn("a.md", str(cm.exception))

    def test_crlf_and_trailing_spaces(self):
        f = fragments.parse("a.md", "---\r\nkind: fix \r\n---\r\nText here.  \r\n")
        self.assertEqual((f.kind, f.text), ("fix", "Text here."))


class LoadTests(Dir):
    def test_readme_and_non_markdown_are_not_fragments(self):
        self.put("README.md", "Explains fragments, no front matter.")
        self.put("notes.txt", "x")
        self.put("b.md", frag("fix"))
        self.put("a.md", frag("feature"))
        self.assertEqual([f.name for f in fragments.load(self.changes)], ["a.md", "b.md"])

    def test_missing_dir_is_empty(self):
        self.assertEqual(fragments.load(self.root / "nope"), [])

    def test_parse_error_names_the_file(self):
        self.put("bad.md", "no front matter")
        with self.assertRaises(fragments.FragmentError) as cm:
            fragments.load(self.changes)
        self.assertIn("bad.md", str(cm.exception))


class NotesTests(Dir):
    def test_grouped_by_kind_internal_left_out(self):
        self.put("1.md", frag("fix", "Fixed a thing."))
        self.put("2.md", frag("feature", "Added a thing."))
        self.put("3.md", frag("internal", "Refactored."))
        self.put("4.md", frag("breaking", "Stored data moved."))
        code, out, _ = run_cli(["notes", "--changes-dir", str(self.changes)])
        self.assertEqual(code, 0)
        self.assertEqual(
            out,
            "### Added\n\n- Added a thing.\n\n"
            "### Changed\n\n- **Breaking:** Stored data moved.\n\n"
            "### Fixed\n\n- Fixed a thing.\n",
        )

    def test_nothing_user_facing(self):
        self.put("3.md", frag("internal"))
        code, out, _ = run_cli(["notes", "--changes-dir", str(self.changes)])
        self.assertEqual((code, out), (0, "No user-facing changes.\n"))

    def test_parse_error_exits_1(self):
        self.put("bad.md", "nope")
        code, out, err = run_cli(["notes", "--changes-dir", str(self.changes)])
        self.assertEqual((code, out), (1, ""))
        self.assertIn("bad.md", err)


class FoldTests(Dir):
    def setUp(self):
        super().setUp()
        self.log = self.root / "CHANGELOG.md"
        self.log.write_text(CHANGELOG)

    def fold(self, version="1.3.0", date="2026-11-09"):
        return run_cli(["fold", version, "--changelog", str(self.log), "--changes-dir", str(self.changes),
                        "--date", date])

    def test_inserts_section_links_and_deletes_fragments(self):
        self.put("README.md", "stays")
        self.put("1.md", frag("feature", "Added a thing."))
        self.put("2.md", frag("fix", "Fixed a thing."))
        self.put("3.md", frag("internal", "Not in the log."))
        code, _, err = self.fold()
        self.assertEqual(code, 0, err)
        self.assertEqual(
            self.log.read_text(),
            CHANGELOG.replace(
                "## [Unreleased]\n\n",
                "## [Unreleased]\n\n## [1.3.0] - 2026-11-09\n\n"
                "### Added\n\n- Added a thing.\n\n### Fixed\n\n- Fixed a thing.\n\n",
            ).replace(
                "[Unreleased]: https://github.com/dtthaison/notchling/compare/v1.2.0...HEAD\n",
                "[Unreleased]: https://github.com/dtthaison/notchling/compare/v1.3.0...HEAD\n"
                "[1.3.0]: https://github.com/dtthaison/notchling/compare/v1.2.0...v1.3.0\n",
            ),
        )
        self.assertEqual(sorted(p.name for p in self.changes.iterdir()), ["README.md"])

    def test_v_prefix_accepted(self):
        self.put("1.md", frag("fix"))
        self.assertEqual(self.fold("v1.2.1")[0], 0)
        self.assertIn("## [1.2.1] - 2026-11-09", self.log.read_text())

    def test_existing_unreleased_text_is_kept_and_merged(self):
        self.log.write_text(CHANGELOG.replace(
            "## [Unreleased]\n", "## [Unreleased]\n\n### Added\n\n- By hand.\n\n### Changed\n\n- Hand change.\n"))
        self.put("1.md", frag("feature", "From fragment."))
        self.put("2.md", frag("fix", "Fix from fragment."))
        self.assertEqual(self.fold()[0], 0)
        text = self.log.read_text()
        section = text.split("## [1.3.0] - 2026-11-09\n")[1].split("## [1.2.0]")[0]
        self.assertEqual(
            section,
            "\n### Added\n\n- By hand.\n- From fragment.\n\n### Changed\n\n- Hand change.\n\n"
            "### Fixed\n\n- Fix from fragment.\n\n",
        )
        self.assertTrue(text.split("\n## [1.3.0]")[0].rstrip().endswith("## [Unreleased]"))

    def test_nothing_user_facing_still_cuts_the_section_and_clears_fragments(self):
        self.put("1.md", frag("internal"))
        code, _, _ = self.fold()
        self.assertEqual(code, 0)
        self.assertIn("## [1.3.0] - 2026-11-09\n\nNo user-facing changes.\n\n## [1.2.0]", self.log.read_text())
        self.assertEqual(list(self.changes.iterdir()), [])

    def test_refusals_change_nothing(self):
        self.put("1.md", frag("fix"))
        cases = {
            "beta": ("1.3.0-beta.1", "2026-11-09"),
            "malformed": ("1.3", "2026-11-09"),
            "bad date": ("1.3.0", "11/09/2026"),
            "already there": ("1.2.0", "2026-11-09"),
        }
        for label, (version, date) in cases.items():
            with self.subTest(label):
                code, _, err = self.fold(version, date)
                self.assertEqual(code, 1)
                self.assertTrue(err)
                self.assertEqual(self.log.read_text(), CHANGELOG)
                self.assertEqual(sorted(p.name for p in self.changes.iterdir()), ["1.md"])

    def test_bad_fragment_changes_nothing(self):
        self.put("1.md", frag("fix"))
        self.put("2.md", "broken")
        code, _, err = self.fold()
        self.assertEqual(code, 1)
        self.assertIn("2.md", err)
        self.assertEqual(self.log.read_text(), CHANGELOG)
        self.assertEqual(sorted(p.name for p in self.changes.iterdir()), ["1.md", "2.md"])

    def test_no_unreleased_heading(self):
        self.log.write_text("# Changelog\n")
        self.put("1.md", frag("fix"))
        self.assertEqual(self.fold()[0], 1)

    def test_changelog_without_compare_links_gets_none(self):
        self.log.write_text("# Changelog\n\n## [Unreleased]\n\n## [1.2.0] - 2026-11-07\n\n- x\n")
        self.put("1.md", frag("fix", "F."))
        self.assertEqual(self.fold()[0], 0)
        self.assertEqual(
            self.log.read_text(),
            "# Changelog\n\n## [Unreleased]\n\n## [1.3.0] - 2026-11-09\n\n### Fixed\n\n- F.\n\n"
            "## [1.2.0] - 2026-11-07\n\n- x\n",
        )

    def test_default_date_is_today_utc(self):
        self.put("1.md", frag("fix"))
        code, _, _ = run_cli(["fold", "1.3.0", "--changelog", str(self.log), "--changes-dir", str(self.changes)])
        self.assertEqual(code, 0)
        import datetime
        today = datetime.datetime.now(datetime.timezone.utc).date().isoformat()
        self.assertIn(f"## [1.3.0] - {today}", self.log.read_text())


class Repo(unittest.TestCase):
    """A temp git repo with the paths the check cares about."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        self.git("init", "-q", "-b", "main")
        self.write("README.md", "hi\n")
        self.write("changes/README.md", "about fragments\n")
        self.write("legacy/macos/Sources/A.swift", "a\n")
        self.base = self.commit("base")

    def git(self, *args):
        env = dict(os.environ, GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_SYSTEM="/dev/null")
        r = subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.com",
                            "-c", "commit.gpgsign=false", *args], cwd=self.root, env=env,
                           capture_output=True, text=True, check=True)
        return r.stdout.strip()

    def write(self, rel, text):
        p = self.root / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)

    def commit(self, msg):
        self.git("add", "-A")
        self.git("commit", "-q", "-m", msg)
        return self.git("rev-parse", "HEAD")

    def check(self, *extra):
        return run_cli(["check", "--base", self.base, "--head", self.git("rev-parse", "HEAD"),
                        "--repo", str(self.root), *extra])


class CheckTests(Repo):
    def test_app_code_without_fragment_fails(self):
        for path in ("legacy/macos/Sources/Notch/B.swift", "apps/flutter/lib/main.dart",
                     "crates/engine/src/lib.rs", "crates/platform/mac/src/x.rs"):
            with self.subTest(path):
                self.git("reset", "-q", "--hard", self.base)
                self.write(path, "x\n")
                self.commit("change")
                code, _, err = self.check()
                self.assertEqual(code, 1)
                self.assertIn(path, err)
                self.assertIn("changes/", err)

    def test_non_app_changes_pass(self):
        for path in ("docs/x.md", "legacy/macos/Tests/T.swift", "crates/engine/Cargo.toml",
                     "crates/engine/tests/it.rs", "apps/flutter/test/t.dart", ".github/workflows/x.yml"):
            with self.subTest(path):
                self.git("reset", "-q", "--hard", self.base)
                self.write(path, "x\n")
                self.commit("change")
                self.assertEqual(self.check()[0], 0)

    def test_added_fragment_passes(self):
        self.write("legacy/macos/Sources/Notch/B.swift", "x\n")
        self.write("changes/fix-notch.md", frag("fix"))
        self.commit("change")
        self.assertEqual(self.check()[0], 0)

    def test_internal_fragment_passes(self):
        self.write("crates/engine/src/lib.rs", "x\n")
        self.write("changes/refactor.md", frag("internal"))
        self.commit("change")
        self.assertEqual(self.check()[0], 0)

    def test_added_fragment_must_parse(self):
        self.write("legacy/macos/Sources/Notch/B.swift", "x\n")
        self.write("changes/bad.md", "no front matter")
        self.commit("change")
        code, _, err = self.check()
        self.assertEqual(code, 1)
        self.assertIn("changes/bad.md", err)

    def test_bad_fragment_fails_even_without_app_code(self):
        self.write("changes/bad.md", "no front matter")
        self.commit("change")
        self.assertEqual(self.check()[0], 1)

    def test_readme_edit_is_not_a_fragment(self):
        self.write("legacy/macos/Sources/Notch/B.swift", "x\n")
        self.write("changes/README.md", "more text\n")
        self.commit("change")
        self.assertEqual(self.check()[0], 1)

    def test_modified_fragment_does_not_count(self):
        self.write("changes/old.md", frag("fix"))
        self.base = self.commit("old fragment")
        self.write("changes/old.md", frag("fix", "Edited."))
        self.write("legacy/macos/Sources/Notch/B.swift", "x\n")
        self.commit("change")
        self.assertEqual(self.check()[0], 1)

    def test_deleted_fragment_does_not_count(self):
        self.write("changes/old.md", frag("fix"))
        self.base = self.commit("old fragment")
        (self.root / "changes/old.md").unlink()
        self.write("legacy/macos/Sources/Notch/B.swift", "x\n")
        self.commit("change")
        self.assertEqual(self.check()[0], 1)

    def test_no_changelog_label_skips(self):
        self.write("legacy/macos/Sources/Notch/B.swift", "x\n")
        self.commit("change")
        self.assertEqual(self.check("--label", "no-changelog")[0], 0)

    def test_other_labels_do_not_skip(self):
        self.write("legacy/macos/Sources/Notch/B.swift", "x\n")
        self.commit("change")
        self.assertEqual(self.check("--label", "bug", "--label", "docs")[0], 1)

    def test_no_changelog_label_does_not_excuse_a_bad_fragment(self):
        self.write("changes/bad.md", "nope")
        self.commit("change")
        self.assertEqual(self.check("--label", "no-changelog")[0], 1)

    def test_deleting_app_code_counts(self):
        (self.root / "legacy/macos/Sources/A.swift").unlink()
        self.commit("delete")
        self.assertEqual(self.check()[0], 1)

    def test_bad_revision_fails(self):
        code, _, err = run_cli(["check", "--base", "0" * 40, "--head", "HEAD", "--repo", str(self.root)])
        self.assertEqual(code, 1)
        self.assertTrue(err)

    def test_uses_merge_base_so_base_moving_on_is_not_a_change(self):
        # main gains app code after the PR branched; the PR itself touches only docs.
        self.git("checkout", "-q", "-b", "pr")
        self.write("docs/x.md", "x\n")
        self.commit("pr")
        self.git("checkout", "-q", "main")
        self.write("legacy/macos/Sources/Notch/B.swift", "x\n")
        main_tip = self.commit("main moves on")
        code, _, _ = run_cli(["check", "--base", main_tip, "--head", "pr", "--repo", str(self.root)])
        self.assertEqual(code, 0)


class NotesSinceTests(Repo):
    def test_since_tag_leaves_out_fragments_already_there(self):
        self.write("changes/old.md", frag("fix", "Old fix."))
        self.commit("old")
        self.git("tag", "v1.4.1-beta.1")
        self.write("changes/new.md", frag("feature", "New feature."))
        self.commit("new")
        code, out, err = run_cli(["notes", "--since", "v1.4.1-beta.1", "--repo", str(self.root)])
        self.assertEqual(code, 0, err)
        self.assertEqual(out, "### Added\n\n- New feature.\n")
        code, out, _ = run_cli(["notes", "--repo", str(self.root)])
        self.assertIn("Old fix.", out)
        self.assertIn("New feature.", out)

    def test_unknown_tag_fails(self):
        code, _, err = run_cli(["notes", "--since", "v9.9.9", "--repo", str(self.root)])
        self.assertEqual(code, 1)
        self.assertIn("v9.9.9", err)


if __name__ == "__main__":
    unittest.main()
