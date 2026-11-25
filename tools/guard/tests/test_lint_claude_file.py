import json
import unittest
from pathlib import Path

from guard.lint import claude_file_allow, rule
from tests.helpers import make_repo

ENGINE = "crates/engine/src/x.rs"
REPO = make_repo({"README.md": "x\n"})
SESSIONS = 'let file = dir.join("sessions").join(format!("{pid}.json"));\n'
STATE = 'let file = dir.join("jobs").join(id).join("state.json");\n'
STRICT = "let v: Value = serde_json::from_slice(&data).unwrap();\n"
ALLOW = "tools/guard/claude-file-parse-allow.json"


def parse(path, text, repo=REPO):
    return rule("LINT-CLAUDE-FILE-PARSE").check(path, text.splitlines(), repo)


def lines(path, text, repo=REPO):
    return [line for line, _ in parse(path, text, repo)]


class ClaudeFileParseTests(unittest.TestCase):
    def test_flags_a_strict_parse_in_a_file_that_names_a_session_file(self):
        for named in (SESSIONS, STATE, 'let k = value.get("respawnFlags");\n', 'let f = v.get("inFlight");\n'):
            for call in ("serde_json::from_slice(&d)", "serde_json::from_str(&t)", "serde_json::from_reader(r)"):
                found = parse(ENGINE, named + f"let v: Value = {call}.unwrap();\n")
                self.assertEqual([line for line, _ in found], [2], (named, call))

    def test_flags_a_strict_parse_beside_a_tolerant_one(self):
        for tolerant in ("parse_object(&data)", "parse_leading(&data)"):
            text = SESSIONS + f"let v = {tolerant};\n" + STRICT
            self.assertEqual(lines(ENGINE, text), [3], tolerant)

    def test_ignores_a_file_that_names_no_session_file(self):
        self.assertEqual(parse(ENGINE, STRICT), [])
        self.assertEqual(parse(ENGINE, 'let k = "sessions";\n' + STRICT), [])
        self.assertEqual(parse(ENGINE, 'let f = dir.join("sessions");\n' + STRICT), [])
        self.assertEqual(parse(ENGINE, 'let f = "a.json";\n' + STRICT), [])

    def test_ignores_comments_and_strings(self):
        self.assertEqual(parse(ENGINE, SESSIONS + "// serde_json::from_str(&t)\n"), [])
        self.assertEqual(parse(ENGINE, SESSIONS + 'let s = "serde_json::from_slice";\n'), [])
        self.assertEqual(parse(ENGINE, "// state.json\n" + STRICT), [])
        self.assertEqual(parse(ENGINE, "// parse_object\n" + STATE + STRICT)[0][0], 3)

    def test_ignores_serialising_and_value_conversion(self):
        text = STATE + "let b = serde_json::to_vec(&p)?;\nlet p = serde_json::from_value(v)?;\n"
        self.assertEqual(parse(ENGINE, text), [])

    def test_ignores_inline_test_module(self):
        tests = "#[cfg(test)]\nmod tests {\n    fn t() { serde_json::from_str(&t); }\n}\n"
        self.assertEqual(parse(ENGINE, SESSIONS + tests), [])
        self.assertEqual(len(parse(ENGINE, SESSIONS + STRICT + tests)), 1)

    def test_a_tolerant_parse_before_or_after_does_not_hide_a_strict_one(self):
        text = (STATE + "let a = parse_object(&d);\n" + STRICT + "let b = parse_leading(&e);\n"
                + "let w = serde_json::from_str(&u);\n" + "let c = parse_object(&f);\n")
        self.assertEqual(lines(ENGINE, text), [3, 5])

    def test_flags_the_de_module_forms(self):
        for call in ("serde_json::de::from_slice(&d)", "serde_json::de::from_str(&t)", "serde_json::de::from_reader(r)",
                     "serde_json::from_slice::<Value>(&d)", "serde_json :: from_str(&t)"):
            self.assertEqual(lines(ENGINE, STATE + f"let v = {call};\n"), [2], call)

    def test_flags_a_bare_call_when_the_file_imports_it(self):
        for use in ("use serde_json::from_slice;", "use serde_json::{from_str, Value};",
                    "use serde_json::{Value, from_reader};", "use serde_json::de::from_str;",
                    "use serde_json::de::{from_slice, Deserializer};",
                    "use serde_json::{\n    Map,\n    from_slice,\n};"):
            for call in ("from_slice(&d)", "from_str(&t)", "from_reader(r)", "from_str::<Value>(&t)"):
                text = use + "\n" + STATE + f"let v = {call};\n"
                found = parse(ENGINE, text)
                self.assertEqual([t for _, t in found], [f"let v = {call};"], (use, call))

    def test_flags_a_bare_call_when_the_file_globs_serde_json(self):
        for call in ("from_slice(&d)", "de::from_str(&t)"):
            text = "use serde_json::*;\n" + STATE + f"let v = {call};\n"
            self.assertEqual([t for _, t in parse(ENGINE, text)], [f"let v = {call};"], call)
        text = "use serde_json::value::*;\n" + STATE + "let v = from_slice(&d);\n"
        self.assertEqual(parse(ENGINE, text), [])

    def test_flags_a_de_call_when_the_file_imports_the_module(self):
        for use in ("use serde_json::de;", "use serde_json::{de, Value};", "use serde_json::{\n    Value,\n    de,\n};"):
            text = use + "\n" + STATE + "let v = de::from_slice(&d);\n"
            self.assertEqual([t for _, t in parse(ENGINE, text)], ["let v = de::from_slice(&d);"], use)

    def test_ignores_a_bare_call_the_file_does_not_import_from_serde_json(self):
        self.assertEqual(parse(ENGINE, STATE + "let v = from_slice(&d);\n"), [])
        self.assertEqual(parse(ENGINE, "use toml::from_str;\n" + STATE + "let v = from_str(&t);\n"), [])
        self.assertEqual(parse(ENGINE, "use serde_json::{Value, to_vec};\n" + STATE + "let v = from_str(&t);\n"), [])
        self.assertEqual(parse(ENGINE, STATE + "let v = toml::de::from_str(&t);\n"), [])
        self.assertEqual(parse(ENGINE, "use serde_json::Value;\n" + STATE + "let v = other::from_str(&t);\n"), [])
        self.assertEqual(parse(ENGINE, "use serde_json::Value;\n" + STATE + "let v = x.from_str(&t);\n"), [])

    def test_ignores_the_definition_of_a_from_str_method(self):
        text = "use serde_json::from_str;\n" + STATE + "impl FromStr for X {\n    fn from_str(s: &str) -> Result<Self, E> { todo() }\n}\n"
        self.assertEqual(parse(ENGINE, text), [])

    def test_a_use_statement_in_a_comment_or_after_the_test_module_does_not_import(self):
        self.assertEqual(parse(ENGINE, "// use serde_json::from_str;\n" + STATE + "let v = from_str(&t);\n"), [])
        tests = "#[cfg(test)]\nmod tests {\n    use serde_json::from_str;\n}\n"
        self.assertEqual(parse(ENGINE, STATE + "let v = from_str(&t);\n" + tests), [])

    def test_allowlist_names_the_line_by_path_and_text(self):
        entry = {"path": ENGINE, "text": "let v: Value = serde_json::from_str(&t).unwrap();", "reason": "not Claude's file"}
        repo = make_repo({ALLOW: json.dumps({"entries": [entry]})})
        text = STATE + "  let v: Value =   serde_json::from_str(&t).unwrap();\n" + STRICT
        self.assertEqual(lines(ENGINE, text, repo), [3])
        self.assertEqual(lines("crates/engine/src/y.rs", text, repo), [2, 3])

    def test_allowlist_entry_without_a_reason_does_not_count(self):
        entry = {"path": ENGINE, "text": STRICT.strip(), "reason": " "}
        repo = make_repo({ALLOW: json.dumps({"entries": [entry]})})
        self.assertEqual(lines(ENGINE, STATE + STRICT, repo), [2])

    def test_the_real_allowlist_gives_every_entry_a_reason_and_a_live_line(self):
        root = Path(__file__).resolve().parents[3]
        for e in claude_file_allow(root):
            self.assertTrue(e["reason"].strip(), e)
            file_lines = (root / e["path"]).read_text().splitlines()
            self.assertIn(e["text"], [" ".join(x.split()) for x in file_lines], e)

    def test_applies_to_crates_outside_tests_and_the_guard(self):
        r = rule("LINT-CLAUDE-FILE-PARSE")
        for path in (ENGINE, "crates/app/src/x.rs", "crates/notchlingd/src/x.rs"):
            self.assertTrue(r.applies(path), path)
        for path in ("crates/engine/src/x_tests.rs", "crates/engine/src/tests_x.rs", "crates/engine/tests/x.rs",
                     "crates/app/src/testutil.rs", "crates/engine/src/guard/state.rs", "tools/x.rs"):
            self.assertFalse(r.applies(path), path)

    def test_bad_and_good_probes_behave(self):
        r = rule("LINT-CLAUDE-FILE-PARSE")
        self.assertEqual(len(r.check(r.bad[0], r.bad[1].splitlines(), REPO)), 1)
        self.assertEqual(r.check(r.good[0], r.good[1].splitlines(), REPO), [])


if __name__ == "__main__":
    unittest.main()
