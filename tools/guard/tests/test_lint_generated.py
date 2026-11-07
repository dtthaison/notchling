"""LINT-FILE-SIZE and LINT-FILE-SIZE-WARN leave the two generated protocol files alone, by exact path:
nobody reviews or splits what tools/protocol/gen.py writes. A mark inside a file is no bypass."""

import unittest

from guard import lint
from tests.helpers import make_repo

LONG = "x\n" * 1500
RUST_MARK = "//! Types.\n//! @generated: do not edit.\n"
DART_MARK = "// GENERATED CODE - DO NOT MODIFY BY HAND.\n"
GENERATED_RS = "crates/protocol/src/generated.rs"
GENERATED_DART = "apps/flutter/lib/core/protocol/generated.dart"
SIZE_RULES = ("LINT-FILE-SIZE", "LINT-FILE-SIZE-WARN")


class GeneratedFileSizeTests(unittest.TestCase):
    def found(self, path, text, rule="LINT-FILE-SIZE"):
        repo = make_repo({path: text})
        return [v for v in lint.run(repo, [path]) if v.rule == rule]

    def test_a_long_hand_written_file_still_fails(self):
        self.assertEqual(len(self.found("crates/protocol/src/long.rs", LONG)), 1)

    def test_the_two_generated_files_are_exempt_from_both_size_rules(self):
        for path, mark in [(GENERATED_RS, RUST_MARK), (GENERATED_DART, DART_MARK)]:
            for rule in SIZE_RULES:
                self.assertEqual(self.found(path, mark + LONG, rule), [], (path, rule))

    def test_the_listed_paths_are_exempt_by_path_alone(self):
        for path in (GENERATED_RS, GENERATED_DART):
            for rule in SIZE_RULES:
                self.assertEqual(self.found(path, LONG, rule), [], (path, rule))

    def test_a_rust_file_marked_generated_at_another_path_is_still_flagged(self):
        text = RUST_MARK + LONG
        for rule in SIZE_RULES:
            self.assertEqual(len(self.found("crates/engine/src/big.rs", text, rule)), 1, rule)

    def test_a_dart_file_marked_generated_at_another_path_is_still_flagged(self):
        text = DART_MARK + LONG
        for rule in SIZE_RULES:
            self.assertEqual(len(self.found("apps/flutter/lib/features/big.dart", text, rule)), 1, rule)

    def test_a_marked_swift_file_is_still_flagged(self):
        text = "// @generated\n" + LONG
        self.assertEqual(len(self.found("legacy/macos/Sources/Notchling/Big.swift", text)), 1)

    def test_a_lookalike_path_is_not_exempt(self):
        for path in ("crates/protocol/src/generated.rs.bak", "crates/other/src/generated.rs",
                     "crates/protocol/src/sub/generated.rs", "apps/flutter/lib/core/protocol/generated.dart.orig",
                     "apps/flutter/lib/core/other/generated.dart"):
            self.assertEqual(len(self.found(path, RUST_MARK + LONG)), 1, path)


if __name__ == "__main__":
    unittest.main()
