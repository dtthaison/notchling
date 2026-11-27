# Phase 4: Repo Layout and Protocol Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** The repository has the target layout (the Swift app under `legacy/macos/`, the phone under `apps/flutter/`, a Cargo workspace under `crates/`), and the bridge protocol `agentconsole.v1` is written down as a JSON Schema with fixtures that the Swift Mac, the Dart phone and generated Rust and Dart types all pass.

**Architecture:** Task A moves the folders with `git mv` and updates every reference, so every check stays green. Tasks B and C then run in parallel:
- **B** creates the Cargo workspace: seven crates from the spec plus `crates/protocol`, CI on macOS, Linux and Windows, clippy, and `cargo-deny` with permissive licenses only.
- **C** writes `protocol/`: the schema, conformance fixtures, a stdlib Python generator for Rust (serde) and Dart types, and tests in Swift, Dart and Rust that read the same fixtures.

**Tech Stack:** git, Swift 5.9 (swift test), Flutter 3.44.8, Rust 1.99 (cargo, clippy, cargo-deny), Python 3.9+ stdlib.

**Spec:** `docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md`, "Architecture" (repo layout, Protocol), "Parts" (P1), "Phases" item 4; `docs/decisions/0006`, `0008`.

## Global Constraints

- The Swift app keeps building and testing exactly as before:
  - `swift test` from the repo root still works (the root `Package.swift` stays at the root, with paths into `legacy/macos/`);
  - `./build.sh`, `./build.sh --install` and `./build.sh --dev` still work from the root (the root `build.sh` becomes a one-line wrapper that runs `legacy/macos/build.sh`), producing the same app.
- The phone app keeps passing `flutter analyze` and `flutter test`, goldens included, from `apps/flutter/`.
- `agentconsole.v1` doesn't change. The schema describes what the Mac and the phone send today; it is additive only from here on.
- Protocol fields are additive only. The schema has `"version": "agentconsole.v1"`, and a new field is optional.
- Crates use only MIT, Apache-2.0 or BSD licenses (enforced by `deny.toml`). No Zed GPL crates.
- The phone stays free of Rust (record 0006).
- Python is stdlib-only and 3.9-compatible. The guard suite and every guard check stay green after each task.
- Tests never touch Keychain, real logins, ssh or `gh`.

## Review Focus

- **A reference to an old path left behind:** in workflows, zones.json, the lint and capability paths, weakening's TEST_PATHS, CODEOWNERS, AGENTS.md files, docs, scripts (`tools/install-hostd.sh`, `mobile/tool/*.sh` → `apps/flutter/tool/*.sh` with `../viz` → `../../viz`), `Demo/make-demo.sh`, `release.sh`, `.github/ISSUE_TEMPLATE`, the dev profile. Check: a test greps tracked text files for `Sources/Notchling`, `Sources/ConsoleCore`, `mobile/` and `Tests/NotchlingTests` outside `legacy/` and the history docs (`docs/superpowers/`, `docs/decisions/0001`, `CHANGELOG.md`).
- **The auto-merge zone shifting by accident in the move.** Before the move, list `in_zone` for every tracked file. After it, map each old path to its new one and compare: the zone must be the same set.
- **Generated types drifting from the schema.** A guard rule `GUARD-PROTOCOL-GEN` fails when regenerating changes the committed files.
- **A schema that describes an imagined protocol.** Fixtures are captured from the real Swift encoders (a Swift test writes them, then the fixtures are committed), not typed by hand. Every op in `BridgeRouter` has at least one fixture.
- **A dependency with a disallowed license.** `cargo deny check licenses` runs in CI, and its bad example is a test crate with a GPL dependency that the rule rejects.

---

### Task A: The move

**Files:** `git mv` of `Sources/`, `Tests/`, `Demo/`, `Icon/`, `Pet/` (if only the Swift app uses it), `Info.plist`, `release.sh` and `build.sh` → `legacy/macos/`; `mobile/` → `apps/flutter/`. Then a root `build.sh` wrapper, the root `Package.swift` paths, and every reference listed in Review Focus.

- [ ] Before moving, save `in_zone` for every tracked file to a temp JSON.
- [ ] Write `tools/guard/tests/test_layout.py`. It asserts:
  - the old paths are gone;
  - the new ones exist;
  - no tracked text file outside the history docs mentions the old paths;
  - the zone map is the same up to renames. The old map is saved as a fixture during this task.
- [ ] Watch it fail. Do the moves and update every reference.
- [ ] Verify:
  - `swift test`;
  - `./build.sh` (and that `build/Notchling.app` has the same bundle id and binary architectures);
  - `./build.sh --dev`;
  - `cd apps/flutter && flutter analyze && flutter test`;
  - the guard suite on both Pythons;
  - every guard check;
  - `test_manual_move.py`, which reads `Sources/AGENTS.md` and moves with it, so update its path constants. That's allowed in this one test because the file moved; record it as a ruling.

### Task B: Cargo workspace and Rust CI

