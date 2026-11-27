import json
import unittest
from pathlib import Path

from guard.lint import rule, unbounded_allow
from tests.helpers import make_repo

ENGINE = "crates/engine/src/x.rs"
DAEMON = "crates/notchlingd/src/x.rs"
REPO = make_repo({"README.md": "x\n"})


def wait(path, text, repo=REPO):
    return rule("LINT-NO-UNBOUNDED-WAIT").check(path, text.splitlines(), repo)


class UnboundedWaitTests(unittest.TestCase):
    def test_flags_each_unbounded_call(self):
        for text in ["let v = rx.recv();", "let _ = handle.join();", "g = cv.wait(g).unwrap();",
                     "state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());",
                     "let g = cv.wait_while(g, |s| !*s);", "match rx.recv() {"]:
            self.assertEqual(len(wait(ENGINE, text)), 1, text)

    def test_allows_bounded_calls(self):
        for text in ["let v = rx.recv_timeout(d);", "let (g, t) = cv.wait_timeout(g, d).unwrap();",
                     "let v = rx.recv().await;", 'let s = parts.join(", ");', "let p = dir.join(name);",
                     "book.wait(LOOP_EVERY);", "patience.wait(why, now)"]:
            self.assertEqual(wait(ENGINE, text), [], text)

    def test_ignores_comments_and_strings(self):
        self.assertEqual(wait(ENGINE, '// rx.recv()\nlet s = "x.join()";'), [])

    def test_ignores_inline_test_module(self):
        text = "fn f() {}\n#[cfg(test)]\nmod tests {\n    fn t() { rx.recv(); }\n}\n"
        self.assertEqual(wait(ENGINE, text), [])
        self.assertEqual(len(wait(ENGINE, "fn f() { rx.recv(); }\n" + text)), 1)

    def test_flags_a_socket_in_a_file_without_a_read_timeout(self):
        text = "let s = UnixStream::connect(path)?;"
        self.assertEqual(len(wait(DAEMON, text)), 1)
        self.assertEqual(wait(DAEMON, text + "\ns.set_read_timeout(Some(d))?;"), [])

    def test_applies_to_daemon_and_engine_only(self):
        r = rule("LINT-NO-UNBOUNDED-WAIT")
        self.assertTrue(r.applies(ENGINE))
        self.assertTrue(r.applies(DAEMON))
        self.assertFalse(r.applies("crates/app/src/x.rs"))
        self.assertFalse(r.applies("crates/engine/src/x_tests.rs"))
        self.assertFalse(r.applies("crates/notchlingd/src/tests_x.rs"))
        self.assertFalse(r.applies("crates/engine/tests/x.rs"))

    def test_allowlist_names_the_line_by_path_and_text(self):
        entry = {"path": ENGINE, "text": "let v = rx.recv();", "reason": "idle worker"}
        repo = make_repo({"tools/guard/unbounded-wait-allow.json": json.dumps({"entries": [entry]})})
        self.assertEqual(wait(ENGINE, "  let v =   rx.recv();", repo), [])
        self.assertEqual(len(wait("crates/engine/src/y.rs", "let v = rx.recv();", repo)), 1)

    def test_allowlist_entry_without_a_reason_does_not_count(self):
        entry = {"path": ENGINE, "text": "let v = rx.recv();", "reason": ""}
        repo = make_repo({"tools/guard/unbounded-wait-allow.json": json.dumps({"entries": [entry]})})
        self.assertEqual(len(wait(ENGINE, "let v = rx.recv();", repo)), 1)

    def test_the_real_allowlist_gives_every_entry_a_reason_and_a_live_line(self):
        root = Path(__file__).resolve().parents[3]
        entries = unbounded_allow(root)
        self.assertTrue(entries)
        for e in entries:
            self.assertTrue(e["reason"].strip(), e)
            lines = (root / e["path"]).read_text().splitlines()
            self.assertIn(e["text"], [" ".join(x.split()) for x in lines], e)


if __name__ == "__main__":
    unittest.main()
