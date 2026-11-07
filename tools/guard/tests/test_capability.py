import re
import unittest
from pathlib import Path

from guard.capability import OWN_FILES, PROTECTED_TYPES, RULES, scan
from guard.changes import Change, diff
from guard.zones import load as load_zones
from tests.helpers import commit, head_of, make_repo

ZONE = "Sources/Notchling/Search/X.swift"
DART = "mobile/lib/features/chat/a.dart"
REPO = Path(__file__).resolve().parents[3]

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

# Final review I1: equivalents the first patterns missed. One example per pattern.
I1_BAD = [
    ("CAP-PROCESS", ZONE, "let p = Foundation.Process.launchedProcess(launchPath: path, arguments: args)"),
    ("CAP-PROCESS", ZONE, "typealias Job = Process"),
    ("CAP-PROCESS", ZONE, 'task.launchPath = "/bin/sh"'),
    ("CAP-PROCESS", ZONE, "try task.launch()"),
    ("CAP-PROCESS", DART, "final r = Process.runSync('ls', []);"),
    ("CAP-PROCESS", DART, "final p = await Process.start('ls', []);"),
    ("CAP-PROCESS", ZONE, 'let f = popen(cmd, "r")'),
    ("CAP-PROCESS", ZONE, "NSWorkspace.shared.open(url)"),
    ("CAP-PROCESS", ZONE, "NSWorkspace.shared.openApplication(at: appURL, configuration: cfg)"),
    ("CAP-PROCESS", ZONE, "let ws = NSWorkspace.shared"),
    ("CAP-DYNAMIC", ZONE, '@_silgen_name("posix_spawn")'),
    ("CAP-DYNAMIC", ZONE, '@_cdecl("entry")'),
    ("CAP-DYNAMIC", ZONE, 'let f = dlsym(nil, "SecItemCopyMatching")'),
    ("CAP-DYNAMIC", ZONE, 'let h = dlopen("/usr/lib/libx.dylib", RTLD_NOW)'),
    ("CAP-NETWORK", ZONE, "let d = try Data(contentsOf: URL(string: s)!)"),
    ("CAP-NETWORK", ZONE, "let s = try String(contentsOf: url, encoding: .utf8)"),
    ("CAP-NETWORK", ZONE, "let a = NSArray(contentsOf: url)"),
    ("CAP-NETWORK", DART, "final s = await Socket.connect(host, 80);"),
    ("CAP-NETWORK", DART, "final s = await HttpServer.bind(addr, 8080);"),
    ("CAP-FILEWRITE", ZONE, "try s.write(toFile: p, atomically: true, encoding: .utf8)"),
    ("CAP-FILEWRITE", ZONE, "let h = FileHandle(forWritingAtPath: p)"),
    ("CAP-FILEWRITE", ZONE, "let h = try FileHandle(forWritingTo: url)"),
    ("CAP-FILEWRITE", ZONE, "fm.createFile(atPath: p, contents: nil)"),
    ("CAP-FILEWRITE", ZONE, "try fm.removeItem(at: url)"),
    ("CAP-FILEWRITE", ZONE, "try fm.moveItem(at: a, to: b)"),
    ("CAP-FILEWRITE", ZONE, "try fm.copyItem(atPath: a, toPath: b)"),
]

# The app's own wrappers around the powerful APIs, from outside their own files.
WRAPPER_USES = [
    "TerminalSession(run: cmd, in: dir)",
    "AgentActions.stop(agent)",
    "CodexRunner.run(prompt, in: dir)",
    "AntigravityRunner.shared.prompt(text)",
    "SecretRunner.run(names, command)",
    "VaultServer.shared.start()",
]


def added(path, *lines):
    return Change(path, "M", None, tuple((i + 1, l) for i, l in enumerate(lines)))


