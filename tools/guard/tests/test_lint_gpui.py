"""LINT-GPUI-MACOS-ONLY: a `gpui` dependency sits only under the macOS target table."""

import unittest
from pathlib import Path

from guard.lint import rule

RULE = rule("LINT-GPUI-MACOS-ONLY")
MANIFEST = "crates/app/Cargo.toml"
REPO = Path(".")


def check(text):
    return RULE.check(MANIFEST, text.splitlines(), REPO)


class GpuiMacosOnlyTests(unittest.TestCase):
    def test_plain_dependencies_table_fails(self):
        found = check('[package]\nname = "x"\n\n[dependencies]\ngpui = "=0.2.2"\n')
        self.assertEqual([line for line, _ in found], [5])

    def test_macos_target_table_passes(self):
        self.assertEqual(check(
            "[target.'cfg(target_os = \"macos\")'.dependencies]\n"
            'gpui = { version = "=0.2.2", features = ["runtime_shaders"] }\n'
            "[target.'cfg(target_os = \"macos\")'.dev-dependencies]\n"
            'gpui = "=0.2.2"\n'), [])

    def test_double_quoted_cfg_passes_too(self):
        self.assertEqual(check('[target."cfg(target_os = \\"macos\\")".dependencies]\ngpui = "=0.2.2"\n'), [])

    def test_other_target_fails(self):
        found = check("[target.'cfg(unix)'.dependencies]\ngpui = \"=0.2.2\"\n")
        self.assertEqual(len(found), 1)

    def test_table_form_fails_outside_macos(self):
        found = check('[dependencies.gpui]\nversion = "=0.2.2"\n')
        self.assertEqual([line for line, _ in found], [1])
        self.assertEqual(check("[target.'cfg(target_os = \"macos\")'.dependencies.gpui]\nversion = \"=0.2.2\"\n"), [])

    def test_comments_and_other_crates_are_ignored(self):
        self.assertEqual(check('[dependencies]\n# gpui = "=0.2.2"\ngpui_macros_like = "1"\nserde = "1"\n'), [])

    def test_applies_to_crate_manifests_only(self):
        self.assertTrue(RULE.applies("crates/kit/Cargo.toml"))
        self.assertTrue(RULE.applies("crates/engine/Cargo.toml"))
        self.assertFalse(RULE.applies("Cargo.toml"))
        self.assertFalse(RULE.applies("crates/kit/src/lib.rs"))

    def test_real_manifests_pass(self):
        repo = Path(__file__).resolve().parents[3]
        for manifest in sorted(repo.glob("crates/*/Cargo.toml")):
            path = str(manifest.relative_to(repo))
            self.assertEqual(RULE.check(path, manifest.read_text().splitlines(), repo), [], path)


if __name__ == "__main__":
    unittest.main()
