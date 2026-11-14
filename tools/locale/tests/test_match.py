from __future__ import annotations

import unittest

import check
from tests.helpers import TREE, small_tree

EN_SHARED = "language-name = English\n"
DE_SHARED = "language-name = Deutsch\n"


def match_problems(en_extra: str, de_extra: str, en_file="shared.ftl", de_file=None):
    de_file = de_file or en_file
    files = {"en/shared.ftl": EN_SHARED, "de/shared.ftl": DE_SHARED}
    files["en/" + en_file] = files.get("en/" + en_file, "") + en_extra
    files["de/" + de_file] = files.get("de/" + de_file, "") + de_extra
    work = small_tree(files, ship=("en",), draft=("de",))
    try:
        tree, loaded = check.load(work.locales)
        assert not loaded, [str(p) for p in loaded]
        assert not check.check_structure(tree), [str(p) for p in check.check_structure(tree)]
        return check.check_match(tree)
    finally:
        work.cleanup()


SEL = "state-x = { $count ->\n    [one] { $count } file\n   *[other] { $count } files\n}\n"


class MatchTests(unittest.TestCase):
    def one(self, problems, word):
        self.assertEqual(len(problems), 1, [str(p) for p in problems])
        self.assertEqual(problems[0].rule, "GUARD-LOCALE-MATCH")
        self.assertIn(word, problems[0].message)

    def test_fixture_tree_is_clean(self):
        tree, _ = check.load(TREE)
        self.assertEqual(check.check_match(tree), [])

    def test_unknown_key_is_refused_and_names_prune(self):
        found = match_problems("", "state-nope = X\n")
        self.one(found, "gen.py --prune")

    def test_same_variables_required(self):
        found = match_problems("state-x = Moved to { $folder_name }\n", "state-x = Verschoben\n")
        self.one(found, "$folder_name")
        found = match_problems("state-x = Moved to { $folder_name }\n", "state-x = V { $folder_name } { $other }\n")
        self.one(found, "$other")

    def test_select_on_string_variable_is_refused(self):
        found = match_problems(
            "state-x = Hello { $name }\n",
            "state-x = { $name ->\n    [one] Hallo { $name }\n   *[other] Hallo { $name }\n}\n",
        )
        self.one(found, "string variable")

    def test_select_on_number_english_does_not_select_is_allowed(self):
        de = (
            "state-x = { $count ->\n    [one] { $count } offen\n    [few] { $count } offene\n"
            "   *[other] { $count } offen\n}\n"
        )
        self.assertEqual(match_problems("state-x = { NUMBER($count) } open\n", de), [])

    def test_variable_missing_from_other_is_refused(self):
        de = "state-x = { $count ->\n    [one] { $count } Datei\n   *[other] Dateien\n}\n"
        self.one(match_problems(SEL, de), "$count")

    def test_other_variants_may_drop_a_number(self):
        de = "state-x = { $count ->\n    [one] eine Datei\n   *[other] { $count } Dateien\n}\n"
        self.assertEqual(match_problems(SEL, de), [])

    def test_no_links_or_markup(self):
        for text in ("https://x", "www.x.org", "a@b.co", "<b>x</b>"):
            with self.subTest(text=text):
                self.one(match_problems("state-x = X\n", f"state-x = {text}\n"), "carries")

    def test_english_with_links_is_not_checked(self):
        tree_files = {"en/shared.ftl": EN_SHARED + "state-x = https://x\n"}
        work = small_tree(tree_files)
        try:
            tree, _ = check.load(work.locales)
            self.assertEqual(check.check_match(tree), [])
        finally:
            work.cleanup()

    def test_length_cap(self):
        self.one(match_problems("state-x = abc\n", "state-x = " + "x" * 30 + "\n"), "29")
        self.assertEqual(match_problems("state-x = abc\n", "state-x = " + "x" * 29 + "\n"), [])

    def test_length_counts_variables_and_brand_terms(self):
        work = small_tree(
            {
                "en/shared.ftl": "-brand-notchling = Notchling\n" + EN_SHARED + "state-x = { -brand-notchling }\n",
                "de/shared.ftl": DE_SHARED + "state-x = " + "x" * 47 + "\n",
            },
            draft=("de",),
        )
        try:
            tree, _ = check.load(work.locales)
            self.assertEqual(check.check_match(tree), [])
        finally:
            work.cleanup()

    def test_missing_message_is_not_an_error(self):
        self.assertEqual(match_problems("state-x = X\nstate-y = Y\n", "state-x = X\n"), [])

    def test_key_in_the_wrong_file(self):
        work = small_tree(
            {
                "en/shared.ftl": EN_SHARED + "state-x = X\n",
                "de/shared.ftl": DE_SHARED,
                "de/mac/state.ftl": "state-x = Y\n",
            },
            draft=("de",),
        )
        try:
            tree, _ = check.load(work.locales)
            found = check.check_match(tree)
        finally:
            work.cleanup()
        self.one(found, "belongs in shared.ftl")

    def test_english_helpers(self):
        tree, _ = check.load(TREE)
        types = check.var_types(tree)
        self.assertEqual(types["session-subagents-running"], {"count": "num"})
        self.assertEqual(types["chat-moved-to"], {"folder_name": "str"})
        self.assertEqual(types["units-age-minutes"], {"n": "num"})
        self.assertEqual(types["item-number-sign"], {"count": "str"})
        self.assertEqual(check.english(tree)["chat-move"][0], "mac/chat.ftl")


if __name__ == "__main__":
    unittest.main()
