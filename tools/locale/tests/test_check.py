from __future__ import annotations

import unittest

import check
from tests.helpers import TREE, Workdir, small_tree

BASE = "language-name = English\n"


def problems_of(files, ship=("en",), draft=(), outputs=("rust", "arb"), base=True):
    files = dict(files)
    if base and "en/shared.ftl" not in files:
        files["en/shared.ftl"] = BASE
    work = small_tree(files, ship=ship, draft=draft, outputs=outputs)
    try:
        tree, loaded = check.load(work.locales)
        return loaded + check.check_structure(tree)
    finally:
        work.cleanup()


class StructureTests(unittest.TestCase):
    def one(self, problems, path, line, word):
        self.assertEqual(len(problems), 1, [str(p) for p in problems])
        problem = problems[0]
        self.assertEqual(problem.rule, "GUARD-LOCALE-SUBSET")
        self.assertEqual(problem.path, path)
        self.assertEqual(problem.line, line)
        self.assertIn(word, problem.message)

    def test_fixture_tree_is_clean(self):
        tree, problems = check.load(TREE)
        self.assertEqual(problems, [])
        self.assertEqual(check.check_structure(tree), [])

    def test_key_must_start_with_file_name(self):
        found = problems_of({"en/mac/notch.ftl": "\nchat-x = X\n"})
        self.one(found, "locales/en/mac/notch.ftl", 2, "notch-")

    def test_shared_namespaces_only(self):
        found = problems_of({"en/shared.ftl": BASE + "chat-x = X\n"})
        self.one(found, "locales/en/shared.ftl", 2, "shared.ftl keys start with")
        found = problems_of({"en/mac/chat.ftl": "state-x = X\n"})
        self.one(found, "locales/en/mac/chat.ftl", 1, "chat-")

    def test_area_file_cannot_take_a_shared_namespace(self):
        found = problems_of({"en/mac/state.ftl": "state-x = X\n"})
        self.one(found, "locales/en/mac/state.ftl", 1, "reserved")

    def test_key_shape(self):
        keys = ["Chat-x", "chat_x", "chat-1x", "chat", "chat-" + "a" * 60]
        for key in keys:
            with self.subTest(key=key):
                found = problems_of({"en/mac/chat.ftl": f"{key} = X\n"})
                self.one(found, "locales/en/mac/chat.ftl", 1, "is not lowercase words")

    def test_duplicate_key_across_files(self):
        found = problems_of(
            {
                "en/shared.ftl": BASE + "state-x = A\n",
                "en/mac/state.ftl": "state-x = B\n",
            }
        )
        keys = [(p.path, p.line) for p in found if "also defined" in p.message]
        self.assertEqual(sorted(keys), [("locales/en/mac/state.ftl", 1), ("locales/en/shared.ftl", 2)])

    def test_unlisted_language_folder(self):
        found = problems_of({"fr/shared.ftl": "language-name = Français\n"})
        self.one(found, "locales/fr", 1, "languages.json")

    def test_tag_form(self):
        for tag in ("pt-br", "zh-hans", "Fr"):
            with self.subTest(tag=tag):
                found = problems_of({f"{tag}/shared.ftl": "language-name = X\n"}, draft=(tag,))
                self.one(found, f"locales/{tag}", 1, "language tag")

    def test_braces_only_in_mac(self):
        text = 'x-y = a{"{"}b\n'
        self.assertEqual(problems_of({"en/mac/chat.ftl": text.replace("x-y", "chat-y")}), [])
        cases = {
            "en/shared.ftl": BASE + text.replace("x-y", "state-y"),
            "en/consent.ftl": text.replace("x-y", "consent-y"),
            "en/phone/pairing.ftl": text.replace("x-y", "pairing-y"),
        }
        for rel, content in cases.items():
            with self.subTest(rel=rel):
                found = problems_of({rel: content})
                self.assertEqual(len(found), 1, [str(p) for p in found])
                self.assertIn("only in mac/", found[0].message)

    def test_brand_terms_only_in_english_shared(self):
        found = problems_of(
            {"en/mac/chat.ftl": "-brand-x = X\n"},
        )
        self.one(found, "locales/en/mac/chat.ftl", 1, "English shared.ftl")
        found = problems_of(
            {"en/shared.ftl": BASE, "de/shared.ftl": "language-name = Deutsch\n-brand-x = X\n"}, draft=("de",)
        )
        self.one(found, "locales/de/shared.ftl", 2, "English shared.ftl")
        found = problems_of({"en/shared.ftl": BASE + "state-x = A { -brand-y }\n"})
        self.one(found, "locales/en/shared.ftl", 2, "unknown brand term")
        found = problems_of({"en/shared.ftl": "-brand-y = Y\n" + BASE + "state-x = A { -brand-y }\n"})
        self.assertEqual(found, [])

    def test_language_name_required(self):
        found = problems_of({"en/shared.ftl": "state-x = X\n"})
        self.one(found, "locales/en/shared.ftl", 1, "language-name")
        found = problems_of(
            {"de/mac/chat.ftl": "chat-x = X\n"}, draft=("de",)
        )
        self.assertTrue(any("language-name" in p.message and p.path == "locales/de/shared.ftl" for p in found))

    def test_unknown_output(self):
        found = problems_of({}, outputs=("rust", "ios"))
        self.one(found, "locales/languages.json", 1, "ios")

    def test_unknown_file(self):
        found = problems_of({"en/other.ftl": "other-x = X\n"})
        self.one(found, "locales/en/other.ftl", 1, "unknown file")

    def test_parse_errors_are_problems_and_loading_continues(self):
        work = small_tree({"en/shared.ftl": BASE, "en/mac/chat.ftl": "chat-x = {\n", "en/mac/units.ftl": "units-x = X \n"})
        try:
            tree, loaded = check.load(work.locales)
        finally:
            work.cleanup()
        self.assertEqual(sorted(p.path for p in loaded), ["locales/en/mac/chat.ftl", "locales/en/mac/units.ftl"])

    def test_targets(self):
        self.assertEqual(check.targets("shared.ftl"), {"rust", "phone"})
        self.assertEqual(check.targets("consent.ftl"), {"rust", "phone"})
        self.assertEqual(check.targets("mac/chat.ftl"), {"rust"})
        self.assertEqual(check.targets("phone/pairing.ftl"), {"phone"})


if __name__ == "__main__":
    unittest.main()
