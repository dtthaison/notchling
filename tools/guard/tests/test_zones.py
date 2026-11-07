import unittest
from pathlib import Path

from guard.changes import Change
from guard.zones import load, protected_paths

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


if __name__ == "__main__":
    unittest.main()
