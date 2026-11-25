from __future__ import annotations

import json
import unittest

import check
import emit_android
import emit_xcstrings
from tests.golden_util import assert_golden
from tests.helpers import TREE


class NativeTests(unittest.TestCase):
    def setUp(self):
        self.tree, problems = check.load(TREE)
        self.assertEqual(problems, [])

    def test_xcstrings_golden(self):
        assert_golden(self, "Localizable.xcstrings", emit_xcstrings.render(self.tree))

    def test_android_golden(self):
        files = emit_android.render(self.tree)
        self.assertEqual(sorted(files), ["values-pt-rBR/strings.xml", "values-pt/strings.xml", "values/strings.xml"])
        for rel, text in files.items():
            assert_golden(self, f"android/{rel}", text)

    def test_qualifiers(self):
        self.assertEqual(emit_android.qualifier("de"), "values-de")
        self.assertEqual(emit_android.qualifier("pt-BR"), "values-pt-rBR")
        self.assertEqual(emit_android.qualifier("zh-Hans"), "values-b+zh+Hans")
        self.assertEqual(emit_android.qualifier("zh-Hans-CN"), "values-b+zh+Hans+CN")

    def test_android_apostrophe_and_percent(self):
        text = emit_android.render(self.tree)["values/strings.xml"]
        self.assertIn("Can\\'t find your Mac", text)
        self.assertIn("100%% done", text)
        self.assertIn('<item quantity="one">%2$s shared %1$d file, 100%% done</item>', text)

    def test_android_plurals_and_notes(self):
        text = emit_android.render(self.tree)["values/strings.xml"]
        self.assertIn('<plurals name="session_subagents_running">', text)
        self.assertIn('<item quantity="other">%1$d subagents running</item>', text)
        self.assertIn("<!-- Title of the Subagents section while some still run. $count is how many run. -->", text)
        self.assertNotIn("chat_move", text)

    def test_android_only_messages_a_language_has(self):
        text = emit_android.render(self.tree)["values-pt-rBR/strings.xml"]
        self.assertIn('name="language_name"', text)
        self.assertNotIn("pairing_title", text)

    def test_xcstrings_single_variable_plural_uses_variations(self):
        doc = json.loads(emit_xcstrings.render(self.tree))
        entry = doc["strings"]["session-subagents-running"]["localizations"]["en"]
        self.assertEqual(entry["variations"]["plural"]["one"]["stringUnit"]["value"], "%lld subagent running")
        self.assertEqual(doc["sourceLanguage"], "en")
        self.assertEqual(doc["strings"]["session-subagents-running"]["extractionState"], "manual")

    def test_xcstrings_plural_with_two_variables_uses_substitution(self):
        doc = json.loads(emit_xcstrings.render(self.tree))
        entry = doc["strings"]["pairing-files"]["localizations"]["en"]
        self.assertEqual(entry["stringUnit"]["value"], "%#@count@")
        sub = entry["substitutions"]["count"]
        self.assertEqual(sub["argNum"], 1)
        self.assertEqual(sub["formatSpecifier"], "lld")
        self.assertEqual(
            sub["variations"]["plural"]["one"]["stringUnit"]["value"], "%2$@ shared %arg file, 100%% done"
        )

    def test_xcstrings_positional_for_plain_messages_and_languages(self):
        doc = json.loads(emit_xcstrings.render(self.tree))
        self.assertEqual(doc["strings"]["consent-secret-approval"]["localizations"]["en"]["stringUnit"]["value"], "Allow %1$@ for %2$@")
        self.assertEqual(
            sorted(doc["strings"]["state-working"]["localizations"]),
            ["en", "pt"],
        )
        self.assertNotIn("chat-move", doc["strings"])


if __name__ == "__main__":
    unittest.main()
