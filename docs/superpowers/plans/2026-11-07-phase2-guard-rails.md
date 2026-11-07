# Phase 2: Guard Rails and Quality Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the repository refuse, by automatic checks, any change that the design says needs the maintainer. Agents can then be turned on in Phase 5 without being able to loosen their own guard rails.

**Architecture:**
- **Checks:** one small Python package, `tools/guard/`, standard library only. It reads a git diff and answers four questions:
  - Is any file outside the auto-merge zone?
  - Does any added line trip a capability?
  - Does the change weaken a test?
  - Does the lint find a new violation?
- **Workflows:** GitHub Actions run it on every pull request.
- **`maintainer-gate`:** posts one commit status on the PR's head commit. It passes only when nothing trips, or when `@dtthaison` approved that exact head commit. It runs from `main`'s copy of the code, never the PR's.
- **GitHub settings:** rulesets, CODEOWNERS, Dependabot and security settings live in the repo as files. The maintainer applies them with a script; agents never apply them.

**Tech Stack:**
- Python 3.12+ (stdlib only; it's on every GitHub runner and on macOS with Xcode's tools)
- GitHub Actions, rulesets API
- Flutter (the phone checks)
- Swift (the existing tests)

**Spec:** `docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md`, revision 2. Sections:
- "The line"
- "The flow" step 4 (Gates)
- "GitHub setup"
- "Quality: the first rules"
- "Constraints"
- "Phases" (Phase 2)

Decision record 0001 (GPUI spike) is on PR #19.

## Global Constraints

- **Ownership:** `CODEOWNERS` starts with `* @dtthaison`. Only the auto-merge zone is listed without an owner.
- **The zone:** the auto-merge zone is exactly the spec's list under "The line" (Task 2 copies it). New files under `Tests/`, `Fixtures/` and `mobile/test/` are in the zone. Changes to existing files there are not.
- **Capability patterns:** the capability check flags new uses of:
  - `Process`, `forkpty`/`posix_spawn`
  - `NSAppleScript`/`osascript`
  - `URLSession`/network clients
  - `SecItem*`/`/usr/bin/security`/`keyring`
  - `FileManager` writes
  - the protected types `VaultStore`, `AccountStore`, `LaneStore`, `ReplySender`, `HookBridge`, `OwnerPublish` from outside their own files
- **Labels:** labels are informational only and never decide a gate.
- **Test weakening:** removed or edited existing tests, new `XCTSkip`/`#[ignore]`, edited goldens → maintainer. Invisible or bidirectional Unicode is rejected.
- **Lint baseline:** the lint fails only on violations not in its generated baseline. The baseline may only shrink.
- **File size:** warn over 600 lines, fail over 1,000 for new files.
- **Every rule has an automatic check:** it fails on its own bad example, and its failure names the rule id.
- **No secrets in code jobs:** CI jobs that run repository code have no secrets. Tests never touch Keychain, real logins, ssh or `gh`.
- **Actions:** only GitHub's and allowlisted actions, each pinned to a full commit SHA.
- **Required checks:** pinned to the GitHub Actions source (`integration_id` 15368). There's no merge queue on a user-owned repo, so "require up to date" stays off; binding to the head commit does that job.
- **Dependabot:** for Swift, pub (`mobile/pubspec.lock`) and Actions, with a cooldown. Cargo joins when `Cargo.toml` exists (Phase 4). Lockfile updates never auto-merge.
- **Live settings:** no change to live GitHub settings until the maintainer runs the apply script (Task 10 asks first).
- **Scope:** don't change sd, the worker, the GitHub App or releases. Those are Phases 5 and 6.

## Review Focus

- **A PR that edits `tools/guard/` to make itself pass.** `maintainer-gate` must run `main`'s copy of the code, never the PR's. Pinned by Task 9's workflow test `test_gate_workflow_checks_out_base_only`.
- **Renamed files.** A protected file moved into a zone folder is still a protected change, and a renamed test counts as a test edit. Pinned by `test_rename_out_of_protected_is_protected` (Task 2) and `test_renamed_test_is_weakening` (Task 4).
- **An approval on an older commit.** After a new push, an earlier approval no longer satisfies the gate. Neither does a later "changes requested", or an approval from anyone but `@dtthaison`. Pinned by `test_decide_*` in Task 6.
- **Binary files, deleted files and huge diffs.** The diff reader must not crash on binary changes and must still list them as touched paths. Pinned by `test_binary_change_listed`.
- **Patterns in comments and strings.** `osascript` in a comment must not trip the capability check, while `Process()` on a code line must. Pinned by `test_comment_does_not_trip` (Task 3). The lint shares the same comment stripper.

---

## File Structure

