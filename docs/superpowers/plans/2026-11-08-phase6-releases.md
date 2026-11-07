# Phase 6: Releases (the parts that need no signing identity yet)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** The Beta and Stable release pipeline, built and tested before the maintainer has a Developer ID, a notarization login or an update-signing key. It has:
- a version from tags;
- per-PR changelog fragments;
- a build job with no secrets;
- a separate signing and publishing job that runs no repository code;
- the Sparkle appcast for the Beta and Stable channels;
- a one-click Stable promotion of the same artifact;
- a kill switch.

Everything new is off until its secrets and the `NOTCHLING_RELEASES` variable exist. The current flow (`legacy/macos/release.sh` → `release/<version>` PR → `release.yml`) keeps working until the maintainer turns the new one on.

**Spec:** `docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md`, "Releases and updates" 1–7, "Phases" item 6.

**Blocked on the maintainer:**
- the Developer ID certificate and notarization credentials (`APPLE_*` secrets);
- the Sparkle EdDSA key (`SPARKLE_ED_PRIVATE_KEY`), backed up offline;
- adding Sparkle 2.6.4+ to the Swift app. The spec puts the Developer ID first, because the vault's Keychain item trusts the exact build. This is a protected change to the app the maintainer uses daily.
- setting `NOTCHLING_RELEASES=on`;
- the canary Macs;
- retiring `release.sh` and `release.yml`.

## Global Constraints

- Python 3.9+ standard library only, in `tools/release/`. Tests are in `tools/release/tests/`, run with `python3 -m unittest discover -s tools/release/tests -t tools/release`.
- Workflows:
  - Top-level `permissions: {}`, and per job only what is needed.
  - SHA-pinned actions only. A new action such as `actions/upload-artifact` or `download-artifact` is added to `tools/guard/actions-allowlist.json` with its full commit SHA, resolved with `git ls-remote`.
  - Event fields only through `env:`.
  - `guard pins` and `test_workflows.py` pass.
- Build, then sign apart:
  - The build job runs repository code with no secrets and no write token. It uploads the artifacts and their SHA-256 list.
  - The sign job checks out nothing from the repository. It runs only pinned actions and inline shell. It verifies the SHA-256 list, signs, notarizes, staples, attests (`actions/attest-build-provenance`, pinned) and publishes.
  - The same bytes go from Beta to Stable; promotion never rebuilds.
- Kill switch: the repository variable `NOTCHLING_RELEASES` must equal `on`, or every release and promotion job fails at its first step. Missing counts as off.
- Version from tags: `vX.Y.Z` is Stable and `vX.Y.Z-beta.N` is Beta. The build writes the version into the built app's `Info.plist` copy, never into the source `legacy/macos/Info.plist`.
- Tests never touch GitHub, Keychain, `codesign` with a real identity, notarytool or the network.

## Task A: Version and changelog fragments

**Files:** `tools/release/version.py`, `tools/release/fragments.py`, `changes/README.md`, `tools/release/tests/`.

- `version.py next --channel beta|stable`, given the tags from `git tag --list 'v*'` (passed in or read):
  - The next Beta after Stable `vX.Y.Z` is `vX.Y.(Z+1)-beta.1` unless a fragment of kind `feature` exists since the last Stable; then it is `vX.(Y+1).0-beta.1`. A `breaking` kind needs the maintainer: the command refuses without `--allow-breaking`.
  - Further Betas bump N.
  - Stable promotes a given Beta tag to its `vX.Y.Z`.
- `version.py plist <app> <version>` sets `CFBundleShortVersionString` and `CFBundleVersion` in a built bundle, using `plistlib`.
- Fragments are `changes/<pr-or-slug>.md`, with front matter `kind: fix|feature|breaking|internal` and one short paragraph.
  - `fragments.py check <base> <head>` fails when a PR changes app code (`legacy/macos/Sources/**`, `apps/flutter/lib/**`, `crates/**/src/**`) without adding a fragment, unless the PR has the label `no-changelog`, passed as an argument.
  - `fragments.py notes <since-tag>` assembles Markdown release notes grouped by kind.
  - `fragments.py fold <version>` moves the fragments into `CHANGELOG.md` under the version and deletes them; the Stable promotion runs it.
