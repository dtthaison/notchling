import os
import subprocess
import sys
import unittest
from pathlib import Path

from guard import featuremap
from tests.helpers import make_repo

ROOT = Path(__file__).resolve().parents[3]
PKG = ROOT / "tools/guard"
MAP = "docs/feature-map.md"


def repo_with(map_text, *files):
    return make_repo({MAP: map_text, **{f: "" for f in files}})


class FeatureMapTests(unittest.TestCase):
    def test_full_path_anywhere_counts(self):
        repo = repo_with("Prose names `crates/app/src/notch/` here.\n", "crates/app/src/notch/mod.rs")
        self.assertEqual(featuremap.check(repo), [])

    def test_short_path_in_its_crates_row_counts(self):
        row = "| Engine | `crates/engine/`; `src/reply/` and `src/viz/` | t | protected | E3 |\n"
        repo = repo_with(row, "crates/engine/src/reply/plan.rs", "crates/engine/src/viz/mod.rs")
        self.assertEqual(featuremap.check(repo), [])

    def test_short_path_in_another_crates_row_does_not_count(self):
        row = "| Kit | `crates/kit/`; `src/reply/` | t | auto | U0 |\n"
        problems = featuremap.check(repo_with(row, "crates/engine/src/reply/plan.rs"))
        self.assertEqual(len(problems), 1)
        self.assertIn("crates/engine/src/reply/", problems[0])

    def test_short_path_outside_a_table_does_not_count(self):
        text = "`crates/engine/` keeps `src/reply/`.\n"
        self.assertEqual(len(featuremap.check(repo_with(text, "crates/engine/src/reply/plan.rs"))), 1)

    def test_nested_short_path_is_not_a_crate_folder(self):
        row = "| App | `crates/app/`; `chat/src/view/` | t | auto | U1 |\n"
        self.assertEqual(len(featuremap.check(repo_with(row, "crates/app/src/view/mod.rs"))), 1)

    def test_files_directly_in_src_need_no_row(self):
        self.assertEqual(featuremap.check(repo_with("# Map\n", "crates/app/src/lib.rs")), [])

    def test_deep_files_count_for_their_top_folder(self):
        repo = repo_with("`crates/engine/src/tools/`\n", "crates/engine/src/tools/codex/plan.rs")
        self.assertEqual(featuremap.check(repo), [])

    def test_untracked_folder_is_ignored(self):
        repo = repo_with("# Map\n")
        (repo / "crates/app/src/scratch").mkdir(parents=True)
        (repo / "crates/app/src/scratch/a.rs").write_text("")
        self.assertEqual(featuremap.check(repo), [])

    def test_missing_map_fails(self):
        repo = make_repo({"crates/app/src/notch/mod.rs": ""})
        problems = featuremap.check(repo)
        self.assertTrue(problems and problems[0].startswith(featuremap.RULE), problems)

    def test_cli_exit_code(self):
        env = dict(os.environ, PYTHONPATH=str(PKG))
        bad = repo_with("# Map\n", "crates/app/src/notch/mod.rs")
        good = repo_with("`crates/app/src/notch/`\n", "crates/app/src/notch/mod.rs")
        for repo, code in ((bad, 1), (good, 0)):
            r = subprocess.run([sys.executable, "-m", "guard", "featuremap"], cwd=str(repo), env=env,
                               capture_output=True, text=True, timeout=60)
            self.assertEqual(r.returncode, code, r.stdout + r.stderr)

    def test_real_tree(self):
        self.assertEqual(featuremap.check(ROOT), [])


if __name__ == "__main__":
    unittest.main()