```
tools/guard/
  guard/__init__.py
  guard/__main__.py      CLI: python3 -m guard {gate,lint,codeowners,pins,rulesets}
  guard/changes.py       git diff → Change list (Task 1)
  guard/source.py        comment/string stripping for Swift, Rust, Dart (Task 1)
  guard/zones.py         auto-merge zone, protected paths (Task 2)
  guard/codeowners.py    CODEOWNERS render/check (Task 2)
  guard/capability.py    capability trips (Task 3)
  guard/weakening.py     test-weakening and Unicode trips (Task 4)
  guard/lint.py          lint rules, registry, baseline (Task 5)
  guard/gate.py          maintainer-gate decision and GitHub calls (Task 6)
  guard/pins.py          actions pinned and allowlisted (Task 7)
  guard/rulesets.py      ruleset/workflow consistency (Task 10)
  zones.json             the auto-merge zone
  actions-allowlist.json
  lint-baseline.json     generated
  tests/test_*.py        unittest, one file per module; fixtures built in temp git repos
.github/CODEOWNERS                 generated by `python3 -m guard codeowners --write`
.github/workflows/guard.yml        guard unit tests, lint, pins, codeowners, rulesets (Task 9)
.github/workflows/maintainer-gate.yml  pull_request_target + workflow_run (Task 9)
.github/workflows/review-signal.yml    pull_request_review, no permissions (Task 9)
.github/workflows/build.yml        existing `build` job (swift test) + `app-build` (./build.sh) (Task 9)
.github/workflows/phone.yml        flutter analyze/test/goldens (Task 8)
.github/rulesets/main.json, agent-branches.json, release-tags.json (Task 10)
.github/dependabot.yml             (Task 10)
tools/github/apply-settings.sh     the maintainer runs it (Task 10)
mobile/AGENTS.md, mobile/test/goldens/ (Task 8)
```

Every test runs with: `python3 -m unittest discover -s tools/guard/tests -t tools/guard`, from the repo root. `tools/guard` goes on `PYTHONPATH` through the test files' `sys.path` setup in `tests/__init__.py`.

---

### Task 1: The diff reader and the source stripper

**Files:**
- Create: `tools/guard/guard/__init__.py`, `guard/changes.py`, `guard/source.py`, `tools/guard/tests/__init__.py`, `tests/helpers.py`, `tests/test_changes.py`, `tests/test_source.py`

**Interfaces:**
- Produces:
  - `@dataclass(frozen=True) class Change`, with fields:
    - `path: str`
    - `status: str` (one of `A`, `M`, `D`, `R`)
    - `old_path: str | None`
    - `added: tuple[tuple[int, str], ...]` (new line number, text)
    - `removed: tuple[str, ...]`
    - `binary: bool`
  - `def diff(repo: Path, base: str, head: str) -> list[Change]`
    - runs `git -C repo diff -M --unified=0 --no-color --no-ext-diff base...head`;
    - three dots, so only the PR's own changes count.
  - `def strip_code(path: str, line: str) -> str`
    - returns the line with `//` and `#` comments (Swift, Rust, Dart, shell) and string literal contents blanked;
    - a block comment inside one line is blanked too;
    - files whose language it doesn't know come back unchanged.
  - `tests/helpers.py`: `make_repo(files: dict[str, str|bytes]) -> Path` and `commit(repo, files, message) -> str`. Tests build real temporary git repos with these.

- [ ] **Step 1: Write the failing tests** in `test_changes.py` and `test_source.py`:

```python
def test_added_lines_and_numbers(self):
    repo = make_repo({"a.swift": "one\n"}); base = head_of(repo)
    head = commit(repo, {"a.swift": "one\ntwo\n"}, "add")
    [c] = diff(repo, base, head)
    self.assertEqual((c.path, c.status, c.added), ("a.swift", "M", ((2, "two"),)))

def test_rename_keeps_old_path(self):  # git mv Sources/x.swift Sources/Notchling/Search/x.swift
    ...; self.assertEqual((c.status, c.old_path), ("R", "Sources/x.swift"))

def test_binary_change_listed(self):
    ...; self.assertTrue(c.binary); self.assertEqual(c.added, ())

def test_deleted_file(self):
    ...; self.assertEqual(c.status, "D")

def test_strip_line_comment(self):
    self.assertEqual(strip_code("x.swift", 'let p = Process() // osascript').rstrip(), "let p = Process()")

def test_strip_string(self):
    self.assertNotIn("osascript", strip_code("x.swift", 'let s = "osascript"'))

def test_unknown_language_unchanged(self):
    self.assertEqual(strip_code("a.md", "// keep"), "// keep")
```

- [ ] **Step 2:** Run `python3 -m unittest discover -s tools/guard/tests -t tools/guard`. Expected: FAIL (`ModuleNotFoundError: guard.changes`).
- [ ] **Step 3:** Implement `changes.py` and `source.py`.
  - In `changes.py`, parse the `diff --git`, `rename from/to`, `Binary files` and `@@ -a,b +c,d @@` headers.
  - In `source.py`, use a small character scanner: no regex across quotes, and handle escaped quotes.
- [ ] **Step 4:** Run the tests. Expected: PASS.
- [ ] **Step 5:** Commit `git add tools/guard && git commit -m "guard: diff reader and source stripper"`.

### Task 2: The auto-merge zone and CODEOWNERS

**Files:**
- Create: `tools/guard/zones.json`, `guard/zones.py`, `guard/codeowners.py`, `guard/__main__.py` (the `codeowners` subcommand), `.github/CODEOWNERS`, `tests/test_zones.py`, `tests/test_codeowners.py`

