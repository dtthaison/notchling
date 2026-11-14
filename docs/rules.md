# Rules

Every rule here has an id, a reason and a check. When a check fails, its message starts with the
rule's id. `python3 -m guard rules` runs each check on that rule's own bad example and fails
(`GUARD-RULES`) if a listed rule has no check, a check has no listed rule, or a check stays quiet
on its bad example. Run the guard from the repository root with `PYTHONPATH=tools/guard`.

To add a rule, write its check and a bad example, register a probe in `tools/guard/guard/rules.py`,
and add a row here. Changing this file is a protected change.

The rules that block a merge (`LINT-*`, `GUARD-*`) fail the `guard` job. The rules that send a pull
request to the maintainer (`ZONE`, `CASE-COLLISION`, `CAP-*`, `WEAK-*`) set the `maintainer-gate`
status; the pull request merges only after the maintainer approves its current head.

## Architecture lint

Existing violations sit in `tools/guard/lint-baseline.json`, which can only shrink. A new one fails.

| ID | Rule | Why | Check |
| --- | --- | --- | --- |
| LINT-CORE-NO-NOTCHLING | ConsoleCore does not import Notchling | ConsoleCore is shared with hostd and must stay free of app types | `python3 -m guard lint` |
| LINT-CORE-MACOS13 | ConsoleCore uses no `@Observable` and no macOS 14+ checks | Notchling's minimum is macOS 13 | `python3 -m guard lint` |
| LINT-NO-FORCE-UNWRAP | No force-unwraps, `try!` or `as!` in Sources | A crash in an always-on menu bar app is worse than a handled nil | `python3 -m guard lint` |
| LINT-KIT-TOKENS | Chat and notch views use `Kit*` tokens, not raw colors or font sizes | One design kit keeps every view looking the same | `python3 -m guard lint` |
| LINT-DEFAULTS-MIGRATED | Every UserDefaults key is in `AppDelegate.migrateFromAwake`'s list | A key missing there is lost for users coming from the old app | `python3 -m guard lint` |
| LINT-POINTER-CURSOR | Clickable views show the pointing hand | A clickable view with the arrow cursor looks broken | `python3 -m guard lint` |
| LINT-NO-APPLESCRIPT | No NSAppleScript or osascript | Its notifications show Script Editor's name and icon | `python3 -m guard lint` |
| LINT-GPUI-MACOS-ONLY | In the root `Cargo.toml` and `crates/**/Cargo.toml`, `gpui` is a dependency only under `[target.'cfg(target_os = "macos")'.dependencies]` (or its dev and build forms), never under a plain `[dependencies]` or `[workspace.dependencies]`, by any spelling: a dotted key (`gpui.workspace = true`), a quoted key, a `[dependencies.gpui]` table, or a rename (`ui = { package = "gpui" }`); the manifest is read as TOML, not matched line by line | GPUI builds only on macOS for now: Linux needs system libraries CI doesn't install yet and Windows is unverified, so a plain entry breaks the Rust matrix; the other systems get a stub `main` instead | `python3 -m guard lint` |
| LINT-LOCALE-LITERAL | In an area `tools/locale/areas.json` marks as moved, no string literal that reads as words (two letters or more and a space, or a capital then lowercase) outside tests, renders and lines that name an element id, a part name, a log or an attribute | Once an area's words live in `locales/`, a literal added later shows English to every other language | `python3 -m guard lint` |
| LINT-LOCALE-CONSENT | (A warning.) An English message in `locales/` outside `consent.ftl` whose value starts with Allow, Deny, Always Allow, Approve, Pair, Trust, Install or Sign In | Words that grant or refuse something are translated only after the maintainer reads them, and `consent.ftl` is where that happens | `python3 -m guard lint --warnings` |
| LINT-FILE-SIZE | Files stay under 1,000 lines (the generated files, `crates/protocol/src/generated.rs`, `apps/flutter/lib/core/protocol/generated.dart` and `crates/locale/src/generated.rs`, are exempt by exact path, and so is the folder `apps/flutter/lib/l10n/`; a mark inside a file exempts nothing) | Big files are hard to review and for an agent to hold whole; nobody reads or splits what a generator writes, and a mark any file could carry would be a bypass | `python3 -m guard lint` |
| LINT-FILE-SIZE-WARN | Files over 600 lines should be split (a warning, not a failure; generated files are exempt) | A nudge before a file reaches the hard limit | `python3 -m guard lint --warnings` |
| LINT-LOG-NO-TEXT | In `crates/events/src/kinds/**`, no `String`, `str`, `Value` or `Vec<u8>` in code; and no `impl … LogField for` in any `crates/**/*.rs` file except `crates/events/src/field*.rs` | A log kind holds no free text: its fields come from the closed list of types in `field.rs`, and only those implement `LogField`, so a text field does not compile and no other file can add a type that holds one | `python3 -m guard lint` |
| LINT-LOG-ONE-WRITER | No `eprintln!`, `println!`, `eprint!`, `print!`, `tracing::` or `log::` macro, and no `use tracing::` or `use log::`, in `crates/*/src/**` outside inline test modules (from a `#[cfg(test)]` line followed by `mod name {` to the end of the file), test files (`*_tests.rs`, `tests_*.rs`, `tests.rs`, `testutil.rs`, `test_support.rs`), `crates/events/`, and the files whose output is the command line's or a tool's (`clients.rs`, `pages_cli.rs`, `vault_cli.rs`, `delegate_cli.rs`, `settings_cli.rs`, `service.rs`, `lib.rs` and `main.rs` of `crates/notchlingd/src/`; `crates/app/src/mac/render.rs`, `mac/bench.rs` and `chat/view/list_probe_run.rs`, whose output tests read). Today's uses are in the lint baseline, which only shrinks | One writer keeps the log typed and free of text: a stray print is an untyped, unfiltered record that skips the registry, its privacy rules and its retention | `python3 -m guard lint` |