class CapabilityTests(unittest.TestCase):
    def test_each_rule_fires_on_its_bad_example(self):
        for rule, path, line in BAD:
            self.assertIn(rule, {t.rule for t in scan([added(path, line)])}, f"{rule}: {line}")

    def test_every_rule_has_a_bad_example(self):
        self.assertEqual(set(RULES), {r for r, _, _ in BAD + I1_BAD})

    def test_good_examples_pass(self):
        for path, line in GOOD:
            self.assertEqual(scan([added(path, line)]), [], line)

    def test_comment_does_not_trip(self):
        self.assertEqual(scan([added("Sources/Notchling/Limits/LimitWatcher.swift", "// never osascript")]), [])

    def test_protected_type_in_own_file_ok(self):
        # Was ReplySenderQueue.swift, which only matched by name prefix (final review C3d).
        self.assertEqual(scan([added("Sources/Notchling/Agents/ReplySender.swift", "ReplySender.send(x)")]), [])

    def test_protected_type_elsewhere_trips(self):
        trips = scan([added("Sources/Notchling/Chat/Views/Row.swift", "ReplySender.send(x)")])
        self.assertEqual([t.rule for t in trips], ["CAP-PROTECTED-TYPE"])

    def test_removed_lines_ignored(self):
        self.assertEqual(scan([Change(ZONE, "M", None, (), ("let p = Process()",))]), [])

    def test_trip_has_location(self):
        [t] = scan([added(ZONE, "let a = 1", "let p = Process()")])
        self.assertEqual((t.path, t.line, t.text), (ZONE, 2, "let p = Process()"))


def rules_of(path, *lines):
    return {t.rule for t in scan([added(path, *lines)])}


class BinaryCodeTests(unittest.TestCase):
    """C2: git calls a file with a NUL byte binary and prints no lines for it, but swiftc compiles it."""
    PATH = "Sources/Notchling/Search/S.swift"

    def _changes(self, files, base_files=None):
        repo = make_repo({self.PATH: "import SwiftUI\nstruct S {}\n", **(base_files or {})})
        base = head_of(repo)
        head = commit(repo, files)
        return diff(repo, base, head)

    def test_nul_byte_swift_file_trips_process_and_binary(self):
        rules = {t.rule for t in scan(self._changes({self.PATH: b"import SwiftUI\n// \x00\nstruct S {}\nlet p = Process()\n"}))}
        self.assertIn("CAP-PROCESS", rules)
        self.assertIn("CAP-BINARY", rules)

    def test_binary_code_file_with_clean_lines_still_trips(self):
        trips = scan(self._changes({self.PATH: b"import SwiftUI\n// \x00\nstruct S {}\n"}))
        self.assertEqual([t.rule for t in trips], ["CAP-BINARY"])
        self.assertEqual((trips[0].path, trips[0].line), (self.PATH, 0))

    def test_gitattributes_nodiff_in_head_does_not_hide_process(self):
        changes = self._changes({"Sources/Notchling/Search/.gitattributes": "*.swift -diff\n",
                                 self.PATH: "import SwiftUI\nlet p = Process()\n"})
        self.assertIn("CAP-PROCESS", {t.rule for t in scan(changes)})

    def test_gitattributes_nodiff_in_base_does_not_hide_process(self):
        changes = self._changes({self.PATH: "import SwiftUI\nlet p = Process()\n"},
                                base_files={"Sources/Notchling/Search/.gitattributes": "*.swift -diff\n"})
        self.assertIn("CAP-PROCESS", {t.rule for t in scan(changes)})

    def test_binary_asset_does_not_trip(self):
        repo = make_repo({"mobile/test/goldens/a.png": b"\x89PNG\x00\x01"})
        base = head_of(repo)
        head = commit(repo, {"mobile/test/goldens/a.png": b"\x89PNG\x00\x02"})
        self.assertEqual(scan(diff(repo, base, head)), [])

    def test_binary_flag_on_a_non_code_file_does_not_trip(self):
        self.assertEqual(scan([Change("docs/a.md", "M", None, (), (), True)]), [])

    def test_binary_flag_trips_every_code_extension(self):
        for ext in (".swift", ".rs", ".dart", ".sh", ".py"):
            [t] = scan([Change("x/a" + ext, "M", None, (), (), True)])
            self.assertEqual(t.rule, "CAP-BINARY", ext)