**Interfaces:**
- Consumes: `Change`, `diff` (Task 1).
- Produces:
  - `zones.json`, shaped `{"auto": [glob, ...], "new_files_only": [glob, ...]}`.
    - `auto` is the spec's zone list, written as globs. The views-only Notch list is spelled out file by file:
      - `Sources/Notchling/Notch/NotchView.swift`
      - `NotchRows.swift`
      - `NotchPeek.swift`
      - `NotchDetail.swift`
      - `NotchCardParts.swift`
      - `TabPerch.swift`
      - `Pet/**`
    - Under `docs/**`, these are excluded with `!` entries:
      - `docs/goals.md`
      - `docs/rules.md`
      - `docs/decisions/**`
    - The Rust paths (`crates/app/**`, `crates/kit/**`) are included now, so they're right when Phase 4 creates them.
    - `new_files_only` is `["Tests/**", "Fixtures/**", "mobile/test/**"]`.
  - `def load(path: Path) -> Zones`
  - `Zones.in_zone(path: str) -> bool`
  - `def protected_paths(changes: list[Change], zones: Zones) -> list[str]`, which returns every touched path outside the zone:
    - for a rename, both `old_path` and `path` must be in the zone;
    - a `new_files_only` path counts as in the zone only when its status is `A`.
  - `def render(zones: Zones) -> str` (CODEOWNERS text)
  - CLI `python3 -m guard codeowners [--write|--check]`. `--check` exits 1 with `GUARD-CODEOWNERS: .github/CODEOWNERS is out of date` when the file differs from `render`.

- [ ] **Step 1: Write the failing tests:**

```python
def test_view_in_zone(self):           self.assertTrue(Z.in_zone("Sources/Notchling/Chat/Views/ChatRow.swift"))
def test_model_protected(self):        self.assertFalse(Z.in_zone("Sources/Notchling/Chat/Model/ChatModel.swift"))
def test_notch_controller_protected(self): self.assertFalse(Z.in_zone("Sources/Notchling/Notch/NotchController.swift"))
def test_docs_rules_protected(self):   self.assertFalse(Z.in_zone("docs/rules.md"))
def test_new_test_ok_edit_not(self):
    self.assertEqual(protected_paths([chg("Tests/NotchlingTests/NewTests.swift", "A")], Z), [])
    self.assertEqual(protected_paths([chg("Tests/NotchlingTests/ChatTests.swift", "M")], Z), ["Tests/NotchlingTests/ChatTests.swift"])
def test_rename_out_of_protected_is_protected(self):
    self.assertEqual(protected_paths([chg("Sources/Notchling/Search/X.swift", "R", old="Sources/Notchling/Vault/X.swift")], Z),
                     ["Sources/Notchling/Vault/X.swift"])
def test_codeowners_starts_with_owner(self):
    self.assertTrue(render(Z).splitlines()[0 if not render(Z).startswith("#") else 2].startswith("* @dtthaison"))
def test_codeowners_check_fails_on_stale(self):  # write a stale file in a temp repo, run main(["codeowners","--check"]) → 1
```

- [ ] **Step 2:** Run the tests. Expected: FAIL (module missing).
- [ ] **Step 3:** Implement `zones.py`.
  - Match paths with `fnmatch`. `**` matches across `/`.
  - Later entries win; `!` negates.
- [ ] **Step 4:** Implement `codeowners.py`.
  - Line 1 is a generated-file comment, then `* @dtthaison`, then one line per `auto` glob with no owner.
  - Each `!` exclusion is re-owned to `@dtthaison` after the zone lines, because CODEOWNERS uses the last match.
  - Then run `python3 -m guard codeowners --write`.
- [ ] **Step 5:** Run the tests and `python3 -m guard codeowners --check`. Expected: PASS, exit 0.
- [ ] **Step 6:** Commit `"guard: auto-merge zone and generated CODEOWNERS"`.

### Task 3: The capability check

**Files:**
- Create: `guard/capability.py`, `tests/test_capability.py`

**Interfaces:**
- Consumes: `Change`, `strip_code`.
- Produces:
  - `@dataclass(frozen=True) class Trip`, with fields `rule: str`, `path: str`, `line: int`, `text: str`. Shared with Tasks 4–6.
  - `def scan(changes: list[Change]) -> list[Trip]`, which looks only at added lines, after `strip_code`, in `.swift`, `.rs`, `.dart`, `.sh` and `.py` files.

The rules, each a regex on the stripped line:

| id | pattern |
|---|---|
| `CAP-PROCESS` | `\bProcess\s*\(`, `\.executableURL\b`, `std::process::Command`, `Process\.run` |
| `CAP-PTY` | `\bforkpty\b`, `\bposix_spawn\w*\b`, `portable_pty`, `\bopenpty\b` |
| `CAP-APPLESCRIPT` | `\bNSAppleScript\b`, `\bosascript\b` |
| `CAP-NETWORK` | `\bURLSession\b`, `\bNWConnection\b`, `\breqwest\b`, `\bHttpClient\b`, `package:http/` |
| `CAP-KEYCHAIN` | `\bSecItem\w+`, `/usr/bin/security`, `\bkeyring\b` |
| `CAP-FILEWRITE` | `FileManager[^\n]*\.(createFile|createDirectory|moveItem|copyItem|removeItem|replaceItem|setAttributes)`, `\.write\(to:`, `\bstd::fs::(write|remove|rename|create_dir)` |
| `CAP-PROTECTED-TYPE` | `\b(VaultStore|AccountStore|LaneStore|ReplySender|HookBridge|OwnerPublish)\b` used in a file whose name doesn't start with that type's name |

