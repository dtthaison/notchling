import unittest

from guard.lint import rule
from tests.helpers import make_repo

REPO = make_repo({"README.md": "x\n"})
KINDS = "crates/events/src/kinds/x.rs"


def no_text(path, text):
    return rule("LINT-LOG-NO-TEXT").check(path, text.splitlines(), REPO)


def one_writer(path, text):
    return rule("LINT-LOG-ONE-WRITER").check(path, text.splitlines(), REPO)


class NoTextTests(unittest.TestCase):
    def test_no_text_flags_string_in_kinds(self):
        for text in ["fields { note: String }", "name: &'static str,", "v: serde_json::Value,", "raw: Vec<u8>,"]:
            self.assertEqual(len(no_text(KINDS, text)), 1, text)

    def test_no_text_allows_count(self):
        self.assertEqual(no_text(KINDS, "fields { n: Count, v: Version, p: Option<Ref<Session>> }"), [])

    def test_no_text_ignores_words_in_strings_and_names(self):
        self.assertEqual(no_text(KINDS, 'title: "A String Value",\npub enum SettingValue {'), [])

    def test_no_text_flags_impl_outside_field_files(self):
        self.assertEqual(len(no_text("crates/engine/src/x.rs", "impl LogField for Foo {")), 1)
        self.assertEqual(len(no_text("crates/events/src/kinds/x.rs", "    impl<T> LogField for List<T> {")), 1)
        self.assertEqual(len(no_text("crates/events/src/other.rs", "impl crate::field::LogField for Foo {")), 1)

    def test_no_text_allows_impl_in_field_macros(self):
        for path in ["crates/events/src/field_macros.rs", "crates/events/src/field.rs", "crates/events/src/field_ref.rs"]:
            self.assertEqual(no_text(path, "impl $crate::field::LogField for $name {"), [], path)

    def test_no_text_allows_a_trait_bound_and_a_call(self):
        self.assertEqual(no_text("crates/engine/src/x.rs", "fn f<T: LogField>(t: &T) {}\nlet v = LogField::value(&t);"), [])

    def test_no_text_allows_string_outside_kinds(self):
        self.assertEqual(no_text("crates/events/src/doc.rs", "let s: String = String::new();"), [])


class OneWriterTests(unittest.TestCase):
    def test_one_writer_flags_eprintln_and_tracing(self):
        for text in ['eprintln!("x");', 'println!("x");', 'print!("x");', 'eprint!("x");', 'tracing::warn!("x");',
                     'log::info!("x");', "use tracing::info;", "use log::warn;"]:
            self.assertEqual(len(one_writer("crates/app/src/x.rs", text)), 1, text)

    def test_one_writer_skips_inline_test_module(self):
        text = 'fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() { eprintln!("x"); }\n}\n'
        self.assertEqual(one_writer("crates/app/src/x.rs", text), [])

    def test_one_writer_flags_code_before_the_test_module(self):
        text = 'fn a() { eprintln!("x"); }\n#[cfg(test)]\nmod tests {\n    fn t() { eprintln!("x"); }\n}\n'
        self.assertEqual([n for n, _ in one_writer("crates/app/src/x.rs", text)], [1])

    def test_one_writer_does_not_cut_at_an_external_test_module(self):
        text = '#[cfg(test)]\nmod x_tests;\nfn a() { eprintln!("x"); }\n'
        self.assertEqual(len(one_writer("crates/app/src/x.rs", text)), 1)

    def test_one_writer_skips_cli_output_files(self):
        for name in ["clients", "pages_cli", "vault_cli", "delegate_cli", "settings_cli", "service", "lib", "main"]:
            self.assertFalse(rule("LINT-LOG-ONE-WRITER").applies(f"crates/notchlingd/src/{name}.rs"), name)
        for path in ["crates/app/src/mac/render.rs", "crates/app/src/mac/bench.rs", "crates/app/src/chat/view/list_probe_run.rs"]:
            self.assertFalse(rule("LINT-LOG-ONE-WRITER").applies(path), path)

    def test_one_writer_applies_to_other_sources_and_skips_tests_and_events(self):
        r = rule("LINT-LOG-ONE-WRITER")
        for path in ["crates/app/src/x.rs", "crates/engine/src/a/b.rs", "crates/ptyhost/src/main.rs", "crates/notchlingd/src/hook.rs"]:
            self.assertTrue(r.applies(path), path)
        for path in ["crates/events/src/doc.rs", "crates/app/src/x_tests.rs", "crates/app/src/tests_chat.rs",
                     "crates/app/src/tests.rs", "crates/engine/src/testutil.rs", "crates/engine/src/test_support.rs",
                     "crates/engine/tests/x.rs", "crates/platform/examples/watch_cpu.rs"]:
            self.assertFalse(r.applies(path), path)

    def test_one_writer_ignores_writeln_and_format(self):
        self.assertEqual(one_writer("crates/app/src/x.rs", 'writeln!(out, "x");\nlet s = format!("x");'), [])

    def test_one_writer_ignores_names_that_end_in_print(self):
        self.assertEqual(one_writer("crates/app/src/x.rs", "let a = pretty_print!(x);\nlet b = Self::print(x);"), [])


if __name__ == "__main__":
    unittest.main()