**Files:**
- Create: `Cargo.toml` (workspace), `rust-toolchain.toml`, `deny.toml`, `crates/{engine,notchlingd,ptyhost,local-api,app,platform,kit,protocol}/` (each `Cargo.toml` and `src/lib.rs` or `src/main.rs` with one real test), `.github/workflows/rust.yml`
- Modify: `.github/rulesets/main.json` (required checks `rust-test`, `rust-clippy`, `rust-deny`), `tools/guard/guard/rulesets.py` and its tests if they pin the list, `tools/guard/actions-allowlist.json` (if a Rust action is needed; prefer none, since `rustup` is on the runners), `tools/guard/tests/fixtures` as needed

- [ ] `notchlingd` and `ptyhost` are binaries that print their version and exit. The others are libraries, each with one test that proves the crate builds and links.
- [ ] `crates/protocol` is an empty library that Task C fills; B only creates it.
- [ ] No GPUI yet: it comes with U0 in Milestone 1.
- [ ] `deny.toml` allows MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, Zlib, and denies everything else. It also checks advisories.
- [ ] Prove the license rule with a throwaway crate outside the workspace that depends on a GPL crate. `cargo deny check licenses` must reject it. Record the output; don't commit that crate.
- [ ] Install cargo-deny locally with `cargo install cargo-deny --locked`. CI does the same, pinned with `--version`.
- [ ] `rust.yml`:
  - triggers on `pull_request` and push to main;
  - `permissions: contents: read`;
  - checkouts use `persist-credentials: false`;
  - job `rust-test` runs a matrix (macos-latest, ubuntu-latest, windows-latest) of `cargo test --workspace`;
  - job `rust-clippy` runs `cargo clippy --workspace --all-targets -- -D warnings`;
  - job `rust-deny` runs `cargo deny check`;
  - actions are pinned (`guard pins` passes).
  - A matrix job reports one check per OS. So `rust-test` is required through a final `rust-test` job that `needs` the matrix and fails if any leg failed. Check this against `test_workflows.py`'s job-id parsing.
- [ ] Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`, the guard suite, `guard rulesets`, `guard pins`.

### Task C: Protocol spec, fixtures and generated types

**Files:**
- Create:
  - `protocol/README.md`, `protocol/agentconsole.v1.schema.json`, `protocol/fixtures/*.json`;
  - `tools/protocol/gen.py`, `tools/protocol/tests/`;
  - `crates/protocol/src/generated.rs`;
  - `apps/flutter/lib/core/protocol/generated.dart`;
  - a Swift test in `legacy/macos/Tests/ConsoleCoreTests/` (new file) and a Dart test in `apps/flutter/test/` (new file).
- Modify:
  - `crates/protocol/src/lib.rs` and `crates/protocol/Cargo.toml` (serde, serde_json; dev: none beyond std);
  - `tools/guard/guard/rules.py` and `docs/rules.md` (rule `GUARD-PROTOCOL-GEN`);
  - `.github/workflows/guard.yml` (run the generator check).

- [ ] Read `BridgeProtocol.swift`, `BridgeRouter.swift`, `Models.swift` (ConsoleCore) and `apps/flutter/lib/core/protocol/models.dart`. List every message type and op, with its fields and which side sends it.
- [ ] Write a Swift test (new file) that builds one real example of each message with the Swift types and encodes it. It compares the result with `protocol/fixtures/<name>.json`, or writes the file when `PROTOCOL_FIXTURES_WRITE=1`. Generate the fixtures that way and commit them. For messages only the phone sends, write them by hand from models.dart, and make the Swift test decode them with the Swift decoder.
- [ ] Write the JSON Schema (draft 2020-12) covering every fixture: required versus optional fields as the code treats them, and `additionalProperties: true`, since the protocol is additive.
- [ ] `tools/protocol/gen.py` reads the schema and writes `generated.rs` (serde structs and enums, `#[serde(default)]` on optional fields, `#[serde(rename_all = "camelCase")]` where the wire uses it) and `generated.dart` (plain classes with `fromJson` and `toJson`). Its output is deterministic. `gen.py --check` exits 1 when the files differ.
- [ ] Python tests for gen.py, including a tiny schema → expected output.
- [ ] Rust test: every fixture deserializes into its generated type and re-serializes to equal JSON, ignoring key order and field defaults.
- [ ] Dart test (new file): every fixture parses with `generated.dart`, and also with today's hand-written `models.dart`. That pins the two to the same wire. The phone keeps using `models.dart`; switching it to the generated types is a later, separate change.
- [ ] Validate each fixture against the schema in the Python tests. Write a small stdlib validator for the subset the schema uses (type, required, properties, enum, items, `$ref`, oneOf), with its own bad-example tests.
- [ ] Add rule `GUARD-PROTOCOL-GEN`, with a registry probe that alters a copy of the schema and expects `--check` to fail. List it in rules.md and run it in guard.yml.
- [ ] Run: `swift test`, `cargo test -p protocol`, `cd apps/flutter && flutter test`, the guard suite, `guard rules`.

### Task D: Integrate and review

- [ ] Run everything: swift test, build.sh, flutter analyze and test, cargo test, clippy, deny, the guard suite on both Pythons, and every guard check.
- [ ] Final whole-branch review on a fresh model, then the fix pass.
- [ ] Merge into `agent-run-oss` with `--no-ff`.
