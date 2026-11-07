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
        self.assertTrue(Z.in_zone("legacy/macos/Sources/Notchling/Chat/Views/ChatRow.swift"))

    def test_model_protected(self):
        self.assertFalse(Z.in_zone("legacy/macos/Sources/Notchling/Chat/Model/ChatModel.swift"))

    def test_notch_controller_protected(self):
        self.assertFalse(Z.in_zone("legacy/macos/Sources/Notchling/Notch/NotchController.swift"))

    def test_notch_views_in_zone(self):
        for name in ["NotchView", "NotchRows", "NotchPeek", "NotchDetail", "NotchCardParts", "TabPerch"]:
            self.assertTrue(Z.in_zone(f"legacy/macos/Sources/Notchling/Notch/{name}.swift"), name)
        self.assertTrue(Z.in_zone("legacy/macos/Sources/Notchling/Notch/Pet/Walk.swift"))

    def test_transcript_readers_in_zone(self):
        for p in ["legacy/macos/Sources/Notchling/Chat/ConversationReader.swift", "legacy/macos/Sources/Notchling/Agents/TranscriptReader.swift",
                  "legacy/macos/Sources/ConsoleCore/ChatTranscript.swift"]:
            self.assertTrue(Z.in_zone(p), p)

    def test_reply_sender_protected(self):
        self.assertFalse(Z.in_zone("legacy/macos/Sources/Notchling/Agents/ReplySender.swift"))

    def test_docs_in_zone_but_manual_protected(self):
        self.assertTrue(Z.in_zone("docs/chat-design.md"))
        for p in ["docs/rules.md", "docs/goals.md", "docs/decisions/0001-gpui-spike.md"]:
            self.assertFalse(Z.in_zone(p), p)

    def test_phone_features_in_zone_pairing_not(self):
        self.assertTrue(Z.in_zone("apps/flutter/lib/features/chat/chat_page.dart"))
        self.assertFalse(Z.in_zone("apps/flutter/lib/features/pairing/pair.dart"))
        self.assertFalse(Z.in_zone("apps/flutter/lib/core/pairing/keys.dart"))

    def test_crate_sources_in_zone_but_manifests_and_build_scripts_not(self):
        # A Cargo.toml adds dependencies and a build.rs runs code at build time: both wait for the maintainer.
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
        for p in ["tools/guard/zones.json", ".github/workflows/guard.yml", ".github/CODEOWNERS", "build.sh",
                  "CLAUDE.md", "AGENTS.md", "legacy/macos/Sources/Notchling/AGENTS.md", ".claude/settings.json"]:
            self.assertFalse(Z.in_zone(p), p)

    def test_star_does_not_cross_folders(self):
        self.assertFalse(Z.in_zone("legacy/macos/Sources/Notchling/Notch/Pet2/x.swift"))

    def test_new_test_ok_edit_not(self):
        self.assertEqual(protected_paths([chg("legacy/macos/Tests/NotchlingTests/NewTests.swift", "A")], Z), [])
        self.assertEqual(protected_paths([chg("legacy/macos/Tests/NotchlingTests/ChatTests.swift", "M")], Z),
                         ["legacy/macos/Tests/NotchlingTests/ChatTests.swift"])
        self.assertEqual(protected_paths([chg("legacy/macos/Tests/NotchlingTests/ChatTests.swift", "D")], Z),
                         ["legacy/macos/Tests/NotchlingTests/ChatTests.swift"])

    def test_rename_out_of_protected_is_protected(self):
        self.assertEqual(protected_paths([chg("legacy/macos/Sources/Notchling/Search/X.swift", "R",
                                              old="legacy/macos/Sources/Notchling/Vault/X.swift")], Z),
                         ["legacy/macos/Sources/Notchling/Vault/X.swift"])

    def test_rename_into_protected_is_protected(self):
        self.assertEqual(protected_paths([chg("legacy/macos/Sources/Notchling/Vault/X.swift", "R",
                                              old="legacy/macos/Sources/Notchling/Search/X.swift")], Z),
                         ["legacy/macos/Sources/Notchling/Vault/X.swift"])

    def test_deleting_zone_file_ok(self):
        self.assertEqual(protected_paths([chg("legacy/macos/Sources/Notchling/Search/Old.swift", "D")], Z), [])

    # I4: the spec's protected files stay protected inside a zone folder.
    def test_spec_protected_files_in_zone_folders_are_protected(self):
        for p in ["docs/AGENTS.md", "docs/CLAUDE.md", "legacy/macos/Sources/Notchling/Search/CLAUDE.md",
                  "legacy/macos/Sources/Notchling/Search/AGENTS.md", "legacy/macos/Sources/Notchling/Search/.claude/skills/x/SKILL.md",
                  "docs/.claude/settings.json", "legacy/macos/Sources/Notchling/Search/.gitattributes",
                  "apps/flutter/lib/features/chat/.gitattributes", "docs/.gitattributes", "docs/CODEOWNERS",
                  "legacy/macos/Sources/Notchling/Notch/Pet/CODEOWNERS"]:
            self.assertFalse(Z.in_zone(p), p)

    def test_lookalike_names_stay_in_zone(self):
        for p in ["docs/NOTAGENTS.md", "docs/agents-notes.md", "docs/claude/x.md", "docs/gitattributes.md"]:
            self.assertTrue(Z.in_zone(p), p)

    def test_leading_double_star_matches_at_the_root_too(self):
        rx = glob_regex("**/AGENTS.md")
        self.assertTrue(rx.match("AGENTS.md"))
        self.assertTrue(rx.match("legacy/macos/Sources/x/AGENTS.md"))
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
        for p in ["legacy/macos/Tests/AGENTS.md", "legacy/macos/Fixtures/CLAUDE.md", "apps/flutter/test/.claude/x.md",
                  "legacy/macos/Tests/NotchlingTests/.gitattributes", "legacy/macos/Fixtures/.gitattributes", "apps/flutter/test/CODEOWNERS"]:
            self.assertEqual(protected_paths([chg(p, "A")], Z), [p], p)

    # I3: a new test-config file changes how every existing phone test runs.
    def test_new_flutter_test_config_and_dart_test_yaml_are_protected(self):
        for p in ["apps/flutter/test/flutter_test_config.dart", "apps/flutter/test/chat/flutter_test_config.dart",
                  "apps/flutter/test/dart_test.yaml", "apps/flutter/test/goldens/dart_test.yaml"]:
            self.assertEqual(protected_paths([chg(p, "A")], Z), [p], p)

    def test_new_phone_test_still_ok(self):
        self.assertEqual(protected_paths([chg("apps/flutter/test/chat/new_test.dart", "A")], Z), [])
        self.assertEqual(protected_paths([chg("apps/flutter/test/goldens/new.png", "A")], Z), [])

    # I2: on a Mac a path that differs only by case is the same path.
    def test_case_shadowed_zone_folders(self):
        for p in ["docs/Decisions/x.md", "docs/DECISIONS/x.md", "apps/flutter/lib/features/Pairing/pair.dart",
                  "docs/Rules.md", "docs/Goals.md", "docs/Agents.md", "docs/claude.md",
                  "legacy/macos/Sources/Notchling/Search/.GitAttributes", "docs/.CLAUDE/x"]:
            self.assertEqual(case_shadowed([chg(p, "A")], Z), [p], p)

    def test_case_shadowed_leaves_ordinary_paths_alone(self):
        for p in ["docs/chat-design.md", "legacy/macos/Sources/Notchling/Chat/Views/ChatRow.swift", "apps/flutter/lib/features/chat/a.dart",
                  "docs/Chat-Design.md"]:
            self.assertEqual(case_shadowed([chg(p, "A")], Z), [], p)

    def test_case_shadowed_skips_what_is_protected_anyway(self):
        self.assertEqual(case_shadowed([chg("docs/rules.md", "M")], Z), [])
        self.assertEqual(case_shadowed([chg("legacy/macos/Sources/Notchling/Vault/X.swift", "M")], Z), [])

    def test_case_shadowed_checks_rename_origin(self):
        self.assertEqual(case_shadowed([chg("docs/ok.md", "R", old="docs/Decisions/a.md")], Z),
                         ["docs/Decisions/a.md"])

    def test_case_shadowed_new_test_file_ok(self):
        self.assertEqual(case_shadowed([chg("legacy/macos/Tests/NotchlingTests/chattests.swift", "A")], Z), [])


if __name__ == "__main__":
    unittest.main()