## Capabilities

A new use of a powerful API sends the pull request to the maintainer, wherever the file is.

| ID | Rule | Why | Check |
| --- | --- | --- | --- |
| CAP-PROCESS | Starting a process or opening a file or app needs the maintainer | A process runs with the user's full rights | `python3 -m guard gate` |
| CAP-PTY | Opening a pty needs the maintainer | A pty drives a real terminal session | `python3 -m guard gate` |
| CAP-APPLESCRIPT | AppleScript needs the maintainer | It can control other apps | `python3 -m guard gate` |
| CAP-DYNAMIC | `dlsym`, `dlopen` and symbol renaming need the maintainer | They reach any API without naming it, which hides it from the other capability checks | `python3 -m guard gate` |
| CAP-NETWORK | Network access needs the maintainer | Data leaves the machine | `python3 -m guard gate` |
| CAP-KEYCHAIN | Keychain and keyring access need the maintainer | Logins and the Secret Vault live there | `python3 -m guard gate` |
| CAP-FILEWRITE | Writing, moving or deleting files needs the maintainer | A write can reach the user's real config or data | `python3 -m guard gate` |
| CAP-PROTECTED-TYPE | Using a protected type outside its own file needs the maintainer | These types wrap processes, ptys and credentials | `python3 -m guard gate` |
| CAP-BINARY | A binary change to a code file needs the maintainer | A binary file hides its lines from review | `python3 -m guard gate` |
| CAP-OBSCURE | A line with more `*/` than any real one has needs the maintainer | It can hide code from the check that reads comments | `python3 -m guard gate` |

## Weakening tests

| ID | Rule | Why | Check |
| --- | --- | --- | --- |
| WEAK-TEST-EDIT | Editing, deleting or renaming an existing test, fixture or golden needs the maintainer | What already proves the app works is only changed on purpose | `python3 -m guard gate` |
| WEAK-SKIP | Adding a skip or a hook that changes how tests run needs the maintainer | A skipped test proves nothing | `python3 -m guard gate` |
| WEAK-UNICODE | Invisible, bidirectional or control characters need the maintainer | They make code read differently from what runs | `python3 -m guard gate` |

## The line

| ID | Rule | Why | Check |
| --- | --- | --- | --- |
| ZONE | A change outside the auto-merge zone needs the maintainer | Only named safe areas merge without him (`tools/guard/zones.json`) | `python3 -m guard gate` |
| CASE-COLLISION | A new path whose name equals another's once case is ignored is refused | A Mac checkout keeps only one of the two, so one file silently replaces the other | `python3 -m guard gate` |

## The guard itself

| ID | Rule | Why | Check |
| --- | --- | --- | --- |
| GUARD-CODEOWNERS | `.github/CODEOWNERS` is exactly what `zones.json` generates | The zone and the code owners must never disagree | `python3 -m guard codeowners --check` |
| GUARD-PINS | Every workflow action is pinned to a commit SHA and from an allowed repository | A tag can be moved to other code after review; a SHA can't | `python3 -m guard pins` |
| GUARD-RULESETS | The rulesets in `.github/rulesets/` say what the guard relies on, and every required check is a job | A loosened ruleset, or a check that never runs, would stop the guard from guarding | `python3 -m guard rulesets` |
| GUARD-BASELINE | The lint baseline only shrinks | A baseline that grows hides new violations | `python3 -m guard gate` (a PR whose baseline grew needs the maintainer); `python3 -m guard lint` (refuses to overwrite it) |
| GUARD-CLAUDE-MD | The only tracked `CLAUDE*.md` is the root `CLAUDE.md`, and it holds exactly `@AGENTS.md` | Agent instructions live in `AGENTS.md`; a second file is an instruction source nobody reviews | `python3 -m guard claudemd` |
| GUARD-FEATURE-MAP | Every folder under `crates/*/src/` is named in `docs/feature-map.md`, in full or as `src/<folder>/` in its crate's row | The map is where an agent finds a feature's code, tests and zone; a folder missing from it is code nobody can find that way | `python3 -m guard featuremap` |
| GUARD-PROTOCOL-GEN | The Rust and Dart types in `crates/protocol/src/generated.rs` and `apps/flutter/lib/core/protocol/generated.dart` are exactly what `tools/protocol/gen.py` writes from `protocol/agentconsole.v1.schema.json` | A hand-edited or stale generated file lets the Mac, the phone and the Rust app disagree about the wire | `python3 tools/protocol/gen.py --check` |
| GUARD-LOCALE-GEN | The words in `crates/locale/src/generated.rs` (and the other outputs `locales/languages.json` lists) are exactly what `tools/locale/gen.py` writes from `locales/` | A hand-edited or stale generated file lets the app show words no translator wrote | `python3 tools/locale/gen.py --check` |
| GUARD-LOCALE-SUBSET | Every `.ftl` file under `locales/` stays inside the Fluent subset the generator reads: no attributes, nested or exact-number selects, message references or functions but `NUMBER()`, one-line values | A construct the Rust runtime and the phone read differently shows different words on the Mac and the phone | `python3 tools/locale/gen.py --check` |
| GUARD-LOCALE-MATCH | A translation uses the same variables as the English message, and plural selectors only where English has them | A missing or extra variable shows a blank or a raw placeholder in the user's language | `python3 tools/locale/gen.py --check` |
| GUARD-RULES | Every rule listed here has a check that fires on its bad example, and every check is listed | A rule without a working check is only a wish | `python3 -m guard rules` |
