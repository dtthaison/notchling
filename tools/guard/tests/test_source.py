import unittest

from guard.source import strip_code, strip_comments


class StripTests(unittest.TestCase):
    def test_strip_line_comment(self):
        self.assertEqual(strip_code("x.swift", "let p = Process() // osascript").rstrip(), "let p = Process()")

    def test_strip_string(self):
        self.assertNotIn("osascript", strip_code("x.swift", 'let s = "osascript"'))

    def test_escaped_quote_in_string(self):
        out = strip_code("x.swift", r'let s = "a \" osascript"; Process()')
        self.assertNotIn("osascript", out)
        self.assertIn("Process()", out)

    def test_block_comment_on_one_line(self):
        self.assertEqual(strip_code("a.rs", "let x = 1; /* keyring */ let y = 2;").replace(" ", ""), "letx=1;lety=2;")

    def test_slashes_in_string_are_not_a_comment(self):
        self.assertIn("Process()", strip_code("x.swift", 'let u = "http://x"; Process()'))

    def test_shell_comment(self):
        self.assertEqual(strip_code("a.sh", "echo hi # osascript").rstrip(), "echo hi")

    def test_dart_single_quotes(self):
        self.assertNotIn("osascript", strip_code("a.dart", "final s = 'osascript';"))

    def test_rust_lifetime_is_not_a_string(self):
        self.assertIn("Command", strip_code("a.rs", "fn f<'a>(x: &'a str) { Command::new(x) }"))

    def test_unknown_language_unchanged(self):
        self.assertEqual(strip_code("a.md", "// keep"), "// keep")

    def test_strip_comments_keeps_strings(self):
        self.assertEqual(strip_comments("x.swift", 'run("/usr/bin/security") // x').rstrip(), 'run("/usr/bin/security")')

    def test_length_kept(self):
        line = 'let s = "abc" // c'
        self.assertEqual(len(strip_code("x.swift", line)), len(line))


if __name__ == "__main__":
    unittest.main()