- [ ] **Step 1: Write the failing tests.** There is one bad example and one good example per rule, written as test cases that look up the rule by id:

```python
def test_each_rule_fires_on_its_bad_example(self):
    for rule, path, line in BAD:   # e.g. ("CAP-PROCESS", "Sources/Notchling/Search/X.swift", "let p = Process()")
        self.assertIn(rule, {t.rule for t in scan([added(path, line)])}, rule)
def test_comment_does_not_trip(self):
    self.assertEqual(scan([added("Sources/Notchling/Limits/LimitWatcher.swift", "// never osascript")]), [])
def test_protected_type_in_own_file_ok(self):
    self.assertEqual(scan([added("Sources/Notchling/Agents/ReplySenderQueue.swift", "ReplySender.send(x)")]), [])
def test_protected_type_elsewhere_trips(self):
    self.assertEqual([t.rule for t in scan([added("Sources/Notchling/Chat/Views/Row.swift", "ReplySender.send(x)")])], ["CAP-PROTECTED-TYPE"])
def test_removed_lines_ignored(self): ...
```

- [ ] **Step 2:** Run the tests. Expected: FAIL.
- [ ] **Step 3:** Implement `scan`. Rule ids live in the module's `RULES: dict[str, list[re.Pattern]]`.
- [ ] **Step 4:** Run the tests. Expected: PASS.
- [ ] **Step 5:** Commit `"guard: capability check"`.

### Task 4: The test-weakening check

**Files:**
- Create: `guard/weakening.py`, `tests/test_weakening.py`

**Interfaces:**
- Consumes: `Change`, `Trip`.
- Produces: `def scan(changes: list[Change]) -> list[Trip]`, with these rules:

| id | When it trips |
|---|---|
| `WEAK-TEST-EDIT` | status `M`, `D` or `R` on a path under `Tests/`, `Fixtures/`, `mobile/test/`, `tools/guard/tests/`, any `tests/` folder in a Rust crate, or a `*.png` under any `goldens/` folder; line 0 |
| `WEAK-SKIP` | an added line matching `\bXCTSkip\b`, `#\[ignore\]`, `\bskip:\s*(true\|['"])`, `@Skip\(` |
| `WEAK-UNICODE` | an added line in any text file containing a code point in U+200B–U+200F, U+202A–U+202E, U+2060–U+2064, U+2066–U+2069 or U+FEFF |

- [ ] **Step 1: Write the failing tests:**

```python
def test_edited_test_trips(self):          rules(chg("Tests/NotchlingTests/ChatTests.swift", "M")) == ["WEAK-TEST-EDIT"]
def test_new_test_ok(self):                rules(chg("Tests/NotchlingTests/New.swift", "A", added=["func testX() {}"])) == []
def test_renamed_test_is_weakening(self):  rules(chg("Tests/A/B.swift", "R", old="Tests/A/A.swift")) == ["WEAK-TEST-EDIT"]
def test_golden_edit_trips(self):          rules(chg("mobile/test/goldens/kit.png", "M", binary=True)) == ["WEAK-TEST-EDIT"]
def test_xctskip_trips(self):              rules(chg("Tests/X/New.swift", "A", added=["throw XCTSkip(\"later\")"])) == ["WEAK-SKIP"]
def test_rust_ignore_trips(self):          rules(chg("crates/engine/src/a.rs", "M", added=["#[ignore]"])) == ["WEAK-SKIP"]
def test_bidi_rejected(self):              rules(chg("Sources/Notchling/Search/S.swift", "M", added=["let a = 1 ‮"])) == ["WEAK-UNICODE"]
def test_plain_text_ok(self):              rules(chg("README.md", "M", added=["Việt Nam"])) == []
```

- [ ] **Step 2:** Run the tests. Expected: FAIL.
- [ ] **Step 3:** Implement `scan`.
- [ ] **Step 4:** Run the tests. Expected: PASS.
- [ ] **Step 5:** Commit `"guard: test-weakening and invisible Unicode check"`.

The coverage ratchet waits until a coverage job exists. Ruling: there is no coverage measurement today; the ratchet arrives with the Rust workspace in Phase 4.

### Task 5: The architecture lint and its baseline

**Files:**
- Create: `guard/lint.py`, `tools/guard/lint-baseline.json` (generated), `tests/test_lint.py`
- Modify: `guard/__main__.py` (the `lint` subcommand)

**Interfaces:**
- Consumes: `strip_code`.
- Produces:
  - `@dataclass(frozen=True) class Rule`, with fields:
    - `id: str`
    - `why: str`
    - `paths: tuple[str, ...]` (globs it applies to)
    - `exclude: tuple[str, ...]`
    - `check: Callable[[str, list[str], Path], list[tuple[int, str]]]`
    - `bad: tuple[str, str]` (path, file text)
    - `good: tuple[str, str]`
  - `RULES: list[Rule]`
  - `@dataclass(frozen=True) class Violation`, with fields `rule: str`, `path: str`, `line: int`, `text: str`.
  - `def run(repo: Path, paths: list[str] | None = None) -> list[Violation]`, where `None` means every tracked file.
  - `def fingerprint(v: Violation) -> str`, which is `f"{v.rule}|{v.path}|{' '.join(v.text.split())}"`.
  - `def new_violations(found: list[Violation], baseline: list[str]) -> list[Violation]`. The comparison is a multiset: two identical lines need two entries.
  - CLI `python3 -m guard lint`:
    - prints each new violation as `path:line: RULE-ID why`;
    - exits 1 on any new error;
    - warnings print and exit 0.
  - CLI `python3 -m guard lint --update-baseline` rewrites the baseline only by removing entries. It exits 1 with `GUARD-BASELINE: the baseline can only shrink` if the new run has violations the old baseline lacks.