class BareCarriageReturnTests(unittest.TestCase):
    """C3(a): a bare CR ends a Swift `//` comment, but git and the stripper see one line."""

    def test_code_after_cr_is_scanned(self):
        self.assertEqual([t.rule for t in scan([added(ZONE, "// note\rlet p = Process()")])], ["CAP-PROCESS"])

    def test_line_number_is_the_git_line(self):
        [t] = scan([added(ZONE, "let a = 1", "// note\rlet p = Process()", "let b = 2")])
        self.assertEqual(t.line, 2)

    def test_trip_text_is_the_offending_piece(self):
        [t] = scan([added(ZONE, "// note\rlet p = Process()")])
        self.assertEqual(t.text, "let p = Process()")

    def test_crlf_ending_is_not_a_second_line(self):
        self.assertEqual(scan([added(ZONE, "let a = 1\r")]), [])
        [t] = scan([added(ZONE, "let p = Process()\r")])
        self.assertEqual((t.line, t.text), (1, "let p = Process()"))

    def test_cr_through_a_real_diff(self):
        repo = make_repo({ZONE: "import SwiftUI\n"})
        base = head_of(repo)
        head = commit(repo, {ZONE: b"import SwiftUI\n// note\rlet p = Process()\n"})
        self.assertIn("CAP-PROCESS", {t.rule for t in scan(diff(repo, base, head))})

    def test_comment_only_pieces_do_not_trip(self):
        self.assertEqual(scan([added(ZONE, "// a\r// Process()\r// osascript")]), [])


class InterpolationTests(unittest.TestCase):
    """C3(b): code inside `\\( ... )` in a Swift string runs."""

    def test_process_inside_interpolation_trips(self):
        line = 'let r = "\\({ let p = Process(); try? p.run(); return 0 }())"'
        self.assertIn("CAP-PROCESS", rules_of(ZONE, line))

    def test_network_inside_interpolation_trips(self):
        self.assertIn("CAP-NETWORK", rules_of(ZONE, 'Text("a \\(URLSession.shared)")'))

    def test_nested_parens_in_interpolation(self):
        self.assertIn("CAP-PROCESS", rules_of(ZONE, 'let r = "\\(f(g(1), (2)))"; let p = Process()'))

    def test_string_inside_interpolation_hides_its_text(self):
        self.assertEqual(rules_of(ZONE, 'Text("\\(label("Process() )"))")'), set())

    def test_text_after_interpolation_is_still_text(self):
        self.assertEqual(rules_of(ZONE, 'Text("\\(count) Process() items")'), set())

    def test_plain_interpolation_does_not_trip(self):
        self.assertEqual(rules_of(ZONE, 'Text("\\(count) items, \\(name.uppercased())")'), set())

    def test_raw_string_interpolation_trips(self):
        self.assertIn("CAP-PROCESS", rules_of(ZONE, 'let r = #"\\#(Process())"#'))

    def test_raw_string_plain_backslash_paren_is_text(self):
        self.assertEqual(rules_of(ZONE, 'let r = #"\\(Process())"#'), set())

    def test_dart_interpolation_trips(self):
        self.assertIn("CAP-PROCESS", rules_of(DART, "print('${Process.runSync(\"ls\", [])}');"))


