# XXXX: Sparkle for the Rust app, and one release key

## Context

[Decision 0010](0010-build-then-sign-beta-then-stable.md) planned a separate ed25519 manifest to update the Rust app. The app's update check exists and is off (`welcome/updates.rs`). The Swift app already updates with Sparkle, from an appcast made by `tools/release/appcast.py` and signed with an EdDSA key.

Version sync ([design](../superpowers/specs/2026-11-13-version-sync-design.md), section 16) also needs workers to follow the Mac's Notchling build. A worker runs no app, only `notchling` and `notchling-ptyhost`, so each release must carry per-target tarballs and a signed manifest the Mac can check before it tells a worker what to download.

## Decision

- **The Rust app updates with Sparkle 2** from the existing appcast, and installs an update only when the app is idle (0010's rule). 0010's separate ed25519 manifest for the app is dropped.
- **One key.** The same EdDSA key signs the appcast and `notchling-manifest.json` for workers. One key, one offline backup, held by the maintainer.
- **The public key** is compiled into `notchling` and set as `SUPublicEDKey` in the app.
- **The `sign` job runs no repository code**, as 0010 requires. It holds the private half as a repository secret.
- **Sparkle waits for the Developer ID**, as releases already do (0010: notarization is a prerequisite, since the vault's Keychain item trusts the exact build).
- **Workers get per-target tarballs** of `notchling` and `notchling-ptyhost` (`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`). They download them from GitHub over https, with the hash the Mac sent after it checked the manifest's signature. A worker trusts only what the Mac sent.
- **The manifest's version is the build's own string.** A promoted Stable keeps its Beta's version (`1.3.0-beta.2`), so a rollout matches the Mac's build string, never the release tag. The manifest maps one to the other.

## Consequences

- It amends 0010: its Rust-app update plan (a separate ed25519 manifest) is replaced. Everything else in 0010 stands: build, then sign apart, Beta daily, Stable by one click, the offline backup, the kill switch.
- A lost key means a new key and a release that carries both public keys for one cycle, and it strands installs that miss that cycle. A leaked key means the same, plus revoking the old appcast entries.
- The release path (`beta.yml`, `promote.yml`, `release.yml`, `tools/release/*`) changes, and that is the maintainer's work.
- None of it exists yet. Worker installs of Notchling come in Phase 5, once the Developer ID exists and cluster v1 gives each worker a daemon.

## Status

Proposed. The maintainer settles it by accepting and merging it. It amends [0010](0010-build-then-sign-beta-then-stable.md). The number is a placeholder, assigned at merge.

## Date

2026-11-14