Rules. Probes and tests are excluded from all rules: `Tests/**`, `Demo/**`, `tools/**`, `mobile/test/**`.

| id | Paths | Check |
|---|---|---|
| `LINT-CORE-NO-NOTCHLING` | `Sources/ConsoleCore/**` | `import Notchling` or `@testable import Notchling` |
| `LINT-CORE-MACOS13` | `Sources/ConsoleCore/**` | `@Observable`, `#available(macOS 1[4-9]`, `@available(macOS 1[4-9]` |
| `LINT-NO-FORCE-UNWRAP` | `Sources/**/*.swift` | stripped line matches `[\w\)\]]!(?![=])` or `\b(try\|as)!` |
| `LINT-KIT-TOKENS` | `Sources/Notchling/Chat/Views/**`, the Notch view files from Task 2 | `Color\((red\|white\|hue\|\.sRGB)`, `\.font\(\.system\(size:`, `Color\.(red\|green\|blue\|orange\|yellow\|purple\|pink\|gray\|black\|white)\b` |
| `LINT-DEFAULTS-MIGRATED` | `Sources/Notchling/**` | each key in `forKey: "x"` or `@AppStorage("x")` must be in the array literal in `AppDelegate.migrateFromAwake`, read from `Sources/Notchling/App/AppDelegate.swift`. `migratedFromAwake` itself is exempt. |
| `LINT-POINTER-CURSOR` | `Sources/Notchling/**` | a line with `.onTapGesture` and no `.pointerCursor()` within the same line or the next 3 lines |
| `LINT-NO-APPLESCRIPT` | `Sources/**`, `crates/**` | `NSAppleScript` or `osascript` on a stripped line |
| `LINT-FILE-SIZE` | `Sources/**`, `crates/**`, `mobile/lib/**` | warn over 600 lines; error over 1,000 for a file not in the baseline |

- [ ] **Step 1: Write the failing tests:**

```python
def test_every_rule_fails_on_its_bad_example(self):
    for r in RULES:
        self.assertTrue(r.check(r.bad[0], r.bad[1].splitlines(), FIXTURE_REPO), r.id)
def test_every_rule_passes_its_good_example(self):
    for r in RULES:
        self.assertEqual(r.check(r.good[0], r.good[1].splitlines(), FIXTURE_REPO), [], r.id)
def test_rule_ids_unique_and_prefixed(self): ...
def test_force_unwrap_not_inequality(self):
    self.assertEqual(rule("LINT-NO-FORCE-UNWRAP").check("Sources/A.swift", ["if a != b {}", "if !ok {}"], R), [])
def test_baseline_absorbs_existing(self):
    v = Violation("LINT-NO-APPLESCRIPT", "Sources/Notchling/Agents/AgentActions.swift", 98, "NSAppleScript(source: s)")
    self.assertEqual(new_violations([v], [fingerprint(v)]), [])
def test_baseline_counts_duplicates(self):  two equal violations, one baseline entry → one new
def test_baseline_cannot_grow(self):        main(["lint","--update-baseline"]) on a temp repo with a new violation → exit 1, message GUARD-BASELINE
```

- [ ] **Step 2:** Run the tests. Expected: FAIL.
- [ ] **Step 3:** Implement `lint.py` and the CLI.
- [ ] **Step 4:** Run the tests. Expected: PASS.
- [ ] **Step 5:** Generate the baseline from the current tree: `python3 -m guard lint --write-baseline`. This hidden first-run flag writes only when no baseline file exists.
  - Record the count per rule in the commit message.
  - Expected: `LINT-NO-APPLESCRIPT` includes `Agents/AgentActions.swift`.
  - Expected: `LINT-DEFAULTS-MIGRATED` lists the keys missing from the migration list (the spec counted 30 keys in code and 24 migrated).
- [ ] **Step 6:** Run `python3 -m guard lint`. Expected: exit 0, no new violations.
- [ ] **Step 7:** Commit `"guard: architecture lint with a shrink-only baseline"`.

### Task 6: The `maintainer-gate` decision

**Files:**
- Create: `guard/gate.py`, `tests/test_gate.py`
- Modify: `guard/__main__.py` (the `gate` subcommand)

**Interfaces:**
- Consumes:
  - `protected_paths` and `Zones` (Task 2);
  - `capability.scan` (Task 3);
  - `weakening.scan` (Task 4);
  - `Trip`.
