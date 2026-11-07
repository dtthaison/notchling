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
        self.assertTrue(Z.in_zone("Sources/Notchling/Chat/Views/ChatRow.swift"))

    def test_model_protected(self):
        self.assertFalse(Z.in_zone("Sources/Notchling/Chat/Model/ChatModel.swift"))

    def test_notch_controller_protected(self):
        self.assertFalse(Z.in_zone("Sources/Notchling/Notch/NotchController.swift"))

    def test_notch_views_in_zone(self):
        for name in ["NotchView", "NotchRows", "NotchPeek", "NotchDetail", "NotchCardParts", "TabPerch"]:
            self.assertTrue(Z.in_zone(f"Sources/Notchling/Notch/{name}.swift"), name)
        self.assertTrue(Z.in_zone("Sources/Notchling/Notch/Pet/Walk.swift"))

    def test_transcript_readers_in_zone(self):
        for p in ["Sources/Notchling/Chat/ConversationReader.swift", "Sources/Notchling/Agents/TranscriptReader.swift",
                  "Sources/ConsoleCore/ChatTranscript.swift"]:
            self.assertTrue(Z.in_zone(p), p)

    def test_reply_sender_protected(self):
        self.assertFalse(Z.in_zone("Sources/Notchling/Agents/ReplySender.swift"))

    def test_docs_in_zone_but_manual_protected(self):
        self.assertTrue(Z.in_zone("docs/chat-design.md"))
        for p in ["docs/rules.md", "docs/goals.md", "docs/decisions/0001-gpui-spike.md"]:
            self.assertFalse(Z.in_zone(p), p)

    def test_phone_features_in_zone_pairing_not(self):
        self.assertTrue(Z.in_zone("mobile/lib/features/chat/chat_page.dart"))
        self.assertFalse(Z.in_zone("mobile/lib/features/pairing/pair.dart"))
        self.assertFalse(Z.in_zone("mobile/lib/core/pairing/keys.dart"))

    def test_guard_and_github_protected(self):
        for p in ["tools/guard/zones.json", ".github/workflows/guard.yml", ".github/CODEOWNERS", "build.sh",
                  "CLAUDE.md", "AGENTS.md", "Sources/Notchling/AGENTS.md", ".claude/settings.json"]:
            self.assertFalse(Z.in_zone(p), p)

    def test_star_does_not_cross_folders(self):
        self.assertFalse(Z.in_zone("Sources/Notchling/Notch/Pet2/x.swift"))

    def test_new_test_ok_edit_not(self):
        self.assertEqual(protected_paths([chg("Tests/NotchlingTests/NewTests.swift", "A")], Z), [])
        self.assertEqual(protected_paths([chg("Tests/NotchlingTests/ChatTests.swift", "M")], Z),
                         ["Tests/NotchlingTests/ChatTests.swift"])
        self.assertEqual(protected_paths([chg("Tests/NotchlingTests/ChatTests.swift", "D")], Z),
                         ["Tests/NotchlingTests/ChatTests.swift"])

    def test_rename_out_of_protected_is_protected(self):
        self.assertEqual(protected_paths([chg("Sources/Notchling/Search/X.swift", "R",
                                              old="Sources/Notchling/Vault/X.swift")], Z),
                         ["Sources/Notchling/Vault/X.swift"])

    def test_rename_into_protected_is_protected(self):
        self.assertEqual(protected_paths([chg("Sources/Notchling/Vault/X.swift", "R",
                                              old="Sources/Notchling/Search/X.swift")], Z),
                         ["Sources/Notchling/Vault/X.swift"])

    def test_deleting_zone_file_ok(self):
        self.assertEqual(protected_paths([chg("Sources/Notchling/Search/Old.swift", "D")], Z), [])

    # I4: the spec's protected files stay protected inside a zone folder.
    def test_spec_protected_files_in_zone_folders_are_protected(self):
        for p in ["docs/AGENTS.md", "docs/CLAUDE.md", "Sources/Notchling/Search/CLAUDE.md",
                  "Sources/Notchling/Search/AGENTS.md", "Sources/Notchling/Search/.claude/skills/x/SKILL.md",
                  "docs/.claude/settings.json", "Sources/Notchling/Search/.gitattributes",
                  "mobile/lib/features/chat/.gitattributes", "docs/.gitattributes", "docs/CODEOWNERS",
                  "Sources/Notchling/Notch/Pet/CODEOWNERS"]:
            self.assertFalse(Z.in_zone(p), p)

    def test_lookalike_names_stay_in_zone(self):
        for p in ["docs/NOTAGENTS.md", "docs/agents-notes.md", "docs/claude/x.md", "docs/gitattributes.md"]:
            self.assertTrue(Z.in_zone(p), p)

    def test_leading_double_star_matches_at_the_root_too(self):
        rx = glob_regex("**/AGENTS.md")
        self.assertTrue(rx.match("AGENTS.md"))
        self.assertTrue(rx.match("Sources/x/AGENTS.md"))
        self.assertFalse(rx.match("XAGENTS.md"))
        self.assertFalse(rx.match("docs/NOTAGENTS.md"))
        rx = glob_regex("**/.claude/**")
        self.assertTrue(rx.match(".claude/settings.json"))
        self.assertTrue(rx.match("a/b/.claude/skills/x"))
        self.assertFalse(rx.match("a/claude/x"))

    def test_middle_double_star_matches_zero_or_more_folders(self):
        rx = glob_regex("mobile/**/dart_test.yaml")
        self.assertTrue(rx.match("mobile/dart_test.yaml"))
        self.assertTrue(rx.match("mobile/test/a/dart_test.yaml"))

    def test_root_exclusion_applies(self):
        z = Zones(("**", "!**/AGENTS.md"), ())
        self.assertFalse(z.in_zone("AGENTS.md"))
        self.assertFalse(z.in_zone("a/b/AGENTS.md"))
        self.assertTrue(z.in_zone("a/b.md"))

    def test_new_file_cannot_be_a_spec_protected_file(self):
        for p in ["Tests/AGENTS.md", "Fixtures/CLAUDE.md", "mobile/test/.claude/x.md",
                  "Tests/NotchlingTests/.gitattributes", "Fixtures/.gitattributes", "mobile/test/CODEOWNERS"]:
            self.assertEqual(protected_paths([chg(p, "A")], Z), [p], p)

    # I3: a new test-config file changes how every existing phone test runs.
    def test_new_flutter_test_config_and_dart_test_yaml_are_protected(self):
        for p in ["mobile/test/flutter_test_config.dart", "mobile/test/chat/flutter_test_config.dart",
                  "mobile/test/dart_test.yaml", "mobile/test/goldens/dart_test.yaml"]:
            self.assertEqual(protected_paths([chg(p, "A")], Z), [p], p)

    def test_new_phone_test_still_ok(self):
        self.assertEqual(protected_paths([chg("mobile/test/chat/new_test.dart", "A")], Z), [])
        self.assertEqual(protected_paths([chg("mobile/test/goldens/new.png", "A")], Z), [])

    # I2: on a Mac a path that differs only by case is the same path.
    def test_case_shadowed_zone_folders(self):
        for p in ["docs/Decisions/x.md", "docs/DECISIONS/x.md", "mobile/lib/features/Pairing/pair.dart",
                  "docs/Rules.md", "docs/Goals.md", "docs/Agents.md", "docs/claude.md",
                  "Sources/Notchling/Search/.GitAttributes", "docs/.CLAUDE/x"]:
            self.assertEqual(case_shadowed([chg(p, "A")], Z), [p], p)

    def test_case_shadowed_leaves_ordinary_paths_alone(self):
        for p in ["docs/chat-design.md", "Sources/Notchling/Chat/Views/ChatRow.swift", "mobile/lib/features/chat/a.dart",
                  "docs/Chat-Design.md"]:
            self.assertEqual(case_shadowed([chg(p, "A")], Z), [], p)

    def test_case_shadowed_skips_what_is_protected_anyway(self):
        self.assertEqual(case_shadowed([chg("docs/rules.md", "M")], Z), [])
        self.assertEqual(case_shadowed([chg("Sources/Notchling/Vault/X.swift", "M")], Z), [])

    def test_case_shadowed_checks_rename_origin(self):
        self.assertEqual(case_shadowed([chg("docs/ok.md", "R", old="docs/Decisions/a.md")], Z),
                         ["docs/Decisions/a.md"])

    def test_case_shadowed_new_test_file_ok(self):
        self.assertEqual(case_shadowed([chg("Tests/NotchlingTests/chattests.swift", "A")], Z), [])


if __name__ == "__main__":
    unittest.main()
