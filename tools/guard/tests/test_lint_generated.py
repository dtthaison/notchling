"""LINT-FILE-SIZE leaves generated files alone: nobody reviews or splits what a generator writes."""

import unittest

from guard import lint
from tests.helpers import make_repo

LONG = "x\n" * 1500


class GeneratedFileSizeTests(unittest.TestCase):
    def lines(self, path, text):
        repo = make_repo({path: text})
        return [v for v in lint.run(repo, [path]) if v.rule == "LINT-FILE-SIZE"]

    def test_a_long_hand_written_file_still_fails(self):
        self.assertEqual(len(self.lines("crates/protocol/src/long.rs", LONG)), 1)

    def test_a_rust_file_marked_generated_in_its_first_lines_is_exempt(self):
        text = "//! Types.\n//! @generated: do not edit.\n" + LONG
        self.assertEqual(self.lines("crates/protocol/src/generated.rs", text), [])

    def test_a_dart_file_marked_generated_is_exempt(self):
        text = "// GENERATED CODE - DO NOT MODIFY BY HAND.\n" + LONG
        self.assertEqual(self.lines("apps/flutter/lib/core/protocol/generated.dart", text), [])

    def test_the_mark_far_down_the_file_does_not_count(self):
        text = LONG + "// GENERATED CODE\n"
        self.assertEqual(len(self.lines("crates/protocol/src/long.rs", text)), 1)


if __name__ == "__main__":
    unittest.main()
