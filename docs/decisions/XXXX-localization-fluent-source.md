# XXXX: One English source in Fluent, every platform's words generated from it

## Context

Notchling is going open source, and people who don't read English will use it. The structure for other languages has to exist before they arrive, so that a translation is text files and nothing else.

Today the product's words have no home:

- The Mac app holds about 2,000 English literals in `crates/app` and `crates/kit`.
- The phone keeps two hand-written ARB files, with Vietnamese as the template.
- The daemon writes Vietnamese text that the phone shows.
- A word both apps show is written twice.

The phone may later be rewritten natively (Swift, Kotlin), so the source can't be any one platform's format.

## Decision

The full design is [the localization spec](../superpowers/specs/2026-11-13-localization-design.md): the subset table, the generator's outputs, the runtime and the steps are there. What it decides:

Decided by the maintainer (approach A, 2026-11-13):

- Source format: Project Fluent (`.ftl`), restricted to a subset every target can express. A lint rejects the rest.
- Layout: `locales/<lang>/shared.ftl` (words both apps use), `mac.ftl`, `phone.ftl`, split further as the spec says.
- A generator, `tools/locale/gen.py`, Python standard library only, writes Rust for the Mac app with a compile error on an unknown key or wrong variables; the Flutter ARB files, replacing the hand-written ones; and, ready but off, iOS `Localizable.xcstrings` and Android `values-<lang>/strings.xml` with `<plurals>`. CI checks the generated files match the source, as `GUARD-PROTOCOL-GEN` does for the protocol.
- Every translation has the same keys and variables as English; a missing message falls back to English at run time; CI reports each language's completeness.
- Language follows macOS's preferred languages. A Settings "Language" row appears only once two or more languages ship.
- Not translated: the `notchling` command line, skills, anything agents read, logs and the audit log (they store keys, not text), wire and protocol strings.
- The phone becomes English only: its Vietnamese is dropped and English becomes the source.
- Migration area by area, one pull request each: Settings, then Chat, the notch, the other windows. A lint stops new hard-coded text in an area that has moved.

Calls settled after review (the coordinator, 2026-11-14):

1. Phone scope: every user-visible phone string becomes a message, including the hard-coded Dart literals outside `lib/l10n/`. `BridgeException` carries a code and the UI maps the code to its own message. Code without a `BuildContext` uses a context-free lookup. The dictation prompt and cleanup text are not UI and stay Vietnamese.
2. Base and order: every L-step targets `agent-run-oss`, through the integration branch first. An area's literal migration starts only after the branches touching that area have merged, and `areas.json` arms the lint for an area in the same pull request that moves it.
3. Shared versus per-app wording: a message goes in `shared.ftl` only when both apps show exactly the same text, case included. Where they differ they are two keys, one in `mac/` and one in `phone/`. English rendering stays pixel-identical.
4. Outputs follow file location: `shared.ftl` (and `consent.ftl`) go to both outputs; `mac/**` only to Rust; `phone/**` only to ARB. The generator does no usage scanning.
5. `choose` lives in the `notchling-locale` crate; every entry point calls it once at start, and all honor `NOTCHLING_LOCALE`.
6. Inbox: every kind gets a `code` and `facts`, so titles and action labels translate. Any free-text detail from the daemon stays English and is shown as detail.
7. Splitting a sentence into separate messages is accepted where a part is styled or clickable; a new line is a separate message; literal braces on the Mac are escaped by the generator. Each case is decided in its migration pull request.
8. A migration pull request keeps English pixel-identical, wrong plurals included. Plural fixes go in their own pull request right after the step that moved the area, with before and after renders.
9. The decision record's number is assigned at merge.

Decided by the user after review (2026-11-14):

1. ICU4X as a direct dependency: `icu_plurals` and `icu_decimal` (Unicode-3.0) are taken as direct runtime dependencies, for CLDR plural rules and number formats. [0008](0008-mit-permissive-crates.md) gets a note naming Unicode-3.0 as permissive. Not taken: `intl_pluralrules` with English-only number formatting.
2. The review line for translations: the maintainer approves a new language's first pull request and every `consent.ftl` change, in any language, English included. Other changes to a shipped language's words merge as auto-zone changes once Phase 5 is on. A new language's first pull request is caught because it must add the language to `locales/languages.json`, which is protected.
3. Dictation on the phone stays Vietnamese: Whisper's language `vi`, the Vietnamese prompt and the cleanup text, while the phone's UI becomes English. A "Dictation Language" setting comes later.

## Consequences

- A new top folder, `locales/`. Its zone is auto, except `languages.json` and `consent.ftl`, which wait for the maintainer.
- New code: `tools/locale/` (the generator and its checks) and `crates/locale/` (the Mac app's runtime).
- Three `GUARD-LOCALE-*` rules and two `LINT-LOCALE-*` rules in [docs/rules.md](../rules.md).
- The phone becomes English only, and the daemon's text becomes English. Older Macs still send Vietnamese, so the phone keeps matching the old marker.
- The first direct Unicode-3.0 dependencies; see the note on [0008](0008-mit-permissive-crates.md).
- Migration touches most of the app's views over several pull requests; each keeps English pixel-identical.

## Status

Accepted. The maintainer approved the design on 2026-11-14 *(decided)*.

## Date

2026-11-14
