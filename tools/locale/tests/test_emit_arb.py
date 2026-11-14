from __future__ import annotations

import json
import unittest

import check
import emit_arb
from tests.golden_util import assert_golden
from tests.helpers import TREE, small_tree

BASE = "language-name = English\n"


def render_files(files, tag="en"):
    work = small_tree(files)
    try:
        tree, loaded = check.load(work.locales)
        assert not loaded, [str(p) for p in loaded]
        return emit_arb.render(tree, tag)
    finally:
        work.cleanup()


class EmitArbTests(unittest.TestCase):
    def test_golden_en(self):
        tree, _ = check.load(TREE)
        assert_golden(self, "app_en.arb", emit_arb.render(tree, "en"))

    def test_golden_pt_br_filled_from_pt_then_english(self):
        tree, _ = check.load(TREE)
        text = emit_arb.render(tree, "pt-BR")
        assert_golden(self, "app_pt_BR.arb", text)
        data = json.loads(text)
        self.assertEqual(data["@@locale"], "pt_BR")
        self.assertEqual(data["languageName"], "Português (Brasil)")
        self.assertEqual(data["stateWorking"], "A trabalhar")
        self.assertEqual(data["pairingTitle"], "Pair with your Mac")

    def test_locale_comes_first_and_keys_are_sorted(self):
        tree, _ = check.load(TREE)
        keys = list(json.loads(emit_arb.render(tree, "en")))
        self.assertEqual(keys[0], "@@locale")
        names = [k for k in keys if not k.startswith("@")]
        self.assertEqual(names, sorted(names))

    def test_mac_messages_are_left_out(self):
        tree, _ = check.load(TREE)
        data = json.loads(emit_arb.render(tree, "en"))
        self.assertNotIn("chatMove", data)
        self.assertNotIn("unitsListSeparator", data)
        self.assertIn("consentAllowOnce", data)
        self.assertIn("pairingTitle", data)

    def test_plural_and_placeholders(self):
        tree, _ = check.load(TREE)
        data = json.loads(emit_arb.render(tree, "en"))
        self.assertEqual(
            data["sessionSubagentsRunning"],
            "{count, plural, one{{count} subagent running} other{{count} subagents running}}",
        )
        self.assertEqual(
            data["@sessionSubagentsRunning"]["placeholders"],
            {"count": {"type": "int", "format": "decimalPattern"}},
        )
        self.assertEqual(
            data["@sessionSubagentsRunning"]["description"],
            "Title of the Subagents section while some still run. $count is how many run.",
        )
        self.assertEqual(data["@consentSecretApproval"]["placeholders"], {"names": {"type": "String"}, "command": {"type": "String"}})

    def test_apostrophe_and_hash_pass_through(self):
        tree, _ = check.load(TREE)
        data = json.loads(emit_arb.render(tree, "en"))
        self.assertEqual(data["pairingCantFind"], "Can't find your Mac")
        self.assertEqual(data["pairingStep"], "Step # of {total}")
        self.assertEqual(data["itemNumberSign"], "# of items: {count}")

    def test_brand_inlined(self):
        tree, _ = check.load(TREE)
        data = json.loads(emit_arb.render(tree, "en"))
        self.assertEqual(data["stateNotRunning"], "Notchling isn't running")

    def test_hidden_character_escaped(self):
        text = render_files({"en/shared.ftl": BASE + 'state-x = a{"\\u200c"}b\n'})
        self.assertIn("\\u200c", text)
        self.assertNotIn("\u200c", text)
        self.assertEqual(json.loads(text)["stateX"], "a\u200cb")

    def test_names(self):
        self.assertEqual(emit_arb.arb_name("pairing-title"), "pairingTitle")
        self.assertEqual(emit_arb.arb_name("session-subagents-running"), "sessionSubagentsRunning")
        self.assertEqual(emit_arb.arb_var("folder_name"), "folderName")
        self.assertEqual(emit_arb.file_name("pt-BR"), "app_pt_BR.arb")
        self.assertEqual(emit_arb.file_name("en"), "app_en.arb")


if __name__ == "__main__":
    unittest.main()
