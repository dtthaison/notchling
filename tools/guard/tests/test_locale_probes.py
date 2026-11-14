import unittest
from pathlib import Path

from guard import rules

DOCS = Path(__file__).resolve().parents[3] / "docs/rules.md"
IDS = ("GUARD-LOCALE-GEN", "GUARD-LOCALE-SUBSET", "GUARD-LOCALE-MATCH")


class LocaleProbes(unittest.TestCase):
    def test_each_locale_probe_fires(self):
        for rule_id in IDS:
            with self.subTest(rule=rule_id):
                self.assertTrue(rules.REGISTRY[rule_id]())

    def test_rules_md_lists_them(self):
        text = DOCS.read_text(encoding="utf-8")
        for rule_id in IDS:
            self.assertIn(rule_id, rules.listed_ids(text))
        problems = rules.check(DOCS, run_probes=False)
        self.assertEqual([p for p in problems if "GUARD-LOCALE" in p], [])


if __name__ == "__main__":
    unittest.main()
