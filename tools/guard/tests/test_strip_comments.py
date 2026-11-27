import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

import strip_comments as sc  # noqa: E402


def strip(lang, text, **kw):
    return sc.strip_text(lang, "x", text, kw.get("strip_safety", False)).text


class RustTests(unittest.TestCase):
    def test_line_doc_and_block_comments_go(self):
        src = "//! top\n\n/// doc\nfn a() {\n    // note\n    let x = 1; // tail\n    /* b */ let y = 2;\n}\n"
        self.assertEqual(strip("rust", src), "fn a() {\n    let x = 1;\n    let y = 2;\n}\n")

    def test_strings_hold_slashes(self):
        src = 'let u = "http://a/*b*/"; // gone\nlet r = r#"// "quoted" /* x */"#;\n'
        self.assertEqual(strip("rust", src), 'let u = "http://a/*b*/";\nlet r = r#"// "quoted" /* x */"#;\n')

    def test_char_literals_and_lifetimes(self):
        src = "fn f<'a>(s: &'a str) -> char { let q = '\"'; let b = b'/'; q } // x\n"
        self.assertEqual(strip("rust", src), "fn f<'a>(s: &'a str) -> char { let q = '\"'; let b = b'/'; q }\n")

    def test_nested_block_comment(self):
        self.assertEqual(strip("rust", "a /* x /* y */ z */ b\n"), "a b\n")

    def test_safety_comment_is_kept_with_its_continuation(self):
        src = "    // SAFETY: the pointer is live\n    // and aligned.\n\n    // other\n    unsafe { f() }\n"
        self.assertEqual(strip("rust", src), "    // SAFETY: the pointer is live\n    // and aligned.\n\n    unsafe { f() }\n")

    def test_safety_can_be_stripped(self):
        self.assertEqual(strip("rust", "// SAFETY: x\nunsafe { f() }\n", strip_safety=True), "unsafe { f() }\n")

    def test_blank_lines_do_not_pile_up(self):
        src = "fn a() {}\n\n// h\n\nfn b() {}\n"
        self.assertEqual(strip("rust", src), "fn a() {}\n\nfn b() {}\n")

    def test_license_header_is_kept(self):
        src = "// SPDX-License-Identifier: MIT\n// other\nfn a() {}\n"
        self.assertEqual(strip("rust", src), "// SPDX-License-Identifier: MIT\nfn a() {}\n")

    def test_idempotent(self):
        src = "/// d\nfn a() { let s = \"//\"; } // t\n"
        once = strip("rust", src)
        self.assertEqual(strip("rust", once), once)


class DartTests(unittest.TestCase):
    def test_interpolation_and_strings(self):
        src = "final u = 'http://x/${a + \"//\"}'; // gone\n/// doc\nfinal r = r'//';\n"
        out = strip("dart", src)
        self.assertIn("http://x/${a + \"//\"}", out)
        self.assertIn("r'//'", out)
        self.assertNotIn("gone", out)
        self.assertNotIn("doc", out)

    def test_ignore_directives_stay(self):
        src = "// ignore: unused_element\nvoid a() {}\n// other\n"
        self.assertEqual(strip("dart", src), "// ignore: unused_element\nvoid a() {}\n")


class PythonTests(unittest.TestCase):
    def test_comments_docstrings_and_directives(self):
        src = (
            "#!/usr/bin/env python3\n"
            '"""module doc"""\n'
            "import os  # noqa: F401\n"
            "def f():\n"
            '    """only a docstring"""\n'
            "def g():\n"
            '    """doc"""\n'
            "    return 1  # why\n"
            's = "# not a comment"\n'
        )
        want = (
            "#!/usr/bin/env python3\n"
            "import os  # noqa: F401\n"
            "def f():\n"
            "    pass\n"
            "def g():\n"
            "    return 1\n"
            's = "# not a comment"\n'
        )
        self.assertEqual(strip("python", src), want)

    def test_module_docstring_read_at_runtime_stays(self):
        src = '"""help text"""\nprint(__doc__)\n'
        self.assertEqual(strip("python", src), src)


class ShellTests(unittest.TestCase):
    def test_comments_heredocs_and_expansions(self):
        src = (
            "#!/bin/sh\n"
            "# top\n"
            "echo \"$#\" ${#x} ${y#pre} a#b # tail\n"
            "cat <<'EOF'\n"
            "# stays in the heredoc\n"
            "EOF\n"
            "echo '# quoted' \"# also\"\n"
            "# shellcheck disable=SC2086\n"
            "echo $((1 << 2))\n"
        )
        want = (
            "#!/bin/sh\n"
            "echo \"$#\" ${#x} ${y#pre} a#b\n"
            "cat <<'EOF'\n"
            "# stays in the heredoc\n"
            "EOF\n"
            "echo '# quoted' \"# also\"\n"
            "# shellcheck disable=SC2086\n"
            "echo $((1 << 2))\n"
        )
        self.assertEqual(strip("shell", src), want)


class ScopeTests(unittest.TestCase):
    def test_what_is_in_scope(self):
        lang = sc.language_of
        self.assertEqual(lang("crates/engine/src/lib.rs"), "rust")
        self.assertIsNone(lang("crates/protocol/src/generated.rs"))
        self.assertIsNone(lang("vendor/gpui/src/lib.rs"))
        self.assertIsNone(lang("legacy/macos/Sources/x.swift"))
        self.assertEqual(lang("apps/flutter/lib/main.dart"), "dart")
        self.assertIsNone(lang("apps/flutter/lib/l10n/app_localizations_en.dart"))
        self.assertEqual(lang("tools/guard/guard/lint.py"), "python")
        self.assertEqual(lang("build.sh"), "shell")
        self.assertEqual(lang("tools/bundle-next.sh"), "shell")
        self.assertIsNone(lang("apps/flutter/tool/sync_viz.sh"))


if __name__ == "__main__":
    unittest.main()