class SwiftQuoteTests(unittest.TestCase):
    """C3(c): only `"` delimits a Swift string; `'` is not a quote and backtick names are identifiers."""

    def test_raw_identifier_with_apostrophes_does_not_hide_code(self):
        self.assertIn("CAP-PROCESS", rules_of(ZONE, "let `a'` = 0; let p = Process(); let `b'` = 1"))

    def test_lone_apostrophes_do_not_hide_code(self):
        self.assertIn("CAP-PROCESS", rules_of(ZONE, "let a = x' ; let p = Process(); let b = y'"))

    def test_dart_single_quoted_string_still_hides_text(self):
        self.assertEqual(rules_of(DART, "final s = 'Process.run';"), set())

    def test_python_single_quoted_string_still_hides_text(self):
        self.assertEqual(rules_of("tools/x.py", "s = 'subprocess.run'"), set())

    def test_swift_string_still_hides_text(self):
        self.assertEqual(rules_of(ZONE, 'let s = "Process() and URLSession"'), set())

    def test_raw_string_hides_text_and_inner_quotes(self):
        self.assertEqual(rules_of(ZONE, 'let s = #"say "hi" Process() now"#'), set())

    def test_regex_literal_with_a_quote_does_not_hide_code(self):
        # Verified with swiftc: this runs the Process.
        self.assertIn("CAP-PROCESS", rules_of(ZONE, 'let r = #/"/#; let p = Process(); let s = #/"/#'))

    def test_code_after_raw_string_is_scanned(self):
        self.assertIn("CAP-PROCESS", rules_of(ZONE, 'let s = #"a"#; let p = Process()'))

    def test_code_after_closing_triple_quote_is_scanned(self):
        self.assertIn("CAP-PROCESS", rules_of(ZONE, '  """; let p = Process()'))

    def test_code_between_two_triple_quotes_on_one_line_is_scanned(self):
        self.assertIn("CAP-PROCESS", rules_of(ZONE, '  """; let p = Process(); let t = """'))


class BlockCommentStateTests(unittest.TestCase):
    """A line can start inside a block comment that an earlier line opened (the diff doesn't show
    it), and then `// */ code` or a quote before the `*/` is not what the stripper takes it for.
    Verified with swiftc: `/* ...` then `// */ let p = Process()` runs the Process."""

    def test_line_comment_marker_before_the_closing_star_slash(self):
        trips = scan([added(ZONE, "/* an open comment", "// */ let p = Process()")])
        self.assertEqual([(t.rule, t.line) for t in trips], [("CAP-PROCESS", 2)])

    def test_quote_before_the_closing_star_slash(self):
        self.assertIn("CAP-PROCESS", rules_of(ZONE, '" */ let p = Process(); let q = "x"'))

    def test_second_star_slash_on_the_line(self):
        self.assertIn("CAP-PROCESS", rules_of(ZONE, '/* a */ // b */ let p = Process()'))

    def test_dart_and_rust_too(self):
        self.assertIn("CAP-PROCESS", rules_of(DART, "// */ final r = Process.runSync('ls', []);"))
        self.assertIn("CAP-PROCESS", rules_of("crates/app/src/a.rs", "// */ let c = std::process::Command::new(a);"))

    def test_ordinary_comments_do_not_trip(self):
        for line in ("let a = 1 /* Process() */ // done",
                     "/* Process() */",
                     "// see */ the docs",
                     'let glob = "Sources/**/*.swift"',
                     "*/"):
            with self.subTest(line):
                self.assertEqual(rules_of(ZONE, line), set())

    def test_many_star_slashes_are_not_read_forever(self):
        self.assertEqual(rules_of(ZONE, "*/ " * 5000 + "let a = 1"), {"CAP-OBSCURE"})

    def test_shell_and_python_have_no_block_comments(self):
        self.assertEqual(rules_of("tools/x.sh", "ls */ # Process"), set())


