from __future__ import annotations

import unittest

from ftl import FtlError, Lit, Num, Select, Term, Var, Variant, parse

ZWNJ = "\u200c"


class ParseTests(unittest.TestCase):
    def test_plain_message(self):
        res = parse("settings-act-on-sessions = Act on Sessions\n", "x.ftl")
        self.assertEqual(len(res.messages), 1)
        msg = res.messages[0]
        self.assertEqual(msg.key, "settings-act-on-sessions")
        self.assertEqual(msg.value, [Lit("Act on Sessions")])
        self.assertEqual(msg.line, 1)

    def test_variables_and_number(self):
        res = parse("chat-moved-to = Moved to { $folder_name } at { NUMBER($n) }\n", "x.ftl")
        self.assertEqual(
            res.messages[0].value, [Lit("Moved to "), Var("folder_name"), Lit(" at "), Num("n")]
        )

    def test_plural_select(self):
        text = (
            "session-subagents-running = { $count ->\n"
            "    [one] { $count } subagent running\n"
            "   *[other] { $count } subagents running\n"
            "}\n"
        )
        value = parse(text, "x.ftl").messages[0].value
        self.assertEqual(
            value,
            Select(
                "count",
                [
                    Variant("one", [Var("count"), Lit(" subagent running")], False),
                    Variant("other", [Var("count"), Lit(" subagents running")], True),
                ],
            ),
        )

    def test_notes_attach_to_next_message(self):
        text = (
            "### File note\n"
            "## Counts\n"
            "\n"
            "# First line\n"
            "# Second line\n"
            "a-b = x\n"
            "\n"
            "c-d = y\n"
        )
        res = parse(text, "x.ftl")
        self.assertEqual(res.file_notes, ["File note"])
        self.assertEqual(res.messages[0].notes, ["First line", "Second line"])
        self.assertEqual(res.messages[0].group, "Counts")
        self.assertEqual(res.messages[0].first, 4)
        self.assertEqual(res.messages[1].notes, [])
        self.assertEqual(res.messages[1].group, "Counts")

    def test_brand_term(self):
        res = parse("-brand-notchling = Notchling\na-b = Hello { -brand-notchling }\n", "x.ftl")
        self.assertEqual(res.terms, {"notchling": "Notchling"})
        self.assertEqual(res.messages[0].value, [Lit("Hello "), Term("notchling")])

    def test_string_literals(self):
        cases = {
            'a-b = x{"{"}y\n': "x{y",
            'a-b = x{"}"}y\n': "x}y",
            'a-b = x{" "}\n': "x ",
            'a-b = x{"\\u200C"}y\n': "x" + ZWNJ + "y",
            'a-b = {" "}x\n': " x",
        }
        for text, want in cases.items():
            with self.subTest(text=text):
                self.assertEqual(parse(text, "x.ftl").messages[0].value, [Lit(want)])

    def test_refusals(self):
        rows = [
            ("a-b = { $n ->\n  [0] none\n *[other] x\n}\n", 2, "exact-number"),
            ("a-b = { $g ->\n  [male] x\n *[other] y\n}\n", 2, "category"),
            ("a-b = { $a -> } { $b -> }\n", 1, "one select"),
            ("a-b = { $n ->\n    [one] { $x -> }\n   *[other] y\n}\n", 2, "nested"),
            ("a-b = { other-key }\n", 1, "message reference"),
            ("a-b = x\n    .title = y\n", 2, "attribute"),
            ("a-b = { DATETIME($d) }\n", 1, "function"),
            ("a-b = { NUMBER($n, minimumFractionDigits: 1) }\n", 1, "options"),
            ("-term-x = y\n", 1, "-brand-"),
            ("-brand-x = a { $v }\n", 1, "variables"),
            ("a-b = one\n    two\n", 2, "one line"),
            ("a-b = x \n", 1, "space"),
            ("a-b = x\nc-d = y\tz\n", 2, "tab"),
            ("a-b = x\r\nc-d = y\r\n", 1, "CRLF"),
            ("\ufeffa-b = x\n", 1, "byte order mark"),
            ("a-b = x" + ZWNJ + "y\n", 1, "hidden character"),
            ("a-b = { $n ->\n    [one] x\n   [few] y\n}\n", 1, "*[other]"),
            ("a-b = { $count ->\n", 1, "unclosed"),
            ("a-b = { $count ->\n    [one] x\n   *[other] y\n", 3, "unclosed"),
            ("a-b = x } y\n", 1, "}"),
            ("a-b = {  \n", 1, "space"),
            ("a-b = { $x\n", 1, "unclosed"),
        ]
        for text, line, word in rows:
            with self.subTest(text=text):
                with self.assertRaises(FtlError) as cm:
                    parse(text, "x.ftl")
                err = cm.exception
                self.assertEqual(err.rule, "GUARD-LOCALE-SUBSET")
                self.assertEqual(err.line, line)
                self.assertIn(word, err.message)
                self.assertEqual(str(err), f"GUARD-LOCALE-SUBSET: x.ftl:{line}: {err.message}")

    def test_leading_space_in_value_is_refused(self):
        with self.assertRaises(FtlError) as cm:
            parse("a-b =  x\n", "x.ftl")
        self.assertIn("space", cm.exception.message)

    def test_duplicate_category_is_refused(self):
        text = "a-b = { $n ->\n    [one] x\n    [one] y\n   *[other] z\n}\n"
        with self.assertRaises(FtlError) as cm:
            parse(text, "x.ftl")
        self.assertEqual(cm.exception.line, 3)


if __name__ == "__main__":
    unittest.main()
