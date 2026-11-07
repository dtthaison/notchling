import unittest

from guard.capability import RULES, scan
from guard.changes import Change

ZONE = "Sources/Notchling/Search/X.swift"

BAD = [
    ("CAP-PROCESS", ZONE, "let p = Process()"),
    ("CAP-PROCESS", "crates/app/src/a.rs", "let c = std::process::Command::new(\"ls\");"),
    ("CAP-PTY", ZONE, "let pid = forkpty(&master, nil, nil, nil)"),
    ("CAP-PTY", "crates/app/src/a.rs", "use portable_pty::native_pty_system;"),
    ("CAP-APPLESCRIPT", ZONE, "let s = NSAppleScript(source: src)"),
    ("CAP-NETWORK", ZONE, "URLSession.shared.dataTask(with: url)"),
    ("CAP-NETWORK", "mobile/lib/features/chat/a.dart", "import 'package:http/http.dart';"),
    ("CAP-KEYCHAIN", ZONE, "SecItemCopyMatching(query, &out)"),
    ("CAP-KEYCHAIN", ZONE, 'run("/usr/bin/security", args)'),
    ("CAP-KEYCHAIN", "crates/app/src/a.rs", "let e = keyring::Entry::new(s, u);"),
    ("CAP-FILEWRITE", ZONE, "try FileManager.default.removeItem(at: url)"),
    ("CAP-FILEWRITE", ZONE, "try data.write(to: url)"),
    ("CAP-PROTECTED-TYPE", "Sources/Notchling/Chat/Views/Row.swift", "ReplySender.send(text, to: agent)"),
]

GOOD = [
    (ZONE, "let processed = items.map { $0 }"),
    (ZONE, "// never call osascript here"),
    (ZONE, 'Text("Keychain is not touched")'),
    ("README.md", "Run `Process()` like this"),
]


def added(path, *lines):
    return Change(path, "M", None, tuple((i + 1, l) for i, l in enumerate(lines)))


class CapabilityTests(unittest.TestCase):
    def test_each_rule_fires_on_its_bad_example(self):
        for rule, path, line in BAD:
            self.assertIn(rule, {t.rule for t in scan([added(path, line)])}, f"{rule}: {line}")

    def test_every_rule_has_a_bad_example(self):
        self.assertEqual(set(RULES), {r for r, _, _ in BAD})

    def test_good_examples_pass(self):
        for path, line in GOOD:
            self.assertEqual(scan([added(path, line)]), [], line)

    def test_comment_does_not_trip(self):
        self.assertEqual(scan([added("Sources/Notchling/Limits/LimitWatcher.swift", "// never osascript")]), [])

    def test_protected_type_in_own_file_ok(self):
        self.assertEqual(scan([added("Sources/Notchling/Agents/ReplySenderQueue.swift", "ReplySender.send(x)")]), [])

    def test_protected_type_elsewhere_trips(self):
        trips = scan([added("Sources/Notchling/Chat/Views/Row.swift", "ReplySender.send(x)")])
        self.assertEqual([t.rule for t in trips], ["CAP-PROTECTED-TYPE"])

    def test_removed_lines_ignored(self):
        self.assertEqual(scan([Change(ZONE, "M", None, (), ("let p = Process()",))]), [])

    def test_trip_has_location(self):
        [t] = scan([added(ZONE, "let a = 1", "let p = Process()")])
        self.assertEqual((t.path, t.line, t.text), (ZONE, 2, "let p = Process()"))


if __name__ == "__main__":
    unittest.main()
