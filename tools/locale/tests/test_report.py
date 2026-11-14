from __future__ import annotations

import unittest

import check
import report
from tests.helpers import TREE, small_tree


class ReportTests(unittest.TestCase):
    def setUp(self):
        self.tree, _ = check.load(TREE)

    def row(self, tag):
        return [r for r in report.rows(self.tree) if r.tag == tag][0]

    def test_report_numbers(self):
        de = self.row("de")
        self.assertEqual(de.by_part["mac"], 50.0)
        self.assertEqual(de.by_part["shared"], 60.0)
        self.assertEqual(de.by_part["consent"], 0.0)
        self.assertEqual(de.by_part["phone"], 0.0)
        en = self.row("en")
        self.assertEqual(en.all, 100.0)
        self.assertEqual(en.same_as_english, 0)

    def test_same_as_english_counts_equal_values(self):
        work = small_tree(
            {
                "en/shared.ftl": "language-name = English\nstate-x = Same\nstate-y = Other\n",
                "de/shared.ftl": "language-name = Deutsch\nstate-x = Same\nstate-y = Anders\n",
            },
            draft=("de",),
        )
        try:
            tree, _ = check.load(work.locales)
            self.assertEqual(report.rows(tree, "de")[0].same_as_english, 1)
        finally:
            work.cleanup()

    def test_rows_filter_by_language(self):
        self.assertEqual([r.tag for r in report.rows(self.tree, "pt")], ["pt"])

    def test_text_table(self):
        text = report.text(report.rows(self.tree))
        lines = text.splitlines()
        self.assertTrue(lines[0].startswith("Language  shared  consent  mac    phone  all    same as English"))
        self.assertTrue(lines[1].startswith("en        100%    100%     100%   100%   100%   0"))
        self.assertIn("de        60%", text)

    def test_markdown_table_header(self):
        text = report.markdown(report.rows(self.tree))
        self.assertEqual(
            text.splitlines()[:2],
            [
                "| Language | shared | consent | mac | phone | all | same as English |",
                "| --- | --- | --- | --- | --- | --- | --- |",
            ],
        )

    def test_missing_lists_keys_sorted(self):
        keys = report.missing(self.tree, "de")
        self.assertIn("units-age-minutes", keys)
        self.assertNotIn("chat-move", keys)
        self.assertEqual(keys, sorted(keys))


if __name__ == "__main__":
    unittest.main()
