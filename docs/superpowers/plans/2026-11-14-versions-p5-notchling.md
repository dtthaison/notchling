# Version sync, Phase 5: Notchling on Workers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every release carries `notchling` and `notchling-ptyhost` for four targets with a manifest signed by Sparkle's EdDSA key, and a rollout moves a worker's Notchling to the Mac's exact build: download from GitHub on the worker, check the SHA-256 the Mac sent, unpack under `~/.local/share/notchling/versions/<v>/`, move the `~/.local/bin/notchling` link, restart the daemon between ops. The Mac app updates itself with Sparkle once the Developer ID exists.

**Architecture:** Release side (maintainer only): per-target builds in `beta.yml`/`promote.yml`, `tools/release/worker_manifest.py` writing `notchling-manifest.json`, signed in the `sign` job with the EdDSA key that signs the appcast. Engine side: `versions::manifest` verifies the signature with a public key compiled into `notchling` (ring's Ed25519), maps a build string to its release tag, and `versions::adapters::notchling` writes the worker scripts. The restart goes through the worker's own daemon on the cluster's node link, so it waits for cluster v1 step 1.

**Tech Stack:** Rust (engine with `ring`, already a dependency), GitHub Actions (protected), Python 3.9+ standard library (`tools/release`), Sparkle 2 (the Mac app, after the Developer ID).

**Spec:** `docs/superpowers/specs/2026-11-13-version-sync-design.md`: sections 13 (Notchling row), 16, 21 (Downloads), 22 (Notchling), 23 (Phase 5), user's decision 4. Decision 0010 and Phase 0's record amending it.

**Branch:** `agent/versions-p5-notchling` (engine and tools), plus the maintainer's own release-path commits.

**Depends on:** Phase 4 (rollout engine, `ToolAdapter`); Phase 0's Sparkle record accepted; **cluster plan B** Tasks 10 to 13 (a daemon on each worker, the node tier, headless switches) for Tasks 5 and 6; the **Developer ID** for Task 7. Tasks 1 to 4 can start once Phase 1 is in.

## Global Constraints

- Phase 4's constraints hold. In addition:
- **Maintainer only:** `.github/`, `tools/`, `build.sh`, `tools/bundle-next.sh`, signing keys, the Developer ID. An agent drafts these files in the PR; nothing in this phase is merged or run by an agent, and no agent ever holds a private key.
- **The worker trusts only what the Mac sent:** the asset's URL (on `https://github.com/<owner>/notchling/releases/download/`) and its SHA-256 from a manifest whose signature the Mac checked. `curl --proto =https`; the hash is checked before unpacking.
- **The version is the build's own string** (`1.3.0-beta.2` stays that after promotion); the manifest maps build strings to tags; a rollout never names a tag.
- **Restart only between ops** and `notchling-ptyhost` only when it holds no terminal (decision 0004).
- Test keys are generated inside the tests (`ring::signature::Ed25519KeyPair::generate_pkcs8`); no key is committed.

## Review Focus

1. A manifest with a wrong signature, a wrong hash, or no entry for the build: the rollout refuses before anything reaches a worker. (Task 1 `wrong_signature_refuses`, `wrong_hash_refuses`, `unknown_version_refuses`.)
2. The `sign` job runs no repository code, and the key never leaves it. (Task 3, reviewed by the maintainer.)
3. A worker's daemon restarts while a terminal is open: ptyhost isn't restarted until it holds none. (Task 5 `ptyhost_waits_for_no_terminal`.)
4. Rollback moves the link back and restarts; two versions are kept. (Task 4 `rollback_moves_the_link_back`, `keeps_two_versions`.)

---

### Task 1: The manifest and its signature

**Zone:** maintainer (`crates/engine`). No CAP.

**Files:**
- Create: `crates/engine/src/versions/manifest.rs`
- Test: `crates/engine/src/versions/manifest_tests.rs`

**Interfaces:**
- Produces: `pub struct Manifest { pub version: Version, pub tag: String, pub assets: BTreeMap<String, Asset> }`, `Asset { name, size: u64, sha256: String, url: String }` (target triples as keys: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`); `pub fn verify(bytes: &[u8], sig: &[u8], public_key: &[u8; 32]) -> Result<Manifest, ManifestError>` (Ed25519 over the exact bytes; then parse; every URL on the GitHub releases prefix and https); `pub const RELEASE_KEY: [u8; 32]` (the maintainer fills it in Task 3; zeros until then, and `verify` with a zero key always refuses); `pub fn for_target(&self, target: &str) -> Option<&Asset>`.
- [ ] **Step 1: Write the failing tests:** `good_signature_parses`, `wrong_signature_refuses`, `wrong_hash_refuses` (an asset hash that differs from what the worker computes: tested through Task 4's verify), `unknown_version_refuses`, `non_github_url_refuses`, `zero_key_always_refuses`, `beta_build_maps_to_its_stable_tag`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::manifest`. Expected: FAIL.
- [ ] **Step 3: Implement** with `ring::signature::UnparsedPublicKey::new(&ED25519, key)`. **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `versions: verify Notchling's signed release manifest`.

### Task 2: The worker manifest writer

**Zone:** maintainer (`tools/release`). No CAP in Rust; a protected folder.

**Files:**
- Create: `tools/release/worker_manifest.py`, `tools/release/tests/test_worker_manifest.py`

**Interfaces:**
- Produces: `python3 tools/release/worker_manifest.py --version <build> --tag <tag> --repo <owner/name> <asset>...` writes `notchling-manifest.json` (sorted keys, each asset's name, size, SHA-256 and URL) to stdout; refuses an asset name that doesn't match `notchling-<build>-<triple>.tar.gz`.
- [ ] **Step 1: Write the failing tests** (`unittest`, temp files): `writes_sorted_manifest`, `refuses_misnamed_asset`, `hash_matches_the_file`.
- [ ] **Step 2: Run** `PYTHONPATH=tools/release python3 -m unittest tools/release/tests/test_worker_manifest.py` (and then `tools/test-python.sh`). Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `release: a manifest of the worker builds`.

### Task 3: Per-target builds and signing (maintainer)

**Zone:** maintainer only (`.github/workflows/beta.yml`, `promote.yml`, `release.yml`). `GUARD-PINS` applies to any new action.

**Files:**
- Modify: `.github/workflows/beta.yml` (a matrix job building `cargo build --release --locked -p notchling-notchlingd -p notchling-ptyhost --target <triple>` for the four triples; Linux on `ubuntu-latest` with the `aarch64` cross linker or a native arm runner; tarballs named per Task 2), the `sign` job (downloads the manifest artifact, signs it with the Sparkle EdDSA key from the repository secret the appcast already uses, uploads `notchling-manifest.json.sig`; runs no repository code), `promote.yml` (copies the manifest and signature to the Stable release unchanged), `crates/engine/src/versions/manifest.rs` (`RELEASE_KEY` = the public half)
- [ ] **Step 1:** The agent drafts the YAML in the PR; the maintainer reviews pins, runners and that the signing step reads only the artifact.
- [ ] **Step 2:** `python3 -m guard pins` and `python3 -m guard rulesets`. Expected: clean.
- [ ] **Step 3:** The maintainer runs a Beta and checks: four tarballs, the manifest, its signature; `notchling --version` from each tarball prints the build string.
- [ ] **Step 4: Commit** (maintainer) `release: worker builds and a signed manifest`.

### Task 4: The Notchling adapter

**Zone:** maintainer (`crates/engine`). No CAP (script text).

**Files:**
- Create: `crates/engine/src/versions/adapters/notchling.rs`
- Test: `crates/engine/src/versions/adapters/notchling_tests.rs`

**Interfaces:**
- Produces: `Notchling: ToolAdapter` (method `Release` only): `install` takes the `Asset` the Mac verified and writes: `curl --proto =https -fsSL <url> -o <tmp>`, check `sha256` equals, unpack into `~/.local/share/notchling/versions/<v>/`, `notchling --version` from there equals `<v>`, `ln -sfn` the link; `restore` moves the link back to the previous version folder; keeps the current and one previous version, removing older folders; `verify` reads the link and `--version`.
- [ ] **Step 1: Write the failing tests:** `install_checks_hash_before_unpack`, `install_moves_the_link_last`, `rollback_moves_the_link_back`, `keeps_two_versions`, `urls_are_github_https_only`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::adapters::notchling`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** **Step 5: Commit** `versions: install Notchling's own build on a worker`.

### Task 5: Restart between ops, through the node link

**Zone:** maintainer (`notchlingd`, `crates/engine/src/cluster`). `CAP-PROCESS` (`launchctl`/`systemctl`). **Waits for cluster plan B Tasks 10 to 13 and its headless switch policy (Task 11).**

**Files:**
- Create: `crates/notchlingd/src/versions_self.rs`
- Test: `crates/notchlingd/src/versions_self_tests.rs`
- Modify: `crates/engine/src/cluster/policy.rs` (a row: the home may ask a member `node.restart_when_idle`; nobody else), `crates/local-api/src/wire.rs` (node-only op `NODE_RESTART_WHEN_IDLE`)

**Interfaces:**
- Produces: the worker's daemon answers `node.restart_when_idle {version}` at once, then waits until no op is in flight and restarts itself (`launchctl kickstart -k gui/<uid>/dev.notchling.daemon` or `systemctl --user restart notchling`), and restarts `notchling-ptyhost` only once it holds no terminal. The rollout's Moving step for Notchling sends it and then waits for the node's next `hello` with the new version.
- [ ] **Step 1: Write the failing tests** (a fake launcher, a fake in-flight counter): `restarts_only_between_ops`, `ptyhost_waits_for_no_terminal`, `only_the_home_may_ask`, `hello_with_new_version_finishes_the_step`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib versions_self`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** **Step 5: Commit** `notchlingd: restart for a new build only between ops`.

### Task 6: Notchling in Machines' rows and the rollout

**Zone:** `crates/engine/src/machines` maintainer; `crates/app` auto. No new CAP.

**Files:**
- Modify: `crates/engine/src/machines/rollout_host.rs` (the Mac downloads and verifies the manifest before the sheet: `Fetch` over the Machines runner's local `curl`, a `CAP-NETWORK` line), `crates/app/src/machines/versions_model.rs` (Notchling's row), `crates/app/src/sheets/settings_view.rs` (the General page's Notchling row gets the Workers lines)
- Test: `crates/engine/src/machines/tests_rollout_notchling.rs`, `crates/app/src/machines/versions_model_notchling_tests.rs`

- [ ] **Step 1: Write the failing tests:** `sheet_needs_a_verified_manifest`, `unverified_manifest_has_no_sheet`, `notchling_row_shows_build_strings`.
- [ ] **Step 2: Run** both filters. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** **Step 5: Commit** `machines: roll Notchling's own build out to workers`.

### Task 7: Sparkle for the Mac app (after the Developer ID)

**Zone:** maintainer only (`tools/bundle-next.sh`, `crates/app/Cargo.toml` if a binding crate is needed, the app's `welcome/updates.rs` is auto). `CAP-NETWORK`, `CAP-PROCESS`, `CAP-DYNAMIC` if the framework is loaded at run time.

**Files:**
- Modify: `tools/bundle-next.sh` (embed Sparkle.framework, `SUPublicEDKey`, `SUFeedURL` = the existing appcast), `crates/app/src/welcome/updates.rs` (hand the check to Sparkle when the kill switch is on; install only when the app is idle, decision 0010)
- Test: `crates/app/src/welcome/updates_sparkle_tests.rs` (the idle rule and the kill switch, pure)

- [ ] **Step 1:** Waits for the Developer ID (maintainer). The agent drafts the bundle changes in a draft PR.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib welcome::updates_sparkle`. Expected: PASS after implementation.
- [ ] **Step 3:** The maintainer signs, notarizes and runs one update from a Beta to the next on his Mac.
- [ ] **Step 4: Commit** (maintainer) `app: Sparkle updates the Rust app`.

### Task 8: Docs

**Zone:** maintainer. **Files:** `crates/AGENTS.md`, `crates/engine/AGENTS.md`, `docs/feature-map.md`, `docs/policies/privacy.md` (the Mac fetches the manifest; workers download from GitHub), `docs/policies/automation.md` (the release section: the worker assets and their manifest), `CHANGELOG.md`.
- [ ] **Step 1: Run** the full checks (fmt, clippy, workspace tests, guard lint/featuremap/rules/pins, `tools/test-python.sh`). **Step 2: Write** the docs. **Step 3: Commit** `docs: Notchling on workers`.

### Checkpoint (maintainer)

One Beta with the four tarballs and a signed manifest; one Linux worker that joined the cluster rolled from build A to build B and back with Roll Back; its terminal sessions survived both.