- Produces:
  - `MAINTAINERS = frozenset({"dtthaison"})`
  - `@dataclass(frozen=True) class Decision`, with fields `passed: bool`, `summary: str` (at most 140 chars, for the status description), `details: list[str]`.
  - `def needs_maintainer(changes: list[Change], zones: Zones) -> list[str]`: one line per reason, each prefixed with its rule id, or `ZONE:` for a protected path.
  - `def approved_head(reviews: list[dict], head_sha: str, maintainers: frozenset[str]) -> bool`
    - `reviews` is the GitHub REST list-reviews shape: `user.login`, `state`, `commit_id`, `submitted_at`.
    - It looks at each maintainer's latest review that is `APPROVED` or `CHANGES_REQUESTED` (`COMMENTED` is ignored).
    - It is true when any maintainer's latest review is `APPROVED` with `commit_id == head_sha`.
  - `def decide(reasons: list[str], reviews: list[dict], head_sha: str) -> Decision`
    - no reasons → passed, "Nothing protected";
    - reasons with approval → passed, "Approved by the maintainer at <sha7>";
    - otherwise → failed, "Needs the maintainer: <first reason> (+N more)".
  - CLI `python3 -m guard gate --repo . --base <sha> --head <sha> [--reviews reviews.json]`:
    - prints the details;
    - exits 0 when passed and 1 when not.
    - With `--post`, it posts the commit status `maintainer-gate` through `gh api repos/{owner}/{repo}/statuses/{head}`. Only the workflow uses `--post`; tests never call `gh`.

- [ ] **Step 1: Write the failing tests:**

```python
def test_decide_nothing_protected(self):   self.assertTrue(decide([], [], H).passed)
def test_decide_needs_maintainer(self):    self.assertFalse(decide(["ZONE: build.sh"], [], H).passed)
def test_decide_approved_head(self):       self.assertTrue(decide(R, [rev("dtthaison","APPROVED",H)], H).passed)
def test_decide_old_approval(self):        self.assertFalse(decide(R, [rev("dtthaison","APPROVED","old")], H).passed)
def test_decide_later_changes_requested(self):
    self.assertFalse(decide(R, [rev("dtthaison","APPROVED",H,t=1), rev("dtthaison","CHANGES_REQUESTED",H,t=2)], H).passed)
def test_decide_comment_after_approval_keeps_it(self):
    self.assertTrue(decide(R, [rev("dtthaison","APPROVED",H,t=1), rev("dtthaison","COMMENTED",H,t=2)], H).passed)
def test_decide_other_approver(self):      self.assertFalse(decide(R, [rev("TrietKun","APPROVED",H)], H).passed)
def test_needs_maintainer_combines(self):  # a temp repo where one commit edits build.sh and adds Process() in a zone file
    self.assertEqual(sorted(r.split(":")[0] for r in needs_maintainer(changes, Z)), ["CAP-PROCESS", "ZONE"])
def test_summary_fits_status(self):        self.assertLessEqual(len(decide(["ZONE: " + "x"*300], [], H).summary), 140)
```

- [ ] **Step 2:** Run the tests. Expected: FAIL.
- [ ] **Step 3:** Implement `gate.py` and the CLI.
- [ ] **Step 4:** Run the tests. Expected: PASS.
- [ ] **Step 5:** Dry-run on real history: `python3 -m guard gate --repo . --base cd7aa66~1 --head cd7aa66`. That is PR #18, which changes Chat and is mostly protected Model code, so expect exit 1 with `ZONE:` lines. Paste the output into the commit message.
- [ ] **Step 6:** Commit `"guard: maintainer-gate decision"`.

### Task 7: Pinned and allowlisted actions

**Files:**
- Create: `guard/pins.py`, `tools/guard/actions-allowlist.json`, `tests/test_pins.py`
- Modify: `guard/__main__.py` (the `pins` subcommand); every file in `.github/workflows/` (pin `actions/checkout@v7` and any other `uses:` to a SHA, with the tag as a trailing comment)

**Interfaces:**
- Produces:
  - `def check(workflows_dir: Path, allowlist: set[str]) -> list[str]`. Problems are prefixed `GUARD-PINS:`:
    - a `uses:` not `owner/repo[/path]@<40 hex>`;
    - an owner/repo not in the allowlist;
    - a local `./` action, which is allowed.
  - The allowlist starts as `["actions/checkout", "actions/setup-python", "subosito/flutter-action"]`.
  - CLI `python3 -m guard pins`.

- [ ] **Step 1: Write the failing tests:** a tag-pinned `uses` fails; a SHA-pinned allowlisted one passes; a SHA-pinned unknown owner fails; a docker `uses` fails.
- [ ] **Step 2:** Run the tests. Expected: FAIL.
- [ ] **Step 3:** Implement `pins.py`.
  - Find the SHA for each tag with `git ls-remote https://github.com/<owner>/<repo> refs/tags/<tag>`, taking the peeled `^{}` line when present.
  - Rewrite the `uses:` lines as `owner/repo@<sha> # <tag>`.
- [ ] **Step 4:** Run the tests and `python3 -m guard pins`. Expected: PASS, exit 0.
- [ ] **Step 5:** Commit `"guard: actions pinned by SHA and allowlisted"`.

### Task 8: Phone checks (P0)

**Files:**
- Create: `mobile/test/goldens/kit_golden_test.dart`, `mobile/test/goldens/*.png` (generated), `mobile/AGENTS.md`, `.github/workflows/phone.yml`

**Interfaces:**
- Produces:
  - the `phone` workflow, with jobs `phone-analyze` (`flutter analyze`) and `phone-test` (`flutter test`), both `runs-on: macos-latest`;
  - the Flutter version comes from a `FLUTTER_VERSION` env in the workflow, set to this Mac's `flutter --version`;
  - the check names `phone-analyze` and `phone-test`, which Task 10 makes required.

