# Localization: one English source, every platform generated

Status: design, waiting for the maintainer's approval. Date: 2026-11-13.
Approach A (a Fluent source) was chosen by the maintainer on 2026-11-13; this document turns it into a design. Once approved it becomes decision record 0022 (step L0 below; 0019 to 0021 are taken by other branches). Nothing is built yet.

## Contents

- [Intent](#intent)
- [Goals, non-goals, anti-goals](#goals)
- [Decided by the maintainer](#decided-by-the-maintainer)
- [Calls settled after review](#calls-settled-after-review)
- [What is there today](#what-is-there-today)
- [Approaches considered](#approaches-considered)
- [Constraints](#constraints)
- [The source format](#the-source-format)
- [Files and keys](#files-and-keys)
- [The generator](#the-generator)
- [Runtime in the Mac app](#runtime-in-the-mac-app)
- [What is never translated](#what-is-never-translated)
- [The registries that are coming](#the-registries-that-are-coming)
- [The phone](#the-phone)
- [Checks and rules](#checks-and-rules)
- [Zones: what the maintainer still reviews](#zones-what-the-maintainer-still-reviews)
- [Translating: the contributor guide](#translating-the-contributor-guide)
- [Longer text, and right to left](#longer-text-and-right-to-left)
- [Steps](#steps)
- [Left to the builder](#left-to-the-builder)
- [Open questions](#open-questions)

## Intent

Notchling is going open source, and people who don't read English will use it. The structure for other languages has to exist before they arrive, so that a translation is text files and nothing else: copy the English files, translate them, run one command, open a pull request. Once it merges, the app built from GitHub speaks that language.

Today the product's words have no home. The Mac app holds about 2,000 English literals in `crates/app` and `crates/kit`. The phone keeps two hand-written ARB files with Vietnamese as the template. The daemon writes Vietnamese text that the phone shows. A word both apps show is written twice.

The phone may later be rewritten natively (Swift, Kotlin). So the source can't be any one platform's format: one source generates every platform's file.

## Goals

1. One source of words per language, in `locales/`, that a translator can read and edit in a text editor.
2. English is the source and, for now, the only language that ships, on the Mac and on the phone.
3. A translation is text files only: no Rust, Dart or build knowledge. One command checks it and says how complete it is.
4. A mistake in a translation fails CI, never the app: an unknown key, a wrong variable, broken syntax.
5. A message a language lacks shows in English, message by message.
6. Code that names an unknown message, or passes the wrong variables, does not compile (Rust) or analyze (Dart).
7. Text costs what it costs today. A plain message is a `&'static str`; Chat's cached parts allocate nothing new and draw no more often.
8. New hard-coded user-facing text can't come back into an area that has moved.
9. New designs use it from the start: the Settings search registry and the log event-kind registry first.

### Non-goals

- Shipping a second language now.
- Right-to-left layouts. [Longer text, and right to left](#longer-text-and-right-to-left) says what they would take.
- A translation platform (Weblate, Crowdin, Pontoon). Fluent works with them later; nothing here needs one.
- Date formatting beyond the two date forms the app shows, and time zones (the app has no time zone reader yet, `pages/dates.rs`).
- iOS and Android files in use. The generator can write them; nothing builds them yet.
- Switching language without a relaunch.

### Anti-goals

These count as failures even if everything works:

- A translation that changes what a consent prompt says or does lands without the maintainer: "Allow" written as the word for "Deny" in the vault approval.
- Text the app matches against (a daemon marker, a dialog on Claude Code's screen, git's output) goes through translation and stops matching in another language.
- A log line, an audit line or a stored row holds translated text.
- A generated file that differs from its source passes CI.
- A text is written twice within one app, or shared between the apps where they don't show exactly the same text.
- A language makes the app slower, or a draw allocate.

## Decided by the maintainer

Approach A, 2026-11-13:

- Source format: Project Fluent (`.ftl`), restricted to a subset every target can express. A lint rejects the rest.
- Layout: `locales/<lang>/shared.ftl` (words both apps use), `mac.ftl`, `phone.ftl`. This design splits it further (below).
- A generator, `tools/locale/gen.py`, Python standard library only, writes: Rust for the Mac app with a compile error on an unknown key or wrong variables; the Flutter ARB files, replacing the hand-written ones; and, ready but off, iOS `Localizable.xcstrings` and Android `values-<lang>/strings.xml` with `<plurals>`. CI checks the generated files match the source, as `GUARD-PROTOCOL-GEN` does for the protocol.
- Every translation has the same keys and variables as English; a missing message falls back to English at run time; CI reports each language's completeness.
- Language follows macOS's preferred languages. A Settings "Language" row appears only once two or more languages ship.
- Not translated: the `notchling` command line, skills, anything agents read, logs and the audit log (they store keys, not text), wire and protocol strings.
- The phone becomes English only: its Vietnamese is dropped and English becomes the source.
- Migration area by area, one pull request each: Settings, then Chat, the notch, the other windows. A lint stops new hard-coded text in an area that has moved.

## Calls settled after review

A builder read the first version of this design; the coordinator settled these on 2026-11-14. Each is worked into the sections it touches.

1. **Phone scope** (coordinator's call). Every user-visible phone string becomes a message, including the hard-coded Dart literals outside `lib/l10n/` (`core/net/bridge_client.dart`, `features/chat/photo_send.dart`, `chat_notifier.dart`, `features/sessions/new_session_sheet.dart`, `features/visualize/viz_providers.dart`, `app/notifications.dart`). `BridgeException` carries a code and the UI maps the code to its own message. Notifications and other code without a `BuildContext` use a context-free lookup: the current locale's `AppL10n`, held once at app start and on a locale change. The dictation prompt and cleanup text (`features/voice/prompt_builder.dart`, `transcript_cleanup.dart`) are not UI: they stay Vietnamese for now, pending open question 3.
2. **Base and order** (coordinator's call). The rebuild merges into `agent-run-oss`, not `main`; every L-step targets `agent-run-oss`, through the integration branch `merge/settings-batch` first. Branches in flight merge first: the ones in `merge/settings-batch` plus any open `agent/*` branch touching an area; the decision record keeps no fixed list. An area's literal migration (L3 on) starts only after the branches touching that area have merged, and `areas.json` arms the lint for an area in the same pull request that moves it. L0 to L2 (the crate, the generator, the guards, the phone switch) come before the Settings search registry and the log event-kind registry are built, so both are written with messages from the start; their plans depend on L1.
3. **Shared versus per-app wording** (coordinator's call). A message goes in `shared.ftl` only when both apps show exactly the same text, case included. The Mac keeps Title Case for menus and buttons; the phone keeps its casing; where they differ they are two keys, one in `mac/` and one in `phone/`. Anti-goal 5 reads accordingly. English rendering stays pixel-identical.
4. **Outputs follow file location** (coordinator's call). `shared.ftl` (and `consent.ftl`) go to both outputs; `mac/**` only to Rust; `phone/**` only to ARB. Mac-only content (settings titles and search words, log event kinds, the Mac's units and dates) lives in `mac/` files, not `shared.ftl`, which stays small: session states, inbox kinds and actions, anything both apps show. The generator does no usage scanning.
5. **`choose`** (coordinator's call) lives in the `notchling-locale` crate; every entry point calls it once at start: `notchling-app`'s shell, `notchling-machines`, `--render` and the benches, all honoring `NOTCHLING_LOCALE`.
6. **Inbox** (coordinator's call). All five kinds (secret approval, secret request, limit notice, action failed, notice) get a `code` and `facts` in L6, so titles and action labels translate. Any free-text detail from the daemon stays English and is shown as detail.
7. **Splitting sentences** (coordinator's call). Splitting a sentence into separate messages is accepted where a part is styled or clickable; a new line is a separate message; literal braces on the Mac are escaped by the generator. Each case is decided in its migration pull request.
8. **Wrong English plurals** (coordinator's call). A migration pull request keeps English pixel-identical, wrong plurals included ("1 subagents running"). Plural fixes go in their own pull request right after the step that moved the area, with before and after renders.
9. **Decision number** (coordinator's call). The decision record is 0022: 0019, 0020 and 0021 are taken or reserved by other branches.

## What is there today

| Where | What | Size |
| --- | --- | --- |
| `crates/app/src`, `crates/kit/src` | English literals in models (`Row.title: &'static str`, `AWAKE_MODES`) and views (`"Reply…"`), sentences built with `format!`, plurals made by hand (`pages/model.rs::count_text`: "1 artifact" or "{n} artifacts", then `" · not all shown"` appended; `vault/approval.rs`: "line"/"lines"; `switches/restart.rs`: "is off"/"are off") | about 2,000 literals (a rough count of capitalized ones, part names included): chat 790, accounts 180, sheets 150, machines 100, tools 90, vault 85 |
| `apps/flutter/lib/l10n/` | `app_vi.arb` (the template) and `app_en.arb`, 199 keys each, 21 with placeholders, no plural ("{count} subagents running" says "1 subagents"); `app_localizations*.dart` written by `flutter gen-l10n` and committed | `app_localizations.dart` is 1,326 lines, in the lint baseline |
| `apps/flutter/lib/app/app.dart` | `localeResolutionCallback` picks `en` for an English device, `vi` for any other | |
| `apps/flutter/lib/` outside `l10n/` | hard-coded Vietnamese: `BridgeException('disconnected', 'Chưa kết nối tới Mac.')` and other messages in `core/net/bridge_client.dart`, repeated in `photo_send.dart`, `new_session_sheet.dart`, `viz_providers.dart`, `chat_notifier.dart`; notification titles and bodies in `app/notifications.dart` | `BridgeException` already carries a code |
| `apps/flutter/test/` | the harness defaults to `Locale('vi')`; 14 test files assert Vietnamese text | about 95 lines |
| `crates/engine/src/bridge/`, `viz/engine.rs`, `control/takeover.rs`, `crates/notchlingd/src/bridge_backend.rs`, `crates/ptyhost/` | Vietnamese text the daemon sends: the bridge's `error.message` (`protocol/README.md`: "in Vietnamese; do not parse it"), Visualize's empty states, takeover reasons, ptyhost errors | about 110 lines |
| `crates/notchlingd/src/vault_inbox.rs` | the daemon writes each inbox item's title and action labels in English ("Allow {names} for {command}", "Allow Once", "Always Allow", "Deny", "Open Form"), stores them in `notchling.db` and sends them to the app and the phone. The item already carries its `kind` and each action's `id` | |
| `apps/flutter/lib/features/voice/` | dictation: Whisper runs with language `vi`; `prompt_builder.dart` and `transcript_cleanup.dart` hold Vietnamese prompt and cleanup text | the language you speak, not the UI's |
| `crates/app/src/chat/prompt_flow.rs`, `prompts.rs`, the screen reads | text matched against the daemon's refusals and Claude Code's dialogs; git runs under `LC_ALL=C` | must never be translated |
| `.part("Send Button")` | part names the ? button shows; agents grep them | must never be translated |

## Approaches considered

| | A: Fluent source, generated outputs | B: ARB (ICU MessageFormat) for both apps | C: plain key-value (`rust-i18n` YAML, or JSON) |
| --- | --- | --- | --- |
| For translators | Plain text, one message per line, notes as `#` lines, plurals readable as a list | JSON: escaping, `@key` metadata blocks, ICU's apostrophe quoting ("didn't" breaks when escaping is on) | Easiest to read |
| Plurals (CLDR) | Yes, as a select over a number | Yes | No: `rust-i18n` interpolates only, so "1 subagents" stays |
| Mac (Rust) | Generated typed functions; compile error on a wrong key or variable | Needs an ICU MessageFormat parser in Rust: no mature permissive crate (ICU4X's message formatting is experimental), so we'd write one anyway | `t!("key")` checked at run time; a missing key prints the key |
| Phone (Flutter) | Generated ARB, then `flutter gen-l10n` as today | Native | Needs a converter to ARB anyway |
| Native iOS and Android later | Generated, same as ARB | Converter needed, same work as A | Converter, and plurals are lost |
| Tooling | Pontoon, Weblate and Crowdin read Fluent | Wide support | Wide support |
| Cost | A subset parser in Python (no Fluent library in the standard library) and a subset lint, since Fluent can say more than ARB or Android can | The same generator work for Rust, iOS and Android, on a harder syntax | Small now, rewritten when plurals are needed |

**Picked: A.** One readable source with the plural grammar every target can express. The generator gives typed APIs and each platform's file, so neither the Mac nor a future native phone is tied to another platform's format. B makes the phone native and every other target a conversion from a syntax that is harder to write by hand. C fails on plurals, the first thing a second language needs.

### Inside A: how the Mac reads its words

Two ways to run Fluent in the Rust app:

| | R1: `fluent-bundle` at run time | R2: tables the generator compiles (chosen) |
| --- | --- | --- |
| Parsers of the source | Two: `gen.py` for the phone and native outputs, `fluent-syntax` for the Mac. They can disagree on edge cases (whitespace, escapes) | One: `gen.py`, for every output |
| At start | Parse every `.ftl` embedded with `include_str!` | Nothing: the tables are `static` |
| A plain message | `format_pattern` returns `Cow<str>`; needs `FluentArgs` and a borrow of the bundle | `&'static str`, no allocation, two array reads |
| Errors | At run time (a missing variable) unless typed wrappers prevent them | At generation; the wrappers make them compile errors |
| Isolation marks | Inserts U+2068/U+2069 around every variable unless turned off | None |
| Number formatting | `NUMBER` prints Rust's `Display`: no grouping, no locale | ICU4X `icu_decimal`: CLDR grouping and separators |
| Crates | 8 new to `Cargo.lock` | 7 new (ICU4X plurals and decimals and their data); the rest of their family is already in `Cargo.lock` through `url` and `idna` |

R2 ships. `fluent-bundle` stays as a dev-dependency: a test formats every message of every shipped language with both the tables and `fluent-bundle` and compares them, so the tables can't drift from what Fluent means, and every `.ftl` is proven to be standard Fluent that any Fluent tool can read.

## Constraints

### Licenses

Checked against `deny.toml` on crates.io and the RustSec advisory database on 2026-11-13.

| Crate | Version | License | Role | In `Cargo.lock` today |
| --- | --- | --- | --- | --- |
| `icu_plurals` | 2.4.0 | Unicode-3.0 | run time: plural categories | no |
| `icu_plurals_data` | 2.4.0 | Unicode-3.0 | run time: CLDR data (7.7 KB compressed crate) | no |
| `icu_decimal` | 2.4.0 | Unicode-3.0 | run time: number formatting | no |
| `icu_decimal_data` | 2.4.0 | Unicode-3.0 | run time: CLDR data (23 KB compressed crate) | no |
| `fixed_decimal` | 0.7.3 | Unicode-3.0 | run time | no |
| `icu_locale_fallback` (+ `_data`) | 2.4.0 | Unicode-3.0 | run time: locale fallback for the data | no |
| `icu_locale_core`, `icu_provider`, `zerovec`, `writeable`, `tinystr`, `yoke`, `potential_utf` | 2.4.0, 2.4.0, 0.11.9, 0.6.4, 0.8.5, 0.8.4, 0.1.6 | Unicode-3.0 | run time | yes, through `idna` |
| `displaydoc`, `smallvec` | 0.2.7, 1.16.5 | MIT OR Apache-2.0 | run time | yes |
| `fluent-bundle`, `fluent-syntax` | 0.16.0, 0.12.0 | Apache-2.0 OR MIT | dev only: the conformance test | no |
| `fluent-langneg`, `intl-memoizer` | 0.13.x, 0.5.3 | Apache-2.0 OR MIT | dev only | no |
| `intl_pluralrules` | 7.0.2 | Apache-2.0/MIT | dev only | no |
| `unic-langid`, `unic-langid-impl` | 0.9.6 | MIT OR Apache-2.0 | dev only | no |
| `type-map` | 0.5.1 | MIT/Apache-2.0 | dev only | no |
| `rustc-hash`, `thiserror` | 2.1.3, 2.0.21 | Apache-2.0 OR MIT | dev only | yes |
| `self_cell` | 1.3.0 | Apache-2.0 OR GPL-2.0-only | dev only | yes |
| `memchr` | 2.8.3 | Unlicense OR MIT | dev only | yes |

- Every crate passes the allow list. Two are dual-licensed with a side the list doesn't name (`self_cell`'s GPL-2.0-only, `memchr`'s Unlicense) and pass on their permissive side, as they already do today.
- No advisory names any new crate. `self_cell`'s RUSTSEC-2023-0070 is patched from 1.0.2; the tree has 1.3.0. `intl_pluralrules` last released in 2022 but has no unmaintained advisory, so `unmaintained = "all"` stays green; it is dev only.
- ICU4X 2.4 needs Rust 1.88; we pin 1.99.1. `fixed_decimal`'s optional `ryu` (Apache-2.0 OR BSL-1.0) stays off.
- Unicode-3.0 is on `deny.toml`'s list and ICU4X is already linked through `idna`, but [decision 0008](../../decisions/0008-mit-permissive-crates.md) names only MIT, Apache and BSD, and these would be our first direct Unicode-3.0 dependencies. Open question 1.

### GPUI text

- Fonts: GPUI shapes with Core Text. Scripts outside Latin (CJK, Cyrillic, Greek, Thai, Devanagari) and Vietnamese's stacked marks need a render each to confirm fallback fonts and line heights; the kit's `TypeRole` line heights may need room. Checked when a language with that script arrives, not now.
- Eliding: `elide()` and any `chars().take(n)` cut in display code must cut at grapheme clusters (`unicode-segmentation` is already in `Cargo.lock`), or a combining mark is cut from its letter.
- Casing: the kit's uppercase `TypeRole` calls `to_uppercase()`, which is wrong for Turkish and Azerbaijani (i to İ) and keeps the accents Greek drops in capitals. When such a language arrives the locale crate gets `upper(text)`; until then nothing changes. No other code changes the case of translated text.
- `SharedString::new_static` takes a plain message with no allocation.

### No comments in code

Translator notes live in the `.ftl` files as `#` lines. They are data addressed to translators, as ARB's `description` is today, and the generator copies them into each platform's note field. `tools/strip_comments.py` never reads them: its scope is `.rs` under `crates/`, `.dart` under `apps/flutter/lib` and `test`, `.py` and `.sh` under `tools/`. The generated Rust carries no notes, only an `@generated` header line, and `strip_comments.py` skips it twice over: by its name `generated.rs` and by that header. `app_localizations*.dart` is already skipped by name. The root `AGENTS.md` style line gets one sentence: translator notes go in `.ftl` files.

### The guard

- `WEAK-UNICODE` reads every added line, `.ftl` included, and flags format characters such as U+200C (Persian, Hindi), U+200D and the soft hyphen U+00AD. In a `.ftl` such a character is written as an escape in a string literal, `{"\u200C"}`, so it is visible in review; the generator writes it as `\u{200c}` in Rust and `\u200c` in JSON, so no generated file trips it either. A translation that writes one raw goes to the maintainer, which is right.
- `LINT-FILE-SIZE` reads `crates/**` and `apps/flutter/lib/**`. `crates/locale/src/generated.rs` and `apps/flutter/lib/l10n/**` join its generated exemptions by exact path and folder; `app_localizations.dart` leaves the baseline.
- `CASE-COLLISION`: language tags are written in one form (`pt-BR`, `zh-Hans`, `de`), which `gen.py` enforces.
- Python: standard library only, 3.9 or later, like the guard.

### The rest

- The protocol and the local API change additively only. The inbox's structured facts (below) are a new optional field.
- `notchling-locale` builds and tests on macOS, Linux and Windows: no GPUI, no platform code.
- Chat's counts in `tests/bench_chat.rs` hold after every migration step.

## The source format

Fluent, restricted to what Rust, ARB, xcstrings and Android `strings.xml` can all say.

| Construct | Allowed | Why |
| --- | --- | --- |
| Message `key = text` on one line | yes | The core |
| Variable `{ $name }` | yes; a string | Every target has string placeholders |
| `{ NUMBER($name) }` with no options | yes; marks `$name` as a whole number, formatted for the locale | Fluent has no types; this is how English declares a number. Options differ per target |
| A select `{ $count -> … }` | yes: the whole value, over one number variable, variants `[zero]` `[one]` `[two]` `[few]` `[many]` and a default `*[other]` | Android's `<plurals>` covers a whole string, over one number, by category only |
| Inside a variant | text, string variables, number variables (a number variable prints as a number), on one line | Same as a message |
| Exact-number variants (`[0]`, `[1]`) | no | Android has no exact match; iOS treats `zero` specially. "No sessions" is its own message |
| Selects on strings (gender, kind), nested selects, two selects in one message | no | Android can't; ICU `select` has no Android equivalent |
| Terms `-brand-name = Text` | only `-brand-*`, plain text, defined only in English `shared.ftl`, inlined by the generator | Product names stay the same in every language and are written once |
| Other terms, terms with variables or variants | no | Inlining a parameterised term needs Fluent's runtime |
| Message references `{ other-key }` | no | Couples messages; translators can't see the whole sentence |
| Attributes (`.title`, `.tooltip`) | no | Not in ARB or Android; use two keys |
| Functions other than `NUMBER`; `DATETIME` | no | Dates are messages with variables (below) |
| String literals `{"…"}` | yes, for `{`, `}`, a space at either end and hidden characters as `\uXXXX` | Fluent's only escape; keeps every character visible |
| Text across several lines | no | Each target escapes newlines differently and editors change indentation; a new line is a new message (call 7) |
| A space at the start or end of a value | no, unless written `{" "}` | Fluent trims them; writing them out says they are meant |
| `{` `}` as text in `mac/` | yes, written `{"{"}` and `{"}"}` (Fluent's own escape); the generator emits a plain brace in Rust (call 7) | Rust tables have no format syntax to collide with |
| `{` `}` as text in `shared.ftl`, `consent.ftl` and `phone/` | no | ARB would need ICU's quoting, which breaks apostrophes |
| `#` note, `##` group, `###` file comments | yes | Notes for translators; copied to every output |
| Tabs, CRLF, a byte order mark, parse errors | no | One form, so diffs and the parser stay simple |

A variable's type comes from English: a number if English uses it in `NUMBER()` or as a selector, else a string. A translation may select on any number variable English has, even where English doesn't, since its plurals may differ ("{ $count } open" in English, three forms in Polish). It may not select on a string variable. Every English variable must appear in the translation's `*[other]` variant, or in its value when it has no select; other variants may leave a number out ("[one] one file").

The grammar, informally:

```
resource  = { blank | comment | entry }
entry     = [ note ] message | brand-term
message   = key " = " value NL
          | key " = { $" var " ->" NL { variant NL } [ SP+ ] "}" NL
variant   = SP+ ( "[" category "]" | "*[other]" ) " " value
value     = { text | "{ $" var " }" | "{ NUMBER($" var ") }" | "{ " string " }" | "{ -brand-" name " }" }
key       = word { "-" word }        word = [a-z][a-z0-9]*
var       = [a-z][a-z0-9]* { "_" [a-z0-9]+ }
category  = "zero" | "one" | "two" | "few" | "many"
```

## Files and keys

```
locales/
  languages.json            the languages that ship (protected)
  en/                        the source
    shared.ftl               both apps, only text both show exactly alike: brand terms, language-name,
                             session states, inbox kinds and results
    consent.ftl              both apps: every word that grants or refuses something (protected)
    mac/
      settings.ftl           the Settings sheet and the search registry's titles, lines and search words
      logs.ftl               log event kinds, for the log viewer
      units.ftl              ages, durations, dates, compact numbers, the list separator
      chat.ftl  notch.ftl  accounts.ftl  vault.ftl  kit.ftl  …           one file per area
    phone/
      sessions.ftl  chat.ftl  inbox.ftl  pairing.ftl  …              one file per feature folder
  de/                        a translation: the same files, any subset of their messages
```

- Why split further: the Mac alone has about 2,000 messages; one `mac.ftl` would be a 3,000-line file that every pull request touches. An area file is what one migration step adds, what one translator can finish in a sitting, and where a key is found from its name.
- `consent.ftl` is a file of its own so its zone can differ: it holds the buttons and titles of every prompt that allows, denies, approves, pairs, signs in, trusts a folder, installs the hook or turns a switch back on, and the line that says what is being allowed ("Allow { $names } for { $command }"). Settings rows are not consent.
- Outputs follow file location (call 4): `shared.ftl` and `consent.ftl` go to both apps, `mac/**` only to the Rust tables, `phone/**` only to the ARB files (and, once on, to the iOS and Android files, which are the phone's). The generator never scans code for which keys are used.
- `shared.ftl` stays small. A message goes there only when both apps show exactly the same text, case included (call 3). The Mac's menus and buttons are Title Case and the phone keeps its own casing; where the words or the casing differ, they are two keys, one in `mac/<area>.ftl` and one in `phone/<feature>.ftl`.
- `languages.json`: `{"source": "en", "ship": ["en"]}`. A folder not listed is checked and reported but not built into either app.
- Each language's `shared.ftl` defines `language-name`, its own name as its speakers write it ("Deutsch"), for the Language row.

Key rules, enforced by `gen.py`:

1. Lowercase ASCII words joined by `-`, each word starting with a letter: `settings-act-on-sessions`. At most 64 characters.
2. A key starts with its file's name: `mac/settings.ftl` holds `settings-…`, `phone/pairing.ftl` holds `pairing-…`, `consent.ftl` holds `consent-…`. `shared.ftl` uses its own namespaces, which no area file may use: `language-`, `state-`, `session-`, `item-` (inbox items).
3. A key is unique within a language across all its files. Within one app a text is written once; between the apps it is shared only where identical (call 3).
4. When an English message's meaning or variables change, it gets a new key. A typo fix keeps the key. Old keys are then reported in translations, and `gen.py --prune` removes them. This is Mozilla's Fluent rule, and it keeps translations from silently going stale.

Names per platform, from one key:

| | `session-subagents-running`, variable `$folder_name` |
| --- | --- |
| Rust | `t::session::subagents_running(…)`, parameter `folder_name` (a keyword gets `r#`) |
| Dart (ARB) | `l10n.sessionSubagentsRunning(…)`, placeholder `folderName` |
| iOS | key `session-subagents-running`, positional `%1$@` |
| Android | `R.plurals.session_subagents_running` or `R.string.…`, positional `%1$s` |

## The generator

```diagram
locales/<lang>/*.ftl ─┐
languages.json ───────┤
                      ▼
            tools/locale/gen.py  (parse the subset, check, compare with English)
                      │
     ┌────────────────┼──────────────────────┬────────────────────────────┐
     ▼                ▼                      ▼                            ▼
crates/locale/   apps/flutter/lib/l10n/   Localizable.xcstrings      values[-<lang>]/strings.xml
src/generated.rs app_<lang>.arb           (iOS, off)                 (Android, off)
     │                ▼
     │          flutter gen-l10n → app_localizations*.dart
     ▼
notchling-app (typed functions, static tables)
```

```sh
python3 tools/locale/gen.py                 # check every language, write every output that is on, print the report
python3 tools/locale/gen.py --check         # write nothing; exit 1 on any error or stale output (CI)
python3 tools/locale/gen.py --report [--lang de] [--missing] [--markdown]
python3 tools/locale/gen.py --new de        # copy en/ to de/ as a start
python3 tools/locale/gen.py --prune         # remove keys English no longer has from every translation
python3 tools/locale/gen.py --ios DIR --android DIR   # the outputs that are off
```

Errors name a rule id, a file and a line, as `GUARD-PROTOCOL-GEN` does. Code: `tools/locale/gen.py` (the command), `ftl.py` (the subset parser), `check.py` (rules and comparison), one `emit_*.py` per output, `report.py`; tests in `tools/locale/tests/` with a small fixture tree, its golden outputs, and every refusal with its message.

### Outputs

Each output takes its messages by file location (call 4): Rust gets `shared.ftl`, `consent.ftl` and `mac/**`; ARB, xcstrings and Android get `shared.ftl`, `consent.ftl` and `phone/**`.

- **Rust**, `crates/locale/src/generated.rs`, one file: an id per message, a module per key namespace with a function per message, and one static table per shipped language. English's table is complete; another's holds `None` where a message is missing.
- **ARB**, `apps/flutter/lib/l10n/app_<lang>.arb` for each shipped language, the phone's messages only. Each is complete: a missing message is filled from its parent (`pt` for `pt-BR`) and then English at generation, so Flutter's behavior doesn't depend on its own fallback. Notes become `description`, number variables `int` with `"format": "decimalPattern"`. `flutter gen-l10n` then writes `app_localizations*.dart` as today.
- **iOS**, `Localizable.xcstrings` (off): one catalog, `sourceLanguage` `en`, each language under `localizations`, notes as `comment`, `extractionState` `manual`. A message with one variable that selects uses `variations.plural`; with more variables, the `%#@count@` substitution form.
- **Android**, `values/strings.xml` for English and `values-<qualifier>/strings.xml` per language (`values-de`, `values-pt-rBR`, `values-b+zh+Hans`) (off): `<string>` and `<plurals>`, notes as XML comments before the element. Missing messages fall back to `values/` on their own.
- The iOS and Android writers are built and tested in step L1 so they are proven before a native app needs them; `--check` covers them once they are turned on.

| Source | Rust | ARB | xcstrings | Android |
| --- | --- | --- | --- | --- |
| text | `&'static str` | string | `stringUnit.value` | `<string>` |
| `{ $name }` | `name: &str` | `{name}`, `String` | `%N$@` | `%N$s` |
| `{ NUMBER($n) }` | `n: impl Into<Num>` | `{n}`, `int`, `decimalPattern` | `%N$lld` | `%N$d` |
| select on `$n` | a plural body | `{n, plural, one{…} other{…}}` | `variations.plural` or `%#@n@` | `<plurals>` with `<item quantity>` |
| `{ -brand-x }` | inlined | inlined | inlined | inlined |
| `#` note | none | `@key.description` | `comment` | `<!-- … -->` |
| `'` | `'` | `'` | `'` | `\'` |
| `%` | `%` | `%` | `%%` | `%%` in a formatted string |
| `pt-BR` | table `PT_BR` | `app_pt_BR.arb` | `pt-BR` | `values-pt-rBR` |

### One plural message, in every output

The source, `locales/en/shared.ftl` (shared because, in this example, both apps show exactly this text):

```ftl
## Counts

# Title of the Subagents section while some still run. $count is how many run.
session-subagents-running = { $count ->
    [one] { $count } subagent running
   *[other] { $count } subagents running
}
```

A German translation, `locales/de/shared.ftl`:

```ftl
session-subagents-running = { $count ->
    [one] { $count } Subagent läuft
   *[other] { $count } Subagents laufen
}
```

Rust, `crates/locale/src/generated.rs` (shape, not exact text):

```rust
pub mod session {
    use crate::{Arg, Num, Text};
    pub fn subagents_running(count: impl Into<Num>) -> String {
        crate::format(Text(214), &[Arg::Num(count.into())])
    }
}
pub(crate) static EN: [Body; 2140] = [
    Body::Plural(0, &[(One, &[Int(0), Lit(" subagent running")]), (Other, &[Int(0), Lit(" subagents running")])]),
];
```

Called as `t::session::subagents_running(running.len())`.

ARB, `apps/flutter/lib/l10n/app_en.arb`:

```json
"sessionSubagentsRunning": "{count, plural, one{{count} subagent running} other{{count} subagents running}}",
"@sessionSubagentsRunning": {
  "description": "Title of the Subagents section while some still run. $count is how many run.",
  "placeholders": { "count": { "type": "int", "format": "decimalPattern" } }
}
```

iOS, `Localizable.xcstrings`:

```json
"session-subagents-running" : {
  "comment" : "Title of the Subagents section while some still run. $count is how many run.",
  "extractionState" : "manual",
  "localizations" : {
    "en" : { "variations" : { "plural" : {
      "one" : { "stringUnit" : { "state" : "translated", "value" : "%lld subagent running" } },
      "other" : { "stringUnit" : { "state" : "translated", "value" : "%lld subagents running" } }
    } } }
  }
}
```

Android, `values/strings.xml`:

```xml
<!-- Title of the Subagents section while some still run. $count is how many run. -->
<plurals name="session_subagents_running">
    <item quantity="one">%1$d subagent running</item>
    <item quantity="other">%1$d subagents running</item>
</plurals>
```

## Runtime in the Mac app

### Where it lives

A new library crate, `crates/locale` (package `notchling-locale`): the runtime (`lib.rs`, `choose.rs`, `format.rs`, `fmt.rs`, `pseudo.rs`) and the generated `generated.rs`. No GPUI and no platform code, so it builds and tests everywhere: `choose` is pure and takes the system's languages as an argument. `app` and `kit` depend on it; the engine, the daemon, the hook and the command line never do. The kit owns almost no words (the ? button's tooltip); they go in `mac/kit.ftl`.

### How views call it

```rust
use notchling_locale as t;

t::settings::act_on_sessions()            // &'static str
t::settings::ACT_ON_SESSIONS               // Text: .get() and .english(), for const tables
t::session::subagents_running(n)           // String; n is any integer type
t::chat::moved_to(&folder)                 // String; a string variable is &str
t::lookup("logs-event-hook-permission")   // Option<Text>, for registries keyed by data
t::fmt::count(1234)                        // "1,234"
t::fmt::decimal(12.3, 1)                   // "12.3"
t::fmt::list(&names)                       // "a, b"
```

Before and after, `sheets/settings.rs`:

```rust
const DAEMON_ROWS: [(Area, &str); 5] = [(Area::Acting, "Act on Sessions"), …];
const DAEMON_ROWS: [(Area, Text); 5] = [(Area::Acting, t::settings::ACT_ON_SESSIONS), …];
```

and `pages/model.rs::count_text`, where a sentence was glued from parts:

```ftl
pages-count = { $count ->
    [one] { $count } artifact
   *[other] { $count } artifacts
}
pages-count-partial = { $count ->
    [one] { $count } artifact · not all shown
   *[other] { $count } artifacts · not all shown
}
```

```rust
if self.truncated { t::pages::count_partial(n) } else { t::pages::count(n) }
```

Model types don't change: a title that was `&'static str` stays one.

### Choosing the language

`t::choose` lives in `notchling-locale` (call 5). Every entry point calls it once at start, before any window: `notchling-app`'s shell (`mac/shell.rs::run`, next to `history::apply`), `notchling-machines`, `--render`, and the benches (which run through `--render`). All of them honor `NOTCHLING_LOCALE`. The daemon, the hook and the command line never call it. An entry point that forgot to would show English, the default. The first that names a shipped language wins:

1. `NOTCHLING_LOCALE` in the environment (development, tests, renders; `en-XA` is the pseudo-language below).
2. The Settings Language choice, when it isn't System: a new `language` key in `settings/keys.rs`, read with the store at start. The row exists only once two languages ship.
3. macOS's preferred languages in order (`platform::locale::preferred()`, from `NSLocale.preferredLanguages`): each tag exactly, then language and script, then language alone. `de-AT` gets `de`. This list already honors the per-app language a person sets in System Settings > General > Language & Region > Applications, which macOS offers for an app once its bundle declares two localizations (`CFBundleLocalizations`, step L7).
4. English.

A render or a test is English unless `NOTCHLING_LOCALE` says otherwise, whatever the Mac's languages, as a render never reads `~/.claude`: `--render` passes `choose` no system languages. A change in the Settings row applies at the next launch, with a Relaunch button; macOS's per-app language behaves the same.

`platform::locale` is new in the `platform` crate: `preferred()` (macOS through `objc2-foundation`, already in `Cargo.lock`; Linux from `LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG`; Windows `Unsupported`, so English) and `region()`. The parsing is pure and tested on every system.

### Fallback

Per message: the chosen language, then its parent if it ships (`pt-BR`, then `pt`), then English. The chain is three table indexes, computed once. English is complete by definition. `NOTCHLING_LOCALE_MARK_FALLBACK=1` wraps every message that fell back in `‹ ›`, so a translator sees in a render what is left.

### Numbers, dates, lists

- `NUMBER` and `t::fmt::count` use `icu_decimal` with a formatter built once (`OnceLock`), in the chosen language with the Mac's region: `en` with region `DE` formats as `en-DE` does, "1.234", which is what macOS does with its Region setting. The custom separators macOS also allows are not read.
- Plural categories come from `icu_plurals`, rules built once.
- A number that isn't a quantity (a year, a day of the month, a version, an id) is passed as a string, so it is never grouped.
- These are the Mac's, so they live in `mac/units.ftl` (call 4); the phone formats its own through Flutter's `intl`.
- Compact numbers and units are messages: `units-thousands = { $value }k` with `$value` from `t::fmt::decimal(12.3, 1)`, so a translator controls both the unit and the decimal separator. `chat/format.rs::tokens`, `age` and `duration` move to messages such as `units-age-minutes = { NUMBER($n) }m`.
- Dates are messages: `units-date-short = { $month } { $day }`, `units-date-long = { $month } { $day }, { $year } { $time }`, and `units-month-1` to `units-month-12`. `pages/dates.rs` keeps its arithmetic and formats through them. Full CLDR dates (`icu_datetime`, a few hundred KB of data) wait until the app reads the time zone.
- Lists join with `units-list-separator` (", "), as the code does today. ICU4X's list formatter can replace it if a language needs "a, b and c".

### Cost per draw

- A plain message is two array reads and a branch, then `&'static str`; nothing allocates. A draw may call it.
- A message with variables formats into a `String`. It is called where the model is computed (on change), never per row per frame, which is where today's `format!` calls already are. The model compares by `PartialEq` as before, so a part draws exactly as often as now.
- The active language is an `AtomicU8` read with `Relaxed`; the tables are `static`.
- `bench_chat.rs` and `bench_scroll.rs` hold the counts after every step, and `crates/AGENTS.md` gets the rule: a draw may call a plain message; a message with variables is formatted in a model compute.

### The pseudo-language

`NOTCHLING_LOCALE=en-XA` turns every English message into accented, 40% longer text in brackets ("⟦Àççôûñţš ~~~~⟧") at lookup, cached once per message. It needs no table and costs nothing when off. `NOTCHLING_LOCALE=en-XA notchling-app --render settings --out /tmp/s.png` shows what would clip in German before any German exists, and anything not in brackets is still hard-coded.

### Tests in the crate

Every shipped language formats every message with sample numbers (0, 1, 2, 3, 5, 11, 21, 101, 1,000) without a panic; `fluent-bundle` agrees with the tables on every message (numbers printed plainly on both sides); every plural category a translation uses exists in its language's CLDR rules (`icu_plurals`), so no variant is dead; the choice of language over a table of preferred lists; the fallback chain; `en` and `en-DE` number forms; the pseudo-language.

## What is never translated

| What | Why | How it stays so |
| --- | --- | --- |
| The `notchling` command line and its `--json` | Agents and scripts read it | `notchlingd` doesn't depend on `notchling-locale` |
| Skills, `AGENTS.md`, docs for agents | Agents read them | Not in `locales/` |
| Logs, the audit log, crash log, stored rows (`notchling.db`, `inbox` table) | Read by agents and the maintainer in any language; a row outlives a language change | They store keys and ids (an event kind, an inbox kind, an action id); the viewer turns them into words |
| Wire and protocol strings | Two sides of different versions and languages | Codes and ids go over the wire; free text there is English and never parsed |
| Part names (`.part("Send Button")`) | They name code an agent greps | `LINT-LOCALE-LITERAL` exempts them; the ? overlay may show the translated title beside them later |
| Text matched against another program (`prompt_flow.rs::NOT_PERMISSION`, the dialog markers, Claude Code's screen) | Translating it would break the match | Stays a constant in code, listed in the lint baseline, which only shrinks |
| The phone's dictation prompt and cleanup text (`features/voice/prompt_builder.dart`, `transcript_cleanup.dart`) | They steer Whisper and its cleanup in the language spoken, not the UI's (call 1) | Stay Vietnamese in code, in the lint baseline, until open question 3 is answered |
| Other tools' output | The app parses it | Child processes keep their locale (`LC_ALL=C` for git) |
| Brand names | They are names | `-brand-*` terms, defined once in English |

### Daemon text and the wire

- Step L2 makes the daemon's text English: the bridge's error messages, Visualize's empty states, takeover reasons, ptyhost's errors. `protocol/README.md` and the schema's description of `error.message` then say English. The phone shows its own words for `error.code` and the Mac's message as a detail. A localized empty state or reason, when a second language ships, comes as an additive optional code beside the text.
- Inbox items: the app and the phone already know each action's `id`, so `Allow Once` becomes `consent-allow-once` in both without a protocol change. A title needs the facts it was made from. In L6 all five kinds (secret approval, secret request, limit notice, action failed, notice) get an additive optional `code` and `facts` on the item (call 6): `{"names": [...], "command": "..."}` for a secret approval, rendered through `consent-secret-approval = Allow { $names } for { $command }` in `consent.ftl`; the other kinds' facts are what their titles are made from today (each kind's shape is left to the builder). Their titles go in `shared.ftl` under `item-` when both apps show them alike, else in each app's files. Free text the daemon has no code for (an error's message, a notice's body) stays English and shows as the item's detail, under the translated title. An older daemon without `code` shows its English title as today. The stored row keeps the English text for logs and older clients.

## The registries that are coming

Both are planned on other branches and both are Mac-only, so their words live in `mac/` files (call 4). Both are built after L1, so they name words by key from the start (call 2).

**Settings search.** Each setting's entry holds message constants, never text. The title is the same message the Settings row shows, so the row and the search result can't drift apart:

```rust
Entry { key: "hideNotch", title: t::settings::HIDE_NOTCH, about: t::settings::HIDE_NOTCH_ABOUT, words: t::settings::HIDE_NOTCH_WORDS }
```

In `locales/en/mac/settings.ftl`:

```ftl
## Settings: a title, one line about it, and search words, per setting.

settings-hide-notch = Show agents in the notch
settings-hide-notch-about = Sessions appear in the camera notch while agents work.
# Search words, separated by commas. Add words people in your language would type. English words are always searched too.
settings-hide-notch-words = notch, menu bar, camera, hide
```

Search matches the query against the title, the line and the words in the chosen language, and against the English words (`Text::english()`), so "notch" finds the setting in any language. Choice labels follow the same pattern: `settings-keep-awake-while-working = While Agents Work`.

**Log event kinds.** A log line stores the kind (`hook.permission`), never words. The viewer shows `logs-event-hook-permission` and `logs-event-hook-permission-about`, from `locales/en/mac/logs.ftl`, through `t::lookup`, since kinds are data. A test walks every kind in the registry and fails on one with no words; a kind from a newer daemon shows its id.

## The phone

Step L2, in two pull requests.

**L2a, the app:**

1. `locales/en/phone/<feature>.ftl` written from today's `app_en.arb` by a one-off script in the pull request (not kept). Keys get their feature's prefix (`pairTitle` becomes `pairing-title`, so `l10n.pairingTitle`). A text moves to `shared.ftl` or `consent.ftl` only where the Mac shows exactly the same text, case included; where the two apps' English differs, it stays two keys and neither app's words change (call 3).
2. Dart call sites renamed mechanically, in `lib/features/` and `lib/app/`.
3. Every user-visible literal outside `lib/l10n/` becomes a message too (call 1): the Vietnamese and English strings in `core/net/bridge_client.dart`, `features/chat/photo_send.dart`, `chat_notifier.dart`, `features/sessions/new_session_sheet.dart`, `features/visualize/viz_providers.dart` and `app/notifications.dart`.
   - `BridgeException` already carries a `code` (`view_only`, `unauthorized`, `not_found`, ...). The client's own codes (`disconnected` and the rest) lose their Vietnamese message; the UI maps every code to its own message (`bridge-error-disconnected`), and a daemon code it doesn't know shows the daemon's English message as a detail under a general one. `lib/core/net/` is outside the auto zone, so this part waits for the maintainer.
   - Code with no `BuildContext` (notifications, providers, notifiers) uses a context-free lookup: `lookupAppL10n(locale)`, which `flutter gen-l10n` already writes, through one holder (`lib/app/l10n_holder.dart` or similar) set at start from the platform's locales and again on `didChangeLocales`. Today that is always English; it matters from the second language on.
   - The dictation prompt and cleanup text (`features/voice/prompt_builder.dart`, `transcript_cleanup.dart`) stay Vietnamese and go in the lint's baseline (item 10).
4. `app_vi.arb` and `app_localizations_vi.dart` deleted. `gen.py` writes `app_en.arb`; `flutter gen-l10n` rewrites `app_localizations*.dart`.
5. `l10n.yaml`: `template-arb-file: app_en.arb`. `use-escaping` stays off, since the subset keeps braces out of phone text.
6. `app.dart` drops `localeResolutionCallback`; Flutter picks the first supported locale, English.
7. Tests: the harness defaults to `Locale('en')` and the 14 files that assert Vietnamese assert English. These are existing tests (`WEAK-TEST-EDIT`), so the maintainer approves them. The kit goldens already render English and should not change; if one does, the maintainer decides.
8. Plurals: the English forms stay as they render today (call 3). Where today's English is wrong ("1 subagents running"), the fix is its own pull request right after L2a, with before and after renders (call 8).
9. `apps/flutter/AGENTS.md`, rule 3 becomes: "Text wraps and grows (German is about 30% longer than English); targets are at least 48 dp. Words go in `locales/en/` (`shared.ftl`, `consent.ftl`, `phone/`); run `python3 tools/locale/gen.py`; never edit `lib/l10n/` by hand (`GUARD-LOCALE-GEN`)."
10. Dictation keeps Whisper's Vietnamese, its prompt and its cleanup text: it is the language you speak, not the app's (open question 3). `LINT-LOCALE-LITERAL` covers all of `lib/` once L2a lands, with those two files in its baseline.

**L2b, the daemon's words:** the Vietnamese text in `crates/engine/src/bridge/`, `viz/engine.rs`, `control/takeover.rs`, `crates/notchlingd/src/bridge_backend.rs` and `crates/ptyhost/` becomes English, with the tests that assert it and the protocol's description (regenerated by `tools/protocol/gen.py`).

## Checks and rules

New rows in `docs/rules.md`, each with a probe in `tools/guard/guard/rules.py` that fires on its bad example:

| ID | Rule | Why | Check |
| --- | --- | --- | --- |
| GUARD-LOCALE-GEN | The files in `crates/locale/src/generated.rs` and `apps/flutter/lib/l10n/*.arb` are exactly what `tools/locale/gen.py` writes from `locales/` | A hand-edited or stale file lets the apps show words no source holds | `python3 tools/locale/gen.py --check` |
| GUARD-LOCALE-SUBSET | Every `.ftl` stays in the subset, and keys, variables, file names and language tags follow their rules | A construct one target can't express breaks that app, maybe only later | `python3 tools/locale/gen.py --check` |
| GUARD-LOCALE-MATCH | A translation has no key English lacks, the same variables with the same types, and nothing English doesn't have: no link, address or markup, no value over three times English's length plus 20 characters | A translation must not be able to break the app, carry a link, or say more than English | `python3 tools/locale/gen.py --check` |
| LINT-LOCALE-LITERAL | In an area that has moved, no Rust or Dart literal looks like user-facing text: a word of two letters or more with a space, or a capital first letter. The phone is one area, all of `apps/flutter/lib/`, moved in L2a. Exempt: tests, part names (`.part`, `.part_at`, `.part_inside`, `.window_region`), element ids, log lines; what remains (text matched against other programs, the phone's dictation prompt and cleanup text) sits in the baseline | Moved areas stay moved | `python3 -m guard lint` |
| LINT-LOCALE-CONSENT | A warning: an English message outside `consent.ftl` that starts with Allow, Deny, Always Allow, Approve, Pair, Trust, Install or Sign In | Consent words belong where the maintainer reviews them | `python3 -m guard lint --warnings` |

Changed: `LINT-FILE-SIZE` exempts the generated locale files (above). Which areas have moved is `tools/locale/areas.json`, a list of areas with their code folders and a `moved` flag; it is protected, so an area can't quietly move back.

CI:

- `guard`: a "Locale" step runs `gen.py --check`, the generator's tests (`python3 -m unittest discover -s tools/locale/tests -t tools/locale`), and `gen.py --report --markdown >> "$GITHUB_STEP_SUMMARY"`, which shows each language's completeness per file on every pull request. Missing messages never fail it. `tools/test-python.sh` runs the generator's tests too.
- `phone-analyze`: `flutter gen-l10n`, then `git diff --exit-code -- lib/l10n`, so the Dart is current with the ARB.
- `rust-test`: the locale crate's tests on all three systems.

The completeness report:

```
Language  shared  consent  mac    phone  all    same as English
de        100%    100%     74%    90%    79%    41
```

## Zones: what the maintainer still reviews

`tools/guard/zones.json` gains, in its auto list: `locales/**`, then `!locales/languages.json` and `!locales/*/consent.ftl`; `crates/locale/src/generated.rs` (step L1); `apps/flutter/lib/l10n/**` (step L2a, once those files are generated, not before). `CODEOWNERS` is regenerated.

| Change | Zone | Why |
| --- | --- | --- |
| English words in an area file or `shared.ftl` | auto | Today these words are literals in `crates/app`, which is auto; moving them must not make a text fix wait |
| Generated Rust and ARB | auto | `GUARD-LOCALE-GEN` binds them to the source; a hand edit fails CI |
| A translation's area files and `shared.ftl`, in a language that ships or is being prepared | auto | The words can't break the app (the checks above), and can't carry a link or grow unbounded |
| A new language: adding it to `languages.json` | maintainer | It decides what every user of that language sees |
| `consent.ftl` in any language, English included | maintainer | A mistranslated Allow or Deny is a security problem no check can see |
| `tools/locale/`, the runtime in `crates/locale/`, `Cargo.toml`, `areas.json`, the guard | maintainer | As today for `tools/` and crates outside `app` and `kit` |
| A hidden character, an edited test | maintainer | `WEAK-UNICODE`, `WEAK-TEST-EDIT`, as everywhere |

So each Mac migration step waits for the maintainer once (it flips its area in `areas.json`); after it, text changes in that area merge as they do now. Auto here means what it means everywhere: merging on its own starts with Phase 5; until then the maintainer merges every pull request.

## Translating: the contributor guide

A section for `CONTRIBUTING.md` (protected, step L1), with style notes in `docs/translating.md`:

> ### Translating Notchling
>
> Notchling's words live in `locales/`. English (`locales/en/`) is the source; every other folder is a translation.
>
> 1. Copy `locales/en/` to `locales/<tag>/`, where the tag names your language: `de`, `fr`, `pt-BR`, `zh-Hans`. Or run `python3 tools/locale/gen.py --new de`.
> 2. Translate the text after each `=`. Keep the key on the left and every `{ $name }` (you may move it). Lines starting with `#` say where a text appears; leave them.
> 3. For a number, keep the `{ $count -> … }` block and give the forms your language needs: `zero`, `one`, `two`, `few`, `many`, `other`. Keep `*[other]`.
> 4. Don't translate product names (`{ -brand-notchling }`), commands or file names. Delete a message you don't translate; it shows in English.
> 5. Run `python3 tools/locale/gen.py` (Python 3.9 or later, nothing to install). It checks your files, writes the app files that come from them, and prints how much is translated. Fix what it reports.
> 6. For a new language, add its tag to `ship` in `locales/languages.json`.
> 7. Commit everything with `git commit -s` and open a pull request.
>
> The maintainer reviews a new language's first pull request and every change to `consent.ftl`, the words that allow or refuse something. Other changes to a language that ships follow the usual flow.

`docs/translating.md` covers: English's Title Case for menus is an English rule (follow your platform's style for your language); write whole sentences in each variant; spaces and punctuation belong in the message (French "Note :"); a message split because a part is styled or clickable says in its note which messages it is shown with, so translate them together; how to preview with `NOTCHLING_LOCALE=<tag>` and `NOTCHLING_LOCALE_MARK_FALLBACK=1`; and the meaning-changes-key rule for people editing English.

## Longer text, and right to left

German runs about 30% longer than English, and short labels much more ("Off" to "Aus" is fine; "Done" to "Erledigt" doubles). Layouts must not clip:

1. No fixed width around words, except a single-line label that elides with the kit's `elide()` and shows the whole text in its tooltip.
2. Buttons and segmented choices hug their content. A Settings choice row that doesn't fit becomes a popup menu.
3. Menus already hug up to `MENU_MAX_WIDTH`; a note or detail that may wrap goes in `menu_panel_wide`.
4. The notch's shapes have fixed sizes (`notch::layout::canvas_size`): text in the notch always elides.
5. A sentence is one message with variables, not pieces joined in code: word order differs. The exception (call 7): a sentence may be split where a part is styled or clickable ("Read the [guide] first"), decided per case in the migration pull request; each part's translator note names the messages it is shown with. A new line is a new message, never a `\n` inside one. No code changes the case of translated text.
6. Each migration step renders its area's views with `NOTCHLING_LOCALE=en-XA`; the pull request shows them. Renders in English are pixel-identical before and after the step (call 3), wrong plurals included. Where today's English plural is wrong ("1 subagents running"), the fix is its own pull request right after the step that moved the area, with before and after renders (call 8).
7. The phone already wraps and grows; new phone layout tests also run at the harness's `textScale: 1.3`.

Right to left (Arabic, Hebrew, Persian, Urdu) is not in this design. It would take: mirrored layout (GPUI 0.2.2 has no layout direction; the kit's `Corner` already says leading and trailing, a start), bidirectional caret movement and selection in the kit's text input and Chat's selection, mirrored icons (chevrons, arrows), isolation marks around variables at run time (a path inside Arabic text), and Flutter's `EdgeInsetsDirectional` where the phone uses `left`/`right`. Nothing here blocks it: the format and the generator carry right-to-left text unchanged.

## Steps

Named L0 to L8, so they don't collide with the project's Phase 5 and Phase 6. Each ships on its own and leaves the app working.

Order (call 2):

- Every step targets `agent-run-oss`, where the rebuild merges, through the integration branch `merge/settings-batch` first; none targets `main`.
- The branches in flight merge first: the ones in `merge/settings-batch` plus any open `agent/*` branch touching an area. An area's migration (L3 to L6) starts only after the branches touching that area have merged, so no pull request rewrites literals another branch is still changing.
- `tools/locale/areas.json` marks an area moved, arming `LINT-LOCALE-LITERAL` there, in the same pull request that moves it.
- L0 to L2 come before the Settings search registry and the log event-kind registry are built, so both are written with messages from their first line.
- A step that moves an area changes no English; the area's wrong plurals are fixed in the pull request right after it, with before and after renders (call 8).

| Step | What | Tests | Zone |
| --- | --- | --- | --- |
| L0 | This design accepted; decision record 0022 | none | protected (`docs/decisions/`) |
| L1 | The foundation: `locales/en/` (`shared.ftl` with brand terms, `language-name` and the few texts both apps show alike; `consent.ftl`), `languages.json`; `tools/locale/` (parser, checks, Rust and ARB writers, the iOS and Android writers off, report, `--new`, `--prune`); `crates/locale` (tables, `choose`, fallback, plurals, numbers, `fmt`, pseudo-language); `platform::locale`; every entry point calls `choose` at start (`mac/shell.rs`, `notchling-machines`, `--render`); guard rules and probes, zones, CODEOWNERS, `LINT-FILE-SIZE` exemption, CI step; `crates/locale/AGENTS.md`, notes in `tools/AGENTS.md` and `crates/AGENTS.md`, a feature-map row, `docs/translating.md`, the CONTRIBUTING section. The app looks the same | generator tests with golden outputs and every refusal; crate tests incl. `fluent-bundle` conformance; `cargo deny check`; guard probes | protected |
| L2a | The phone to English and generated ARB, every visible literal included, `BridgeException` codes mapped in the UI, the context-free lookup (above) | the changed phone tests, `flutter analyze`, `gen-l10n` freshness | protected (existing tests, `lib/app/`, `lib/core/net/`, `lib/l10n/` until this step adds it to the zone, `zones.json`, `areas.json`, `AGENTS.md`) |
| L2b | The daemon's text to English; the protocol's description | the changed engine and bridge tests; `tools/protocol/gen.py --check` | protected |
| L3 | Settings: `mac/settings.ftl`; Settings uses the `settings-*` words the search registry uses; the area moves | English renders pixel-identical; en-XA renders in the PR; `bench_chat.rs` | auto for the words and code; `areas.json` once |
| L4 | Chat, in a few pull requests by part (Sidebar, conversation, reply box, Details Panel, sheets) | as L3, plus `bench_scroll.rs` and the render checks (`render_blocks.rs`, `render_copy.rs`) | as L3 |
| L5 | The notch | as L3 | as L3 |
| L6 | The other windows: accounts, vault, phone pairing, pages, search, welcome, Machines, tools, menus, Dock and tray, the kit's words; the inbox's `code` and `facts` for all five kinds, and its action labels; the log viewer's event kinds (`mac/logs.ftl`) | as L3; inbox `facts` round trip in engine, local API and protocol tests | as L3; engine, local API and protocol parts protected |
| L7 | When a second language arrives: the Settings Language row, `CFBundleLocalizations` from `languages.json` in `tools/bundle-next.sh` (so macOS offers the per-app language and localizes its own panels), a font and casing pass for that language's script, then its translation | renders in that language; the Language row's model | protected (`tools/`); the translation per the zone table |
| L8 | Later: iOS and Android outputs turned on with a native phone app; right to left | per target | protected |

## Left to the builder

- The parser's exact handling of Fluent's whitespace and escapes within the subset; the conformance test is the referee.
- The generated Rust's exact layout (ids as `u16`, table types, `Num`'s conversions with saturation from `u64`), and Rust keywords as function names.
- The ICU4X calls and the binary-size cost of their compiled data, measured on a release build; over 1 MB added needs the maintainer's word.
- `LINT-LOCALE-LITERAL`'s heuristics for Rust and Dart, with good and bad examples, and each moved area's baseline.
- The phone's key renames, and which phone texts are exactly the Mac's (those alone go to `shared.ftl`).
- Which sentences split because a part is styled or clickable, per migration pull request.
- `flutter gen-l10n`'s handling of `#` inside a plural branch and of an apostrophe, pinned by a phone test.
- The xcstrings substitution form for a plural message with several variables, checked in Xcode when iOS is turned on.
- The `code` values and the shape of the inbox's `facts` for each of the five kinds.
- `platform::locale` on Linux; Windows stays `Unsupported` until a Windows build needs it.
- Font fallback and line height renders per script, when a language with that script arrives.

## Open questions

Only the maintainer can answer these.

1. **ICU4X as a direct dependency.** Plural rules and number formats come from `icu_plurals` and `icu_decimal` (Unicode-3.0). `deny.toml` allows Unicode-3.0 and ICU4X is already linked through `idna`, but decision 0008 says "only MIT, Apache and BSD crates", and these would be our first direct Unicode-3.0 dependencies. The alternative is `intl_pluralrules` (Apache/MIT, last released 2022, older CLDR) for plurals and English-only number formatting until a language needs more. **Recommendation:** take ICU4X and add a note to 0008 naming Unicode-3.0 as permissive, which is what `deny.toml` already says.
2. **The review line for translations.** Proposed: the maintainer approves a language's first pull request and every `consent.ftl` change; other changes to a shipped language's words merge like any auto-zone change once Phase 5 is on. The stricter choice is that every translation waits for the maintainer. **Recommendation:** the proposed line. Nobody here reads most languages, so reviewing every word would catch little the checks don't; consent is where a mistranslation does harm, and that stays with the maintainer.
3. **Dictation on the phone.** The phone's UI becomes English, but Whisper listens for Vietnamese with a Vietnamese prompt. Keep that, follow the UI language (English), or add a "Dictation Language" setting? **Recommendation:** keep Vietnamese now, since that is how the maintainer uses it, and add the setting when someone needs another language.
