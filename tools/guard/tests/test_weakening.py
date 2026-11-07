import unittest

from guard.changes import Change
from guard.weakening import scan


def chg(path, status="M", old=None, added=(), binary=False):
    return Change(path, status, old, tuple((i + 1, l) for i, l in enumerate(added)), (), binary)


def rules(*changes):
    return sorted({t.rule for t in scan(list(changes))})


class WeakeningTests(unittest.TestCase):
    def test_edited_test_trips(self):
        self.assertEqual(rules(chg("Tests/NotchlingTests/ChatTests.swift", "M")), ["WEAK-TEST-EDIT"])

    def test_deleted_test_trips(self):
        self.assertEqual(rules(chg("Tests/NotchlingTests/ChatTests.swift", "D")), ["WEAK-TEST-EDIT"])

    def test_new_test_ok(self):
        self.assertEqual(rules(chg("Tests/NotchlingTests/New.swift", "A", added=["func testX() {}"])), [])

    def test_renamed_test_is_weakening(self):
        self.assertEqual(rules(chg("Tests/A/B.swift", "R", old="Tests/A/A.swift")), ["WEAK-TEST-EDIT"])

    def test_fixture_edit_trips(self):
        self.assertEqual(rules(chg("Fixtures/acp-agy.jsonl", "M")), ["WEAK-TEST-EDIT"])

    def test_guard_tests_edit_trips(self):
        self.assertEqual(rules(chg("tools/guard/tests/test_zones.py", "M")), ["WEAK-TEST-EDIT"])

    def test_rust_tests_folder_edit_trips(self):
        self.assertEqual(rules(chg("crates/engine/tests/parse.rs", "M")), ["WEAK-TEST-EDIT"])

    def test_golden_edit_trips(self):
        self.assertEqual(rules(chg("mobile/test/goldens/kit.png", "M", binary=True)), ["WEAK-TEST-EDIT"])

    def test_golden_outside_test_folder_trips(self):
        self.assertEqual(rules(chg("crates/app/goldens/notch.png", "M", binary=True)), ["WEAK-TEST-EDIT"])

    def test_xctskip_trips(self):
        self.assertEqual(rules(chg("Tests/X/New.swift", "A", added=['throw XCTSkip("later")'])), ["WEAK-SKIP"])

    def test_rust_ignore_trips(self):
        self.assertEqual(rules(chg("crates/engine/src/a.rs", "M", added=["#[ignore]"])), ["WEAK-SKIP"])

    def test_dart_skip_trips(self):
        self.assertEqual(rules(chg("mobile/test/new_test.dart", "A", added=["test('x', () {}, skip: true);"])),
                         ["WEAK-SKIP"])

    def test_bidi_rejected(self):
        self.assertEqual(rules(chg("Sources/Notchling/Search/S.swift", "M", added=["let a = 1 \u202e"])),
                         ["WEAK-UNICODE"])

    def test_zero_width_rejected(self):
        self.assertEqual(rules(chg("docs/a.md", "M", added=["sa\u200bfe"])), ["WEAK-UNICODE"])

    def test_plain_text_ok(self):
        self.assertEqual(rules(chg("README.md", "M", added=["Việt Nam, đây là tiếng Việt"])), [])

    def test_source_edit_ok(self):
        self.assertEqual(rules(chg("Sources/Notchling/Search/S.swift", "M", added=["let a = 1"])), [])


if __name__ == "__main__":
    unittest.main()