- [ ] **Step 1:** Write one golden test of the chat kit's message bubble and composer from `mobile/lib/` (light and dark), using `matchesGoldenFile('kit_bubble_light.png')`. Flutter's test font (Ahem) keeps it the same across machines.
- [ ] **Step 2:** Run `cd mobile && flutter test test/goldens`. Expected: FAIL (no golden file yet).
- [ ] **Step 3:** Run `cd mobile && flutter test --update-goldens test/goldens`, then read the PNGs to confirm they show the bubble and the composer.
- [ ] **Step 4:** Run `cd mobile && flutter analyze && flutter test`. Expected: both pass. If `flutter analyze` reports existing issues, fix them only when they're mechanical. Otherwise record them in the ledger and make `phone-analyze` run `flutter analyze --no-fatal-infos`.
- [ ] **Step 5:** Write `mobile/AGENTS.md`, under 30 lines:
  - the phone app's rules from `mobile/DESIGN.md`;
  - `agentconsole.v1` is not to be changed casually;
  - goldens change only with the maintainer;
  - how to run analyze, test and goldens.
- [ ] **Step 6:** Write `phone.yml` with pinned actions (Task 7's allowlist) and `permissions: contents: read`.
- [ ] **Step 7:** Commit `"phone: analyze, test and golden checks (P0)"`.

### Task 9: The workflows

**Files:**
- Create: `.github/workflows/guard.yml`, `.github/workflows/maintainer-gate.yml`, `.github/workflows/review-signal.yml`, `tests/test_workflows.py`
- Modify: `.github/workflows/build.yml` (keep job id `build` for `swift test`; add job `app-build` running `./build.sh`, which builds arm64 and x86_64)

**Interfaces:**
- Consumes: the CLI subcommands `gate`, `lint`, `pins`, `codeowners`, `rulesets`.
- Produces the required check names:
  - `build`
  - `app-build`
  - `guard`
  - `maintainer-gate` (a commit status)
  - `phone-analyze`
  - `phone-test`

Workflow contract (the test reads the YAML as text; no YAML library):

| Workflow | Contract |
|---|---|
| `guard.yml` | `on: pull_request`, `permissions: contents: read`. Job `guard` runs on `ubuntu-latest`: the unit tests, `lint`, `pins`, `codeowners --check`, `rulesets`. |
| `maintainer-gate.yml` | `on: pull_request_target` (types opened, synchronize, reopened, ready_for_review) and `workflow_run` (workflows `["Review signal"]`, types completed). `permissions: contents: read, pull-requests: read, statuses: write`. |
| `review-signal.yml` | `name: Review signal`, `on: pull_request_review` (submitted, dismissed), `permissions: {}`, one step `run: echo "$PR"`, where `PR` is the PR number from `env`. It runs no repository code. |

The `maintainer-gate.yml` job does this:
1. Check out `main` (no `ref:` input; never `github.event.pull_request.head.sha`).
2. Fetch the PR head as data: `git fetch origin pull/<n>/head`.
3. Run `python3 -m guard gate --base <merge-base> --head <head sha> --reviews <file from gh api> --post`.
   - For a `workflow_run` event, the PR number and head SHA come from `github.event.workflow_run.pull_requests[0]`.
   - When that list is empty (fork PRs), they come from `gh api repos/{repo}/commits/{workflow_run.head_sha}/pulls`.

- [ ] **Step 1: Write the failing tests** in `test_workflows.py`:

```python
def test_gate_workflow_checks_out_base_only(self):
    text = read(".github/workflows/maintainer-gate.yml")
    self.assertIn("pull_request_target", text)
    self.assertNotRegex(text, r"ref:\s*\$\{\{\s*github\.event\.pull_request\.head")
    self.assertNotIn("actions/checkout@", text.split("git fetch origin pull/")[1])  # no second checkout of PR code
def test_review_signal_has_no_permissions(self):
    self.assertRegex(read(".github/workflows/review-signal.yml"), r"permissions:\s*\{\}")
def test_code_jobs_have_no_secrets(self):
    for f in ("guard.yml", "build.yml", "phone.yml"): self.assertNotIn("secrets.", read(".github/workflows/" + f))
def test_every_workflow_sets_permissions(self): ...
```

- [ ] **Step 2:** Run the tests. Expected: FAIL (files missing).
- [ ] **Step 3:** Write the workflows to the contract.
- [ ] **Step 4:** Run the tests, `python3 -m guard pins` and `python3 -m guard codeowners --check`. Expected: PASS.
- [ ] **Step 5:** Commit `"ci: guard, maintainer-gate, review signal, app build"`.

A ruling to record: a same-repo branch whose author can edit workflows could post a fake `maintainer-gate` status. Today only `@dtthaison` and TrietKun can do that, and the Phase 5 App has no `workflows` permission. Forks can't, because their token is read-only. Phase 5 revisits this if more writers are added.

### Task 10: Rulesets, Dependabot and security settings as code

**Files:**
- Create:
  - `.github/rulesets/main.json`
  - `.github/rulesets/agent-branches.json`
  - `.github/rulesets/release-tags.json`
  - `guard/rulesets.py`
  - `tests/test_rulesets.py`
  - `tools/github/apply-settings.sh`
- Modify: `.github/dependabot.yml`, `guard/__main__.py` (the `rulesets` subcommand)

**Interfaces:**
- Produces:
  - `def validate(rulesets_dir: Path, workflows_dir: Path) -> list[str]`. Problems are prefixed `GUARD-RULESETS:`.
  - CLI `python3 -m guard rulesets`.

What `main.json` must contain (the validator checks each one):
- target branch `~DEFAULT_BRANCH`;
- rules `deletion` and `non_fast_forward`;
- `pull_request`, with:
  - `require_code_owner_review: true`
  - `require_last_push_approval: true`
  - `dismiss_stale_reviews_on_push: true`
  - `required_approving_review_count: 0` (code owner review still applies to protected files)
  - `allowed_merge_methods: ["squash"]`
- `required_status_checks`, with:
  - `strict_required_status_checks_policy: false`
  - contexts `build`, `app-build`, `guard`, `maintainer-gate`, `phone-analyze`, `phone-test`, each with `integration_id: 15368`
- `bypass_actors: []`. The maintainer's override is through a PR, as the spec says; today's ruleset lets the admin role bypass always.

`agent-branches.json`:
- covers `refs/heads/agent/**`;
- rules `creation`, `update`, `deletion`, so only bypass actors may push there;
- empty bypass list until Phase 5 adds the App.

`release-tags.json`:
- target `tag`, `refs/tags/v*`;
- `creation`, `update`, `deletion`;
- empty bypass list until Phase 6 adds the release workflow.

The validator also checks that every required context matches a job id (or, for `maintainer-gate`, a posted status context) found in `.github/workflows/`.

`dependabot.yml`:
- ecosystems `github-actions` (`/`), `pub` (`/mobile`) and `swift` (`/`);
- each `schedule: weekly`, `cooldown: default-days: 7`;
- no auto-merge anywhere.

`apply-settings.sh` uses `gh api`. It prints every request first and asks `Apply? [y/N]`, and `--dry-run` only prints. It does these things, in order:
1. Create or update each ruleset by name (PUT if a ruleset with that name exists).
2. Delete the classic branch protection on `main` (`DELETE /repos/{r}/branches/main/protection`).
3. Turn on `secret_scanning`, `secret_scanning_push_protection` and `dependabot_security_updates`, plus private vulnerability reporting (`PUT /repos/{r}/private-vulnerability-reporting`).
4. Set Actions to `allowed_actions: selected`, with `github_owned_allowed: true`, `patterns_allowed` from the allowlist, and `sha_pinning_required: true`.
5. Set `allow_auto_merge: true`.
6. Set the repository Actions variables `AGENT_DAILY_RUN_CAP=20` and `REVIEW_DAILY_CAP=40`, which Phase 5's worker and review agent read.
7. Print the manual step: set a hard monthly spend limit at the model provider's console.

- [ ] **Step 1: Write the failing tests:**
  - the real files validate clean;
  - each requirement above has a test that removes it from a copy and expects that problem;
  - a required context with no matching job fails;
  - `apply-settings.sh --dry-run` prints `PUT`/`POST` lines and never calls `gh` (run it with `PATH` pointing at a fake `gh` that records its arguments and fails the test if called without `--dry-run` semantics).
- [ ] **Step 2:** Run the tests. Expected: FAIL.
- [ ] **Step 3:** Write the JSON files, `rulesets.py`, `dependabot.yml` and `apply-settings.sh`.
- [ ] **Step 4:** Run the tests and `python3 -m guard rulesets`. Expected: PASS.
- [ ] **Step 5:** Commit `"github: rulesets, Dependabot and security settings as code"`.
- [ ] **Step 6: Stop and ask the maintainer.** Show `tools/github/apply-settings.sh --dry-run` output and ask before running it. It changes live repository settings. Leave this for the maintainer to run or approve.

### Task 11: Prove it on a real pull request

**Files:** none new.

- [ ] **Step 1:** Push the branch and open a PR (draft) titled "Phase 2: guard rails". Its own checks run: `guard`, `build`, `app-build`, `phone-*`.
  - `maintainer-gate` can't run on this PR: `pull_request_target` uses `main`'s workflows, which don't have it yet.
  - Instead, run `python3 -m guard gate --repo . --base origin/main --head HEAD` locally. Expected: exit 1, listing `.github/` and `tools/` as `ZONE:`.
  - Paste the output into the PR. Steps 2–4 run after the maintainer merges this PR.
- [ ] **Step 2:** Open a throwaway draft PR from a branch `guard-probe-zone` that adds one comment line to `Sources/Notchling/Search/SearchPanel.swift`. Expected: `maintainer-gate` passes ("Nothing protected"). Close it without merging and delete the branch.
- [ ] **Step 3:** Open a second throwaway draft PR, `guard-probe-capability`, that adds `let p = Process()` to the same file. Expected: `maintainer-gate` fails with `CAP-PROCESS`. Close and delete it.
- [ ] **Step 4:** Write the three results into the Phase 2 PR description.

Steps 2–3 run only after the maintainer applied the settings in Task 10 Step 6, or with the maintainer's OK to run them against the current settings. The status is posted either way; only "required" depends on the rulesets.