class OwnFileTests(unittest.TestCase):
    """C3(d): a protected type's own files are a fixed list of exact paths."""

    def test_lookalike_names_are_not_own_files(self):
        for path, line in (("Sources/Notchling/Chat/Views/VaultStoreRow.swift", "VaultStore.shared.values()"),
                           ("Sources/Notchling/Chat/Views/ReplySenderButton.swift", "ReplySender.send(x, to: a)"),
                           ("Sources/Notchling/Agents/ReplySenderQueue.swift", "ReplySender.send(x)"),
                           ("Sources/Notchling/Search/AccountStoreView.swift", "AccountStore.shared.accounts")):
            self.assertEqual(rules_of(path, line), {"CAP-PROTECTED-TYPE"}, path)

    def test_no_own_file_is_in_the_zone(self):
        zones = load_zones(REPO / "tools/guard/zones.json")
        for type_name, paths in OWN_FILES.items():
            for path in paths:
                self.assertFalse(zones.in_zone(path), f"{type_name}: {path} is in the auto-merge zone")

    def test_own_file_by_exact_path_is_ok(self):
        for type_name, paths in OWN_FILES.items():
            for path in paths:
                self.assertEqual(rules_of(path, f"{type_name}.shared.use()"), set(), f"{type_name} in {path}")

    def test_own_file_of_another_type_is_not_ok(self):
        self.assertEqual(rules_of("Sources/Notchling/Vault/VaultStore.swift", "ReplySender.send(x)"),
                         {"CAP-PROTECTED-TYPE"})

    def test_every_protected_type_has_own_files(self):
        self.assertEqual(set(OWN_FILES), set(PROTECTED_TYPES))

    def test_own_files_exist_and_define_the_type(self):
        for type_name, paths in OWN_FILES.items():
            text = ""
            for path in paths:
                self.assertTrue((REPO / path).is_file(), f"{type_name}: {path} does not exist")
                text += (REPO / path).read_text(encoding="utf-8")
            self.assertTrue(re.search(r"\b(class|struct|enum|actor|protocol)\s+%s\b" % type_name, text),
                            f"{type_name} is not defined in {paths}")


class BroaderPatternTests(unittest.TestCase):
    """I1: equivalents of the first patterns."""

    def test_each_equivalent_trips_its_rule(self):
        for rule, path, line in I1_BAD:
            with self.subTest(line):
                self.assertIn(rule, rules_of(path, line))

    def test_app_wrappers_trip_from_outside_their_files(self):
        for line in WRAPPER_USES:
            with self.subTest(line):
                self.assertEqual(rules_of("Sources/Notchling/Chat/Views/Row.swift", line), {"CAP-PROTECTED-TYPE"})

    def test_app_wrappers_are_protected_types(self):
        for name in ("TerminalSession", "AgentActions", "CodexRunner", "AntigravityRunner", "SecretRunner",
                     "VaultServer"):
            self.assertIn(name, PROTECTED_TYPES)

    def test_app_wrappers_are_ok_in_their_own_files(self):
        for path, line in (("Sources/Notchling/Terminal/TerminalSession.swift", "TerminalSession(run: c, in: d)"),
                           ("Sources/Notchling/Agents/AgentActions.swift", "AgentActions.stop(a)"),
                           ("Sources/Notchling/Agents/CodexRunner.swift", "CodexRunner.run(p, in: d)"),
                           ("Sources/Notchling/Agents/AntigravityCLI.swift", "AntigravityRunner.shared.prompt(t)"),
                           ("Sources/Notchling/Vault/SecretVault.swift", "SecretRunner.run(n, c)"),
                           ("Sources/Notchling/Vault/VaultServer.swift", "VaultServer.shared.start()")):
            self.assertEqual(rules_of(path, line), set(), line)

    def test_look_alikes_that_are_not_the_api_do_not_trip(self):
        for line in ("let env = ProcessInfo.processInfo.environment",
                     "items.append(contentsOf: more)",
                     "items.insert(contentsOf: more, at: 0)",
                     'Text("Process NSWorkspace dlsym popen")',
                     "let app = NSWorkspace.shared.frontmostApplication",
                     "NSWorkspace.shared.activateFileViewerSelecting([url])",
                     "let calm = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion",
                     "obs = NSWorkspace.shared.notificationCenter.addObserver(forName: n, object: nil, queue: .main) { _ in }",
                     "Image(nsImage: NSWorkspace.shared.icon(forFile: path))",
                     "// launchPath, createFile and toFile: are only mentioned here",
                     "let processes = running.filter { $0.isActive }"):
            with self.subTest(line):
                self.assertEqual(rules_of(ZONE, line), set())


if __name__ == "__main__":
    unittest.main()
