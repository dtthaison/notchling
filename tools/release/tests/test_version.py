from __future__ import annotations

import unittest

import version


class ParseTests(unittest.TestCase):
    def test_parse_tag(self):
        self.assertEqual(version.parse_tag("v1.4.1"), (1, 4, 1, None))
        self.assertEqual(version.parse_tag("v1.4.1-beta.12"), (1, 4, 1, 12))

    def test_malformed_tags(self):
        for tag in ("1.4.1", "v1.4", "v1.4.1.2", "v1.4.1-beta", "v1.4.1-beta.0", "v1.4.1-rc.1", "v01.4.1",
                    "v1.4.1-beta.01", "v1.4.1-beta.1-x", "v1.4.1 ", "vx.y.z", "", "v-1.0.0"):
            with self.subTest(tag):
                self.assertIsNone(version.parse_tag(tag))


class BuildNumberTests(unittest.TestCase):
    def test_formula(self):
        self.assertEqual(version.build_number((1, 4, 1, 3)), 1 * 1000000 + 4 * 10000 + 1 * 100 + 3)
        self.assertEqual(version.build_number((1, 4, 1, None)), 1 * 1000000 + 4 * 10000 + 1 * 100 + 99)

    def test_ordering_matches_release_order(self):
        order = [(1, 2, 0, None), (1, 2, 1, 1), (1, 2, 1, 2), (1, 2, 1, 98), (1, 2, 1, None),
                 (1, 2, 2, 1), (1, 3, 0, 1), (1, 3, 0, None), (1, 10, 0, 1), (2, 0, 0, 1), (2, 0, 0, None)]
        numbers = [version.build_number(v) for v in order]
        self.assertEqual(numbers, sorted(numbers))
        self.assertEqual(len(set(numbers)), len(numbers))

    def test_stable_is_above_every_beta_of_it(self):
        self.assertGreater(version.build_number((3, 7, 5, None)), version.build_number((3, 7, 5, 98)))

    def test_out_of_range_is_refused(self):
        for v in ((1, 100, 0, None), (1, 0, 100, None), (1, 0, 0, 99), (1, 0, 0, 0)):
            with self.subTest(v):
                with self.assertRaises(ValueError):
                    version.build_number(v)

    def test_above_the_legacy_counter(self):
        self.assertGreater(version.build_number((1, 2, 0, None)), 3)


if __name__ == "__main__":
    unittest.main()
