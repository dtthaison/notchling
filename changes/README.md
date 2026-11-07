# Changelog fragments

Every pull request that changes app code adds one small file here. At release the files become the release
notes and the `CHANGELOG.md` entry, so nobody edits `CHANGELOG.md` by hand and two pull requests never
conflict on it.

## Writing one

Make `changes/<pr-number-or-short-slug>.md`:

```markdown
---
kind: fix
---
Replies containing an end-of-paste escape sequence are no longer typed into the session as keystrokes.
```

- `kind` is one of:
  - `feature`: something new a person can use. It makes the next Beta a new minor version and lands under
    Added.
  - `fix`: something that was wrong and now isn't. Lands under Fixed.
  - `breaking`: a change that can break stored data, the phone protocol or a setup. Lands under Changed,
    marked **Breaking**. The daily Beta refuses to cut while one is waiting; the maintainer must approve it.
  - `internal`: app code changed and users won't notice (a refactor, a test-only change to code under
    `Sources/`). It is not in the notes or the changelog.
- The text is one short paragraph in the voice of the changelog: plain, about what the user sees, no issue
  numbers needed.
- Nothing else goes in the front matter.

## Which pull requests need one

A pull request that changes `legacy/macos/Sources/**`, `apps/flutter/lib/**` or `crates/**/src/**` must add
a fragment, or the `fragments` check fails. Docs, tests, workflows and tools need none. When a change to
app code really needs no entry, the maintainer puts the label `no-changelog` on the pull request.

## What happens to them

- A Beta's notes are built from the fragments waiting here (`python3 tools/release/fragments.py notes`).
- Promoting a Beta to Stable folds them into `CHANGELOG.md` under the new version and deletes them
  (`python3 tools/release/fragments.py fold <version>`), in a pull request.

This file is not a fragment.
