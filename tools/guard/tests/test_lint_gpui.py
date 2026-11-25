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

    def test_applies_to_the_root_and_crate_manifests_only(self):
        self.assertTrue(RULE.applies("crates/kit/Cargo.toml"))
        self.assertTrue(RULE.applies("crates/engine/Cargo.toml"))
        self.assertTrue(RULE.applies("Cargo.toml"), "[workspace.dependencies] is in the root manifest")
        self.assertFalse(RULE.applies("crates/kit/src/lib.rs"))
        self.assertFalse(RULE.applies("apps/flutter/pubspec.yaml"))

    def test_real_manifests_pass(self):
        repo = Path(__file__).resolve().parents[3]
        manifests = sorted(repo.glob("crates/*/Cargo.toml")) + [repo / "Cargo.toml"]
        for manifest in manifests:
            path = str(manifest.relative_to(repo))
            self.assertEqual(RULE.check(path, manifest.read_text().splitlines(), repo), [], path)


def lines(text, path=MANIFEST):
    return [line for line, _ in RULE.check(path, text.splitlines(), REPO)]


class BypassTests(unittest.TestCase):

    def test_a_dotted_key_fails(self):
        self.assertEqual(lines("[dependencies]\ngpui.workspace = true\n"), [2])
        self.assertEqual(lines('[dependencies]\ngpui.version = "=0.2.2"\n'), [2])
        self.assertEqual(lines('[dependencies]\ngpui . version = "=0.2.2"\n'), [2], "spaces around the dot")

    def test_a_renamed_dependency_fails(self):
        self.assertEqual(lines('[dependencies]\nui = { package = "gpui" }\n'), [2])
        self.assertEqual(lines('[dependencies]\nui = { version = "=0.2.2", package = "gpui", features = ["a"] }\n'), [2])
        self.assertEqual(lines("[dependencies]\nui = { package = 'gpui' }\n"), [2], "a literal string")
        self.assertEqual(lines('[dependencies]\nui.package = "gpui"\n'), [2])
        self.assertEqual(lines('[dependencies.ui]\nversion = "=0.2.2"\npackage = "gpui"\n'), [3])
        self.assertEqual(lines('[dev-dependencies]\nui = { package = "gpui" }\n'), [2])

    def test_a_rename_of_another_crate_or_under_macos_passes(self):
        self.assertEqual(lines('[dependencies]\nui = { package = "gpui_macros" }\nx = { package = "serde" }\n'), [])
        self.assertEqual(lines("[target.'cfg(target_os = \"macos\")'.dependencies]\nui = { package = \"gpui\" }\n"), [])

    def test_a_quoted_key_fails(self):
        self.assertEqual(lines('[dependencies]\n"gpui" = "=0.2.2"\n'), [2])
        self.assertEqual(lines("[dependencies]\n'gpui' = \"=0.2.2\"\n"), [2])
        self.assertEqual(lines('[dependencies]\n"gpui".version = "=0.2.2"\n'), [2])
        self.assertEqual(lines('[dependencies]\n"\\u0067pui" = "=0.2.2"\n'), [2], "an escape spelling the name")
        self.assertEqual(lines('[dependencies]\n"gpui_macros" = "1"\n'), [])

    def test_a_quoted_or_spaced_table_header_fails(self):
        self.assertEqual(lines('[dependencies."gpui"]\nversion = "=0.2.2"\n'), [1])
        self.assertEqual(lines("[dependencies.'gpui']\nversion = \"=0.2.2\"\n"), [1])
        self.assertEqual(lines('[ dependencies . gpui ]\nversion = "=0.2.2"\n'), [1])
        self.assertEqual(lines('["dependencies"]\ngpui = "=0.2.2"\n'), [2])
        self.assertEqual(lines('[build-dependencies]\ngpui = "=0.2.2"\n'), [2])
        self.assertEqual(lines('[target.\'cfg(unix)\'.build-dependencies.gpui]\nversion = "1"\n'), [1])

    def test_an_inline_dependencies_table_fails(self):
        self.assertEqual(lines('dependencies = { gpui = "=0.2.2" }\n'), [1])
        self.assertEqual(lines('[workspace]\ndependencies = { serde = "1", gpui = { version = "1" } }\n', "Cargo.toml"), [2])
        self.assertEqual(lines('dependencies = { ui = { package = "gpui" } }\n'), [1])
        self.assertEqual(lines('dependencies = { gpui = {} }\n'), [1])
        self.assertEqual(lines("[target.'cfg(target_os = \"macos\")']\ndependencies = { gpui = \"1\" }\n"), [])

    def test_the_workspace_dependencies_of_the_root_manifest_fail(self):
        root = "Cargo.toml"
        self.assertEqual(lines('[workspace.dependencies]\ngpui = "=0.2.2"\n', root), [2])
        self.assertEqual(lines('[workspace.dependencies]\ngpui = { version = "=0.2.2" }\n', root), [2])
        self.assertEqual(lines('[workspace.dependencies]\nui = { package = "gpui" }\n', root), [2])
        self.assertEqual(lines('[workspace.dependencies.gpui]\nversion = "=0.2.2"\n', root), [1])
        self.assertEqual(lines('[workspace]\ndependencies.gpui = "=0.2.2"\n', root), [2])
        self.assertEqual(lines('[workspace.dependencies]\nserde = "1"\n', root), [])
        self.assertEqual(lines('[workspace]\nmembers = ["crates/*"]\n[workspace.package]\nname = "gpui"\n', root), [])

    def test_values_that_go_on_over_lines_are_not_read_as_keys(self):
        text = (
            "[dependencies]\n"
            "serde = { version = \"1\", features = [\n"
            "    \"gpui\",\n"
            "    # gpui = 1\n"
            "] }\n"
            "multi = [\n"
            "  \"gpui\", # trailing\n"
            "]\n"
        )
        self.assertEqual(lines(text), [])
        text = '[dependencies]\ndoc = """\ngpui = "x"\n[dependencies.gpui]\n"""\nserde = "1"\n'
        self.assertEqual(lines(text), [])
        self.assertEqual(lines('[dependencies]\ndoc = """\nx\n"""\ngpui = "1"\n'), [5])
        self.assertEqual(lines('[dependencies]\nui = {\n  package = "gpui",\n  version = "1",\n}\n'), [2])

    def test_other_crates_names_and_text_are_ignored(self):
        text = (
            '[package]\nname = "gpui"\ndescription = "gpui = the framework # not a dep"\n'
            '[features]\ngpui = ["dep:other"]\n'
            '[dependencies]\ngpui-component = "1"\ngpui_macros = "1"\nserde = { version = "1" } # gpui = 1\n'
        )
        self.assertEqual(lines(text), [])

    def test_a_hash_inside_a_string_is_not_a_comment(self):
        self.assertEqual(lines('[dependencies]\nx = { version = "1#2" }\ngpui = "=0.2.2"\n'), [3])
        self.assertEqual(lines('[dependencies]\nx = "a # b"\n# gpui = 1\n'), [])


if __name__ == "__main__":
    unittest.main()
