import json
import unittest

from guard.gate import PROVIDER_FILES, provider_gap_reasons
from tests.helpers import commit, head_of, make_repo

GAPS, UNWIRED = PROVIDER_FILES


def entries(*names):
    return json.dumps({"entries": list(names)})


class ProviderGapTests(unittest.TestCase):
    def reasons(self, base_files, head_files):
        repo = make_repo(base_files)
        base = head_of(repo)
        head = commit(repo, head_files)
        return provider_gap_reasons(repo, base, head)

    def test_shrinking_is_fine(self):
        base = {GAPS: entries("codex subagent.list", "codex subagent.open"),
                UNWIRED: entries("claude session.list")}
        self.assertEqual(self.reasons(base, {GAPS: entries("codex subagent.open"),
                                             UNWIRED: entries()}), [])

    def test_growing_gaps_is_a_reason(self):
        base = {GAPS: entries("codex subagent.list"), UNWIRED: entries()}
        reasons = self.reasons(base, {GAPS: entries("codex subagent.list", "codex turn.hold")})
        self.assertEqual(len(reasons), 1)
        self.assertTrue(reasons[0].startswith("GUARD-PROVIDER-GAPS:"))
        self.assertIn(GAPS, reasons[0])
        self.assertIn("codex turn.hold", reasons[0])

    def test_growing_unwired_is_a_reason(self):
        base = {GAPS: entries(), UNWIRED: entries("claude session.list")}
        reasons = self.reasons(base, {UNWIRED: entries("claude session.list", "claude session.new")})
        self.assertEqual(len(reasons), 1)
        self.assertIn(UNWIRED, reasons[0])
        self.assertIn("claude session.new", reasons[0])

    def test_new_file_counts_as_growth(self):
        repo = make_repo({"x": "1\n"})
        base = head_of(repo)
        head = commit(repo, {GAPS: entries("codex subagent.list", "codex subagent.open")})
        reasons = provider_gap_reasons(repo, base, head)
        self.assertEqual(len(reasons), 1)
        self.assertIn("grew by 2 entries", reasons[0])

    def test_unreadable_head_is_a_reason(self):
        base = {GAPS: entries("codex subagent.list"), UNWIRED: entries()}
        reasons = self.reasons(base, {GAPS: "{ not json"})
        self.assertEqual(len(reasons), 1)
        self.assertTrue(reasons[0].startswith("GUARD-PROVIDER-GAPS:"))
        self.assertIn("unreadable", reasons[0])

    def test_missing_at_the_head_is_fine(self):
        repo = make_repo({"x": "1\n"})
        base = head_of(repo)
        head = commit(repo, {"y": "2\n"})
        self.assertEqual(provider_gap_reasons(repo, base, head), [])


if __name__ == "__main__":
    unittest.main()
