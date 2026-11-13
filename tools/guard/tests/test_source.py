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

    def test_swift_interpolation_is_code(self):
        out = strip_code("x.swift", 'Text("abc \\(Process()) def")')
        self.assertIn("Process()", out)
        self.assertNotIn("abc", out)
        self.assertNotIn("def", out)

    def test_swift_interpolation_tracks_paren_depth(self):
        out = strip_code("x.swift", 'let r = "\\(f(g(1), (2))) tail"; Process()')
        self.assertIn("f(g(1), (2))", out)
        self.assertNotIn("tail", out)
        self.assertIn("; Process()", out)

    def test_swift_string_inside_interpolation_is_blanked(self):
        out = strip_code("x.swift", 'Text("\\(a("p)q Process()")) Process()")')
        self.assertNotIn("Process", out)

    def test_swift_interpolation_closes_at_its_own_paren(self):
        out = strip_code("x.swift", 'let a = "\\(x)"; let p = Process()')
        self.assertIn("let p = Process()", out)

    def test_swift_unterminated_interpolation_stays_code(self):
        self.assertIn("Process()", strip_code("x.swift", 'let a = "\\(Process()'))

    def test_swift_raw_string_interpolation_is_code(self):
        out = strip_code("x.swift", 'let r = #"text \\#(Process()) more"#')
        self.assertIn("Process()", out)
        self.assertNotIn("text", out)

    def test_swift_raw_string_backslash_paren_is_text(self):
        self.assertNotIn("Process", strip_code("x.swift", 'let r = #"\\(Process())"#'))

    def test_swift_raw_string_holds_quotes(self):
        out = strip_code("x.swift", 'let r = #"say "hi" Process()"#; let p = Process()')
        self.assertEqual(out.count("Process()"), 1)
        self.assertIn("; let p = Process()", out)

    def test_dart_interpolation_is_code(self):
        out = strip_code("a.dart", "print('hello ${Process.runSync(\"ls\", [])} there');")
        self.assertIn("Process.runSync", out)
        self.assertNotIn("hello", out)

    def test_dart_dollar_name_is_text(self):
        self.assertNotIn("name", strip_code("a.dart", "print('hello $name');"))

    def test_interpolation_keeps_length(self):
        line = 'let r = "a \\(f(1)) b" // c'
        self.assertEqual(len(strip_code("x.swift", line)), len(line))

    def test_strip_comments_keeps_interpolated_text(self):
        line = 'run("/usr/bin/security \\(args)") // x'
        self.assertEqual(strip_comments("x.swift", line).rstrip(), 'run("/usr/bin/security \\(args)")')

    def test_swift_apostrophe_is_not_a_quote(self):
        self.assertIn("Process()", strip_code("x.swift", "let a = x' ; let p = Process(); let b = y'"))

    def test_swift_backtick_identifier_is_not_a_quote_opener(self):
        out = strip_code("x.swift", "let `a'` = 0; let p = Process(); let `b'` = 1")
        self.assertIn("let p = Process();", out)

    def test_swift_backtick_identifier_may_hold_a_double_quote(self):
        self.assertIn("Process()", strip_code("x.swift", 'let `a"` = 0; let p = Process(); let `b"` = 1'))

    def test_swift_unclosed_backtick_is_a_plain_character(self):
        self.assertIn("Process()", strip_code("x.swift", "let a = ` ; Process()"))

    def test_swift_comment_marker_in_backticks_is_not_a_comment(self):
        self.assertIn("Process()", strip_code("x.swift", "let `a//b` = 0; let p = Process()"))

    def test_python_and_shell_single_quotes_still_strings(self):
        self.assertNotIn("osascript", strip_code("a.py", "s = 'osascript'"))
        self.assertNotIn("osascript", strip_code("a.sh", "echo 'osascript'"))

    def test_swift_triple_quote_is_not_a_string_opener(self):
        self.assertIn("Process()", strip_code("x.swift", '  """; let p = Process()'))
        self.assertIn("Process()", strip_code("x.swift", '  """; let p = Process(); let t = """'))

    def test_swift_regex_literal_may_hold_a_quote(self):
        out = strip_code("x.swift", 'let r = #/"/#; let p = Process(); let s = #/"/#')
        self.assertIn("; let p = Process(); ", out)

    def test_swift_regex_literal_text_is_blanked(self):
        out = strip_code("x.swift", 'let r = #/Process()|a"b/#; run()')
        self.assertNotIn("Process", out)
        self.assertIn("run()", out)

    def test_swift_extended_regex_with_more_hashes(self):
        out = strip_code("x.swift", 'let r = ##/a/#b"/##; let p = Process()')
        self.assertIn("let p = Process()", out)
        self.assertNotIn("a/#b", out)

    def test_swift_unterminated_regex_is_blanked_to_the_end(self):
        self.assertNotIn("Process", strip_code("x.swift", "let r = #/Process()"))

    def test_rust_raw_string_holds_quotes(self):
        out = strip_code("a.rs", 'let s = r#"x"y"#; std::process::Command::new(a)')
        self.assertIn("std::process::Command::new(a)", out)
        self.assertNotIn("x", out.replace("std::process::Command::new(a)", ""))

    def test_rust_raw_string_has_no_escapes(self):
        self.assertIn("Command::new", strip_code("a.rs", 'let s = r"\\"; Command::new(a)'))

    def test_rust_byte_raw_string(self):
        self.assertIn("Command::new", strip_code("a.rs", 'let s = br#"a"#; Command::new(a)'))

    def test_rust_identifier_ending_in_r_is_not_a_raw_string(self):
        self.assertNotIn("Command", strip_code("a.rs", 'let fr = "x"; let s = "Command::new(a)";'))

    def test_dart_raw_string_has_no_escapes(self):
        self.assertIn("Process.run", strip_code("a.dart", "final s = r'\\'; Process.run(a, []);"))

    def test_dart_raw_string_has_no_interpolation(self):
        self.assertNotIn("Process", strip_code("a.dart", "final s = r'${Process.run}';"))

    def test_absurd_nesting_does_not_crash_and_stays_visible(self):
        line = '"\\(' * 2000 + "Process()"
        out = strip_code("x.swift", line)
        self.assertEqual(len(out), len(line))
        self.assertIn("Process()", out)

    def test_swift_hash_directives_are_plain_code(self):
        self.assertIn("Process()", strip_code("x.swift", "#if DEBUG; let p = Process(); #endif"))


if __name__ == "__main__":
    unittest.main()
