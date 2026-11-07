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
    marked **Breaking**. It starts a new major line: the first Beta on it needs the maintainer's click ("Allow a breaking change"); later Betas on that line need nothing, and a Stable never refuses.
  - `internal`: app code changed and users won't notice (a refactor, a test-only change to code under
    `Sources/`). It is not in the notes or the changelog.
- The text is one short paragraph in the voice of the changelog: plain, about what the user sees, no issue
  numbers needed.
- Nothing else goes in the front matter.

## Which pull requests need one

A pull request that changes `legacy/macos/Sources/**`, `apps/flutter/lib/**` or `crates/**/src/**` must add
a fragment. Docs, tests, workflows and tools need none. When a change to app code really needs no entry,
the maintainer puts the label `no-changelog` on the pull request.
`python3 tools/release/fragments.py check --base main --head HEAD` fails when yours is missing one.

Not in force yet:

- CI doesn't run that check, and fragments replace the `[Unreleased]` lines in `CHANGELOG.md` only once the
  Beta flow is on ([CONTRIBUTING.md](../CONTRIBUTING.md#releasing)).
- `changes/` is outside the auto-merge zone, so an agent's pull request that adds a fragment waits for the
  maintainer. Whether to open it to new fragments is his decision
  ([docs/policies/automation.md](../docs/policies/automation.md#fragments-on-pull-requests)).

## What happens to them

- A Beta's notes are built from the fragments waiting here that are not already in the last Stable
  (`python3 tools/release/fragments.py notes`), and only those count toward the Beta's version.
- Promoting a Beta to Stable folds the fragments present at that Beta's commit into `CHANGELOG.md` under
  the new version and deletes them (`python3 tools/release/fragments.py fold <version>`), in a pull
  request. Fragments merged after that Beta stay for the next release.

This file is not a fragment.