- Tests: version rules (the first Beta ever, after Stable, feature bump, refuse breaking, a malformed tag ignored), the plist rewrite on a temp bundle, fragment parse errors, check, notes and fold.

## Task B: Beta and Stable workflows

**Files:** `.github/workflows/beta.yml`, `.github/workflows/promote.yml`, `tools/release/tests/test_release_workflows.py`, and `tools/guard/actions-allowlist.json` (additions only).

- `beta.yml` runs on `schedule` (daily) and `workflow_dispatch`.
  - Job `gate` checks the kill switch, that `main` has merges since the last tag, and that `fragments.py` finds no `breaking` without approval.
  - Job `build` (macos, `contents: read`, no secrets) runs `./build.sh --dmg` and `version.py plist`, then uploads the dmg, the zip and `SHA256SUMS`.
  - Job `sign` (`contents: write`, `id-token: write`, `attestations: write`, environment `release`) checks out nothing:
    - it downloads the artifacts and verifies `SHA256SUMS`;
    - it fails closed when `APPLE_*` or `SPARKLE_ED_PRIVATE_KEY` is missing;
    - it signs with codesign, then notarizes, staples and attests;
    - it creates the prerelease `vX.Y.Z-beta.N` with the artifacts;
    - it updates the appcast (Task C) in the `appcast` release asset, or on the `gh-pages` branch (pick one and rule).
- `promote.yml` (`workflow_dispatch`, input `beta_tag`):
  - The actor must be in MAINTAINERS (from `tools/guard/guard/gate.py`, read by a step in a job that checks out the default branch with no secrets).
  - It checks the kill switch.
  - It downloads the Beta's assets and verifies the same SHA-256.
  - It creates the Stable release `vX.Y.Z` with those same bytes, moves the appcast item to the Stable channel, and opens a PR with the folded changelog (the App or `github-actions` makes the PR; it never pushes to main).
- Tests read the workflow text:
  - the sign job and the promote publishing job have no `actions/checkout` of the repository, and no `run:` invokes a repository script;
  - the build job has no secrets and only `contents: read`;
  - the kill switch is the first step of every job and compares to `on`;
  - env-only event fields;
  - the same-hash check is present in promote.

## Task C: The appcast

**Files:** `tools/release/appcast.py`, `tools/release/tests/test_appcast.py`.

- `appcast.py add` adds an item to a Sparkle 2 appcast XML:
  - version, short version, the enclosure URL, the length, and `sparkle:edSignature` (passed in, produced by the sign step);
  - `sparkle:channel` `beta` for Betas, and no channel element for Stable;
  - `sparkle:minimumSystemVersion` from `LSMinimumSystemVersion`;
  - `sparkle:phasedRolloutInterval` (default 86400);
  - `sparkle:criticalUpdate` only with `--critical`;
  - release notes as a link.
- `appcast.py promote <version>` drops the `beta` channel from that item, keeping the same enclosure bytes and signature.
- `appcast.py verify` checks the structure and that items are sorted.
- It uses `xml.etree` and is deterministic. Its output is checked against golden files in the tests.
- Ed25519 signing is done by Sparkle's `sign_update` in the sign job, not here. Python has no stdlib ed25519, so record that as a ruling.

## Task D: Docs

- `CONTRIBUTING.md` "Releasing", `docs/policies/automation.md`, `GOVERNANCE.md`, `ARCHITECTURE.md` and `docs/feature-map.md` cover: what exists, what is off, and the maintainer's turn-on steps in order (Developer ID, then notarization secrets, then the Sparkle key with an offline backup, then Sparkle in the app as its own protected PR, then `NOTCHLING_RELEASES=on`, then the first Beta, then canaries, then retiring `release.sh`).
- `changes/README.md` explains fragments.
- `.github/workflows/guard.yml` runs the `tools/release` tests.
- Not built yet: the upgrade and downgrade tests from the last Stable, and the launch-and-render smoke test. Record them as next steps; they need the release pipeline running.
- Final review on a fresh model, then the fix pass, then the merge into `agent-run-oss`.
