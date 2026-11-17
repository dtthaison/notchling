import unittest
from pathlib import Path

from guard.changes import Change
from guard.zones import Zones, case_shadowed, glob_regex, load, protected_paths

ZONES = Path(__file__).resolve().parent.parent / "zones.json"
Z = load(ZONES)


def chg(path, status="M", old=None):
    return Change(path, status, old)


class ZoneTests(unittest.TestCase):
    def test_view_in_zone(self):
        self.assertTrue(Z.in_zone("crates/app/src/chat/view/row.rs"))

    def test_model_protected(self):
        self.assertFalse(Z.in_zone("crates/engine/src/reply/plan.rs"))

    def test_notch_controller_protected(self):
        self.assertFalse(Z.in_zone("crates/notchlingd/src/main.rs"))

    def test_notch_views_in_zone(self):
        for name in ["view", "pet", "perch"]:
            self.assertTrue(Z.in_zone(f"crates/app/src/notch/{name}.rs"), name)
        self.assertTrue(Z.in_zone("crates/kit/src/button.rs"))

    def test_transcript_readers_in_zone(self):
        for p in ["crates/engine/src/transcript/reader.rs", "crates/engine/src/search/rank.rs"]:
            self.assertTrue(Z.in_zone(p), p)

    def test_reply_sender_protected(self):
        self.assertFalse(Z.in_zone("crates/engine/src/reply/plan.rs"))

    def test_docs_in_zone_but_manual_protected(self):
        self.assertTrue(Z.in_zone("docs/chat-design.md"))
        for p in ["docs/rules.md", "docs/goals.md", "docs/decisions/0001-gpui-spike.md"]:
            self.assertFalse(Z.in_zone(p), p)

    def test_phone_features_in_zone_pairing_not(self):
        self.assertTrue(Z.in_zone("apps/flutter/lib/features/chat/chat_page.dart"))
        self.assertFalse(Z.in_zone("apps/flutter/lib/features/pairing/pair.dart"))
        self.assertFalse(Z.in_zone("apps/flutter/lib/core/pairing/keys.dart"))

    def test_crate_sources_in_zone_but_manifests_and_build_scripts_not(self):
        for p in ["crates/app/src/view.rs", "crates/kit/src/button.rs", "crates/engine/src/transcript/reader.rs",
                  "crates/engine/src/search/rank.rs"]:
            self.assertTrue(Z.in_zone(p), p)
        for p in ["crates/app/Cargo.toml", "crates/kit/Cargo.toml", "crates/kit/build.rs", "crates/app/build.rs",
                  "crates/app/src/build.rs", "crates/kit/src/nested/Cargo.toml", "crates/engine/Cargo.toml"]:
            self.assertFalse(Z.in_zone(p), p)

    def test_changing_a_crate_manifest_or_build_script_is_protected(self):
        for p in ["crates/app/Cargo.toml", "crates/kit/build.rs"]:
            self.assertEqual(protected_paths([chg(p, "M")], Z), [p], p)
            self.assertEqual(protected_paths([chg(p, "A")], Z), [p], p)
        self.assertEqual(protected_paths([chg("crates/app/src/view.rs", "M")], Z), [])

    def test_guard_and_github_protected(self):
        for p in ["tools/guard/zones.json", ".github/workflows/guard.yml", ".github/CODEOWNERS",
                  "CLAUDE.md", "AGENTS.md", "crates/app/AGENTS.md", ".claude/settings.json"]:
            self.assertFalse(Z.in_zone(p), p)

    def test_star_does_not_cross_folders(self):
        self.assertFalse(Z.in_zone("locales/en/consent.ftl"))
        self.assertTrue(Z.in_zone("locales/en/mac/consent.ftl"))

    def test_new_test_ok_edit_not(self):
        self.assertEqual(protected_paths([chg("apps/flutter/test/chat/new_test.dart", "A")], Z), [])
        self.assertEqual(protected_paths([chg("apps/flutter/test/chat/chat_test.dart", "M")], Z),
                         ["apps/flutter/test/chat/chat_test.dart"])
        self.assertEqual(protected_paths([chg("apps/flutter/test/chat/chat_test.dart", "D")], Z),
                         ["apps/flutter/test/chat/chat_test.dart"])

    def test_rename_out_of_protected_is_protected(self):
        self.assertEqual(protected_paths([chg("crates/engine/src/search/x.rs", "R",
                                              old="crates/engine/src/reply/x.rs")], Z),
                         ["crates/engine/src/reply/x.rs"])

    def test_rename_into_protected_is_protected(self):
        self.assertEqual(protected_paths([chg("crates/engine/src/reply/x.rs", "R",
                                              old="crates/engine/src/search/x.rs")], Z),
                         ["crates/engine/src/reply/x.rs"])

    def test_deleting_zone_file_ok(self):
        self.assertEqual(protected_paths([chg("crates/engine/src/search/old.rs", "D")], Z), [])

    def test_spec_protected_files_in_zone_folders_are_protected(self):
        for p in ["docs/AGENTS.md", "docs/CLAUDE.md", "crates/app/src/CLAUDE.md",
                  "crates/app/src/AGENTS.md", "crates/app/src/.claude/skills/x/SKILL.md",
                  "docs/.claude/settings.json", "crates/app/src/.gitattributes",
                  "apps/flutter/lib/features/chat/.gitattributes", "docs/.gitattributes", "docs/CODEOWNERS",
                  "crates/app/src/notch/CODEOWNERS"]:
            self.assertFalse(Z.in_zone(p), p)

    def test_lookalike_names_stay_in_zone(self):
        for p in ["docs/NOTAGENTS.md", "docs/agents-notes.md", "docs/claude/x.md", "docs/gitattributes.md"]:
            self.assertTrue(Z.in_zone(p), p)

    def test_leading_double_star_matches_at_the_root_too(self):
        rx = glob_regex("**/AGENTS.md")
        self.assertTrue(rx.match("AGENTS.md"))
        self.assertTrue(rx.match("crates/x/AGENTS.md"))
        self.assertFalse(rx.match("XAGENTS.md"))
        self.assertFalse(rx.match("docs/NOTAGENTS.md"))
        rx = glob_regex("**/.claude/**")
        self.assertTrue(rx.match(".claude/settings.json"))
        self.assertTrue(rx.match("a/b/.claude/skills/x"))
        self.assertFalse(rx.match("a/claude/x"))

    def test_middle_double_star_matches_zero_or_more_folders(self):
        rx = glob_regex("apps/flutter/**/dart_test.yaml")
        self.assertTrue(rx.match("apps/flutter/dart_test.yaml"))
        self.assertTrue(rx.match("apps/flutter/test/a/dart_test.yaml"))

    def test_root_exclusion_applies(self):
        z = Zones(("**", "!**/AGENTS.md"), ())
        self.assertFalse(z.in_zone("AGENTS.md"))
        self.assertFalse(z.in_zone("a/b/AGENTS.md"))
        self.assertTrue(z.in_zone("a/b.md"))

    def test_new_file_cannot_be_a_spec_protected_file(self):
        for p in ["apps/flutter/test/AGENTS.md", "apps/flutter/test/CLAUDE.md", "apps/flutter/test/.claude/x.md",
                  "apps/flutter/test/chat/.gitattributes", "apps/flutter/test/.gitattributes", "apps/flutter/test/CODEOWNERS"]:
            self.assertEqual(protected_paths([chg(p, "A")], Z), [p], p)

    def test_new_flutter_test_config_and_dart_test_yaml_are_protected(self):
        for p in ["apps/flutter/test/flutter_test_config.dart", "apps/flutter/test/chat/flutter_test_config.dart",
                  "apps/flutter/test/dart_test.yaml", "apps/flutter/test/goldens/dart_test.yaml"]:
            self.assertEqual(protected_paths([chg(p, "A")], Z), [p], p)

    def test_new_phone_test_still_ok(self):
        self.assertEqual(protected_paths([chg("apps/flutter/test/chat/new_test.dart", "A")], Z), [])
        self.assertEqual(protected_paths([chg("apps/flutter/test/goldens/new.png", "A")], Z), [])

    def test_case_shadowed_zone_folders(self):
        for p in ["docs/Decisions/x.md", "docs/DECISIONS/x.md", "apps/flutter/lib/features/Pairing/pair.dart",
                  "docs/Rules.md", "docs/Goals.md", "docs/Agents.md", "docs/claude.md",
                  "crates/app/src/.GitAttributes", "docs/.CLAUDE/x"]:
            self.assertEqual(case_shadowed([chg(p, "A")], Z), [p], p)

    def test_case_shadowed_leaves_ordinary_paths_alone(self):
        for p in ["docs/chat-design.md", "crates/app/src/chat/view/row.rs", "apps/flutter/lib/features/chat/a.dart",
                  "docs/Chat-Design.md"]:
            self.assertEqual(case_shadowed([chg(p, "A")], Z), [], p)

    def test_case_shadowed_skips_what_is_protected_anyway(self):
        self.assertEqual(case_shadowed([chg("docs/rules.md", "M")], Z), [])
        self.assertEqual(case_shadowed([chg("crates/engine/src/reply/x.rs", "M")], Z), [])

    def test_case_shadowed_checks_rename_origin(self):
        self.assertEqual(case_shadowed([chg("docs/ok.md", "R", old="docs/Decisions/a.md")], Z),
                         ["docs/Decisions/a.md"])

    def test_case_shadowed_new_test_file_ok(self):
        self.assertEqual(case_shadowed([chg("apps/flutter/test/chat/chattests.dart", "A")], Z), [])


if __name__ == "__main__":
    unittest.main()
