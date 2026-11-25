# Translating Notchling: style notes

The steps are in [CONTRIBUTING.md](../CONTRIBUTING.md#translating-notchling). These notes are for the choices a translator meets while doing them.

## Where the words are

`locales/en/` is the source. `shared.ftl` holds the words the Mac and the phone show alike (the language's own name, brand terms, states). `mac/*.ftl` holds the Mac app's, `phone/*.ftl` the phone's. `consent.ftl` holds words that allow or refuse something (Allow Once, Deny, Pair, Trust Folder); the maintainer reads every change to it.

Each message has a `#` note above it saying where it appears. Read it before you translate: the same English word can need two words in your language.

## Style

- English menus use Title Case ("Open Settings"). That is an English rule. Follow your platform's style for menus and buttons in your language.
- Write whole sentences in each variant of a plural. Don't build a sentence from a number plus a word that you attach in code or by reusing another message: word order and endings differ.
- Spaces and punctuation belong in the message. French writes "Note :" with a space before the colon; put it in the message.
- Some texts are split because one part is styled or clickable. The note of each part says which messages it is shown with. Translate those together, so they read as one sentence.
- Keep every `{ $name }` and `{ -brand-… }`. You may move them to where your language wants them.
- Don't translate product names, commands, file names or anything the note says is data.
- For a number, give the plural forms your language needs (`zero`, `one`, `two`, `few`, `many`, `other`) and keep `*[other]`. The generator tells you when a form your language needs is missing.
- A message you don't translate shows in English. Delete the line rather than copying English into it.

## Preview

Run the app with `NOTCHLING_LOCALE=<tag>`, for example `NOTCHLING_LOCALE=de`. Only a language listed under `ship` in `locales/languages.json` can be chosen; a `draft` language is for the files, not the app, until the maintainer ships it.

`NOTCHLING_LOCALE_MARK_FALLBACK=1` wraps every message that fell back to English in `‹ ›`, so you can see what is left. `NOTCHLING_LOCALE=en-XA` is the pseudo-language: accented, 40% longer, inside `⟦ ⟧`. It shows text that is hard-coded (no accents) and text that doesn't fit (cut off), whatever language you translate.

`python3 tools/locale/gen.py --report` prints how complete each language is, `--report --lang de --missing` lists what de still lacks, and `--prune` removes keys English no longer has.

## Editing English

A message's key names its meaning. If you change what an English message means (not just how it is worded), give it a new key and delete the old one, so each translation is marked missing and gets translated again. Fixing a typo or wording that keeps the meaning keeps the key.
