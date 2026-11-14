# 0010: Build, then sign apart; Beta, then Stable

## Context

Agents will write most of the code that ships. One goal of the design: users get updates on their own, from Beta to Stable, without a release ever touching a signing key from code an agent wrote.

Today a release is manual. `./release.sh <version>` bumps `Info.plist` on a `release/<version>` branch and opens a PR; merging it tags `v<version>` and publishes the GitHub release (`.github/workflows/release.yml`). Builds are signed ad hoc, and there is no auto-update.

From the design: [Releases and updates](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md#releases-and-updates).

## Decision

1. **Beta, daily.** On a day `main` has new merges, the release workflow cuts a Beta. The version comes from tags at build time, not `Info.plist` edits. Notes come from per-PR changelog fragments. A breaking protocol or stored-data change needs the maintainer.
2. **Build, then sign apart** *(review)*. The build job runs repository code with no secrets and produces the artifacts. A separate signing job runs no repository code; it signs those exact artifacts, attaches provenance attestations and publishes. The same immutable artifact goes from Beta to Stable.
3. **Gates before Stable can be offered:** upgrade and downgrade tests from the last Stable (stored data stays readable by the previous release, so rollback is safe), a launch-and-render smoke test on each OS, and the maintainer's own machines running the Beta as canaries. A new Beta resets the soak only for the newer candidate; the older one can still be promoted.
4. **Stable by one click** *(decided)*. The maintainer promotes. It becomes automatic (a clean 3-day soak on a minimum number of Beta installs) only after a privacy policy and opt-in crash reports exist. A security fix has its own fast path.
5. **Updates.** One signed feed with Beta and Stable channels and phased rollout. Updates install only when the app is idle. The Swift app uses Sparkle 2.6.4 or later. The Rust app uses a static manifest we own, signed with ed25519 (minisign-style), and installers from Astral's `dist`. Zed's updater talks to zed.dev and isn't a reusable format; upstream cargo-dist is unmaintained.
6. **Kill switch.** A repository Actions variable stops every promotion and fails closed when missing. It isn't a file, since a red CI could block the PR that flips it.
7. **Keys.** The update signing key is backed up offline. A Developer ID (notarized builds) comes before auto-update is turned on.

## Consequences

- `./release.sh` and the `release/<version>` PR are replaced.
- Code an agent wrote never runs in a job that holds a key.
- Losing the update signing key strands every install, hence the offline backup.
- Notarization moves from a non-goal to a Phase 6 prerequisite. The vault's Keychain item trusts the exact build, so ad hoc updates would bring its access prompt back after every Beta.
- None of this exists yet. It is Phase 6.

## Status

Accepted. Stable by one click is the maintainer's decision *(decided)*. Building and signing apart, the kill switch as a variable, and the keys came from the reviews and are part of revision 2 of the design, which the maintainer approved on 2026-11-07.

## Date

2026-11-07

## 2026-11-14 note

The Rust app now updates with Sparkle and one EdDSA key shared with the worker manifest, instead of the separate ed25519 manifest planned above. See [XXXX](XXXX-sparkle-and-one-release-key.md) (proposed).
