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

    # I3: hooks that rewrite how every existing test runs count as skips.
    def test_swift_test_run_hooks_trip(self):
        for line in ["    override func invokeTest() {}", "override class var defaultTestSuite: XCTestSuite { XCTestSuite() }",
                     "override class var testInvocations: [NSInvocation] { [] }"]:
            self.assertEqual(rules(chg("Tests/X/New.swift", "A", added=[line])), ["WEAK-SKIP"], line)

    def test_dart_golden_comparator_trips(self):
        self.assertEqual(rules(chg("mobile/test/flutter_test_config.dart", "A",
                                   added=["  goldenFileComparator = AlwaysPass();"])), ["WEAK-SKIP"])

    def test_skip_lookalikes_ok(self):
        self.assertEqual(rules(chg("Tests/X/New.swift", "A", added=["let invokeTestCount = 1", "var testInvocationsLog = 2"])), [])

    # I6: anything invisible or control-like in an added line goes to the maintainer.
    def test_invisible_characters_rejected(self):
        bad = {
            "U+061C arabic letter mark": "let a = 1 \u061c",
            "tag characters": "docs note \U000e0049\U000e0047\U000e004e",
            "tag begin": "x\U000e0001",
            "variation selector 16": "x\ufe0f",
            "variation selector 1": "x\ufe00",
            "variation selector text": "x\ufe0e",
            "variation selector supplement first": "x\U000e0100",
            "variation selector supplement last": "x\U000e01ef",
            "mongolian variation selector": "x\u180b",
            "hangul filler": "let \u3164 = 1",
            "halfwidth hangul filler": "let \uffa0 = 1",
            "hangul choseong filler": "let \u115f = 1",
            "hangul jungseong filler": "let \u1160 = 1",
            "khmer inherent vowel": "let a\u17b4 = 1",
            "soft hyphen": "ad\u00admin",
            "mongolian vowel separator": "a\u180eb",
            "line separator": "a\u2028b",
            "paragraph separator": "a\u2029b",
            "bare carriage return": "// note\rlet p = Process()",
            "trailing carriage return": "let a = 1\r",
            "nul": "a\x00b",
            "escape": "\x1b[31mred",
            "vertical tab": "a\x0bb",
            "form feed": "a\x0cb",
            "next line": "a\x85b",
            "delete": "a\x7fb",
            "right to left override": "a\u202eb",
            "zero width joiner": "a\u200db",
            "zero width non joiner": "a\u200cb",
            "word joiner": "a\u2060b",
            "bom": "\ufeffa",
            "interlinear annotation": "a\ufff9b",
        }
        for name, line in bad.items():
            for path in ("Sources/Notchling/Search/S.swift", "docs/a.md"):
                self.assertEqual(rules(chg(path, "M", added=[line])), ["WEAK-UNICODE"], f"{name} in {path}")

    def test_ordinary_text_still_ok(self):
        for line in ["\tlet a = 1", "Vi\u1ec7t Nam", "Vie\u0323\u0302t Nam (decomposed)", "caf\u00e9", "\u65e5\u672c\u8a9e",
                     "emoji \u2705 done", "no\u00a0break space", "arrow \u2192 here", "quote \u201cx\u201d"]:
            self.assertEqual(rules(chg("docs/a.md", "M", added=[line])), [], repr(line))

    def test_the_lines_own_newline_is_not_a_trip(self):
        self.assertEqual(rules(chg("docs/a.md", "M", added=["let a = 1\n"])), [])
        self.assertEqual(rules(chg("docs/a.md", "M", added=["let a = 1\nlet b = 2"])), ["WEAK-UNICODE"])

    def test_unicode_trip_shows_codepoints_not_the_characters(self):
        trips = scan([chg("docs/a.md", "M", added=["a\u061c\U000e0049\rb"])])
        self.assertEqual(len(trips), 1)
        self.assertEqual(trips[0].text, "a<U+061C><U+E0049><U+000D>b")


if __name__ == "__main__":
    unittest.main()
