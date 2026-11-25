import json
import unittest

from guard import lint, locale_lint
from tests.helpers import make_repo

MOVED = json.dumps({"areas": [{"name": "notch", "paths": ["crates/app/src/notch/**"], "moved": True}]})
NOT_MOVED = json.dumps({"areas": [{"name": "notch", "paths": ["crates/app/src/notch/**"], "moved": False}]})
NOTCH = "crates/app/src/notch/a.rs"
BAD_LINE = 'fn f() { div().child("Reply to Claude") }\n'


def found(files, path, rule="LINT-LOCALE-LITERAL"):
    repo = make_repo(files)
    return [v for v in lint.run(repo, [path]) if v.rule == rule]


class LiteralTests(unittest.TestCase):
    def test_literal_in_moved_area(self):
        self.assertEqual(len(found({NOTCH: BAD_LINE, locale_lint.AREAS: MOVED}, NOTCH)), 1)

    def test_area_not_moved_is_quiet(self):
        self.assertEqual(found({NOTCH: BAD_LINE, locale_lint.AREAS: NOT_MOVED}, NOTCH), [])

    def test_no_areas_file_is_quiet(self):
        self.assertEqual(found({NOTCH: BAD_LINE}, NOTCH), [])

    def test_a_message_is_quiet(self):
        text = "fn f() { div().child(t::notch::reply()) }\n"
        self.assertEqual(found({NOTCH: text, locale_lint.AREAS: MOVED}, NOTCH), [])

    def test_each_exemption(self):
        for token in locale_lint.EXEMPT:
            with self.subTest(token=token):
                text = 'fn f() { x("Reply to Claude") ' + token + " }\n"
                self.assertEqual(found({NOTCH: text, locale_lint.AREAS: MOVED}, NOTCH), [])

    def test_short_lowercase_strings_are_not_words(self):
        for text in ('let k = "id";\n', 'let k = "chat/view";\n', 'let k = "a-b_c";\n'):
            with self.subTest(text=text):
                self.assertEqual(found({NOTCH: text, locale_lint.AREAS: MOVED}, NOTCH), [])

    def test_capitalised_single_word_is_a_literal(self):
        self.assertEqual(len(found({NOTCH: 'let k = "Reply";\n', locale_lint.AREAS: MOVED}, NOTCH)), 1)

    def test_a_comment_is_not_a_literal(self):
        text = '// "Reply to Claude"\nlet k = 1;\n'
        self.assertEqual(found({NOTCH: text, locale_lint.AREAS: MOVED}, NOTCH), [])

    def test_rust_test_module_is_skipped(self):
        text = "fn f() {}\n#[cfg(test)]\nmod tests {\n    fn t() { x(\"Reply to Claude\"); }\n}\n"
        self.assertEqual(found({NOTCH: text, locale_lint.AREAS: MOVED}, NOTCH), [])

    def test_test_files_and_renders_are_skipped(self):
        for path in ("crates/app/src/notch/a_tests.rs", "crates/app/src/notch/tests_view.rs",
                     "crates/app/src/notch/renders.rs", "crates/app/src/notch/tests/x.rs"):
            with self.subTest(path=path):
                self.assertEqual(found({path: BAD_LINE, locale_lint.AREAS: MOVED}, path), [])

    def test_dart_literal(self):
        areas = json.dumps({"areas": [{"name": "phone", "paths": ["apps/flutter/lib/**"], "moved": True}]})
        path = "apps/flutter/lib/features/a.dart"
        self.assertEqual(len(found({path: "final a = Text('Reply to Claude');\n", locale_lint.AREAS: areas}, path)), 1)
        self.assertEqual(len(found({path: 'final a = Text("Reply to Claude");\n', locale_lint.AREAS: areas}, path)), 1)
        gen = "apps/flutter/lib/l10n/app_localizations_en.dart"
        self.assertEqual(found({gen: "final a = 'Reply to Claude';\n", locale_lint.AREAS: areas}, gen), [])


class ConsentTests(unittest.TestCase):
    def test_consent_warning(self):
        path = "locales/en/mac/chat.ftl"
        hits = found({path: "chat-allow = Allow\n"}, path, "LINT-LOCALE-CONSENT")
        self.assertEqual(len(hits), 1)
        self.assertTrue(hits[0].warn)

    def test_each_start_is_a_warning(self):
        path = "locales/en/mac/chat.ftl"
        for start in locale_lint.CONSENT_START:
            with self.subTest(start=start):
                self.assertEqual(len(found({path: f"k = {start} the thing\n"}, path, "LINT-LOCALE-CONSENT")), 1)

    def test_other_words_are_quiet(self):
        path = "locales/en/mac/chat.ftl"
        self.assertEqual(found({path: "k = Allowed here\nj = Reply\n"}, path, "LINT-LOCALE-CONSENT"), [])

    def test_consent_file_is_quiet(self):
        path = "locales/en/consent.ftl"
        self.assertEqual(found({path: "k = Allow\n"}, path, "LINT-LOCALE-CONSENT"), [])


class GeneratedSizeTests(unittest.TestCase):
    def test_generated_locale_files_have_no_size_limit(self):
        for path in ("crates/locale/src/generated.rs", "apps/flutter/lib/l10n/app_localizations.dart"):
            with self.subTest(path=path):
                repo = make_repo({path: "x\n" * 5000})
                self.assertEqual(lint.run(repo, [path]), [])


if __name__ == "__main__":
    unittest.main()
