---
name: port-swift-part
description: Use when porting one part of the Swift app to Rust (an engine part E1 to E17 or a desktop app part U0 to U11 from the design's Parts tables), for example "port E1 transcripts" or "port U3 search". Tests and behavior fixtures first, then the code, then the ownership table.
---

Needs the Rust tree from Phase 4 (`crates/`, the Cargo workspace, `cargo-deny`). Parts are built in Phase 7, milestone by milestone. Before Phase 4, stop: there is nowhere to put the code.

A part is done when its ported tests pass and its behavior fixtures match the Swift app, not when the old fixtures parse.

1. **Pick the part, and check it's next.** Parts land in milestone order (`docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md`, "Milestones"). Milestone 1 is read-only: E1, E2, E12, E14, U0, U10. E3 waits for the recording phase, which captures terminal bytes for every reply behavior while the Swift app can still produce them. A protected part (E3 to E11, E13, E14, E16, E17) waits for the maintainer at merge.

2. **List what it covers.** In `docs/feature-map.md`, "Parts to areas" gives the areas; each area's row gives the Swift files, the tests and the fixtures. Read those files, the tests, and the matching parts of `Sources/AGENTS.md` ("Architecture in one pass", "Things that bit before"). Each "bit before" item for the area becomes a test.

3. **Collect behavior fixtures from the Swift app.** For each input fixture the Swift tests use (`Tests/NotchlingTests/Fixtures/`, `Tests/ConsoleCoreTests/Fixtures/`), record what the Swift code returns, as JSON, from a new test file (for example `Tests/NotchlingTests/<Part>BehaviorExportTests.swift`) that writes into a new fixture folder. Commit the inputs and the recorded outputs together. Don't edit an existing fixture; add new ones.

4. **Port the tests first.** Each Swift test case for the part becomes a Rust test with the same intent and a similar name, in the crate the part lands in:
   - engine parts: `crates/engine/` (transcript and search modules are in the auto-merge zone; the rest is protected), `crates/notchlingd/`, `crates/ptyhost/`, `crates/local-api/`;
   - app parts: `crates/app/`, `crates/kit/`, `crates/platform/`.

   Add one test per behavior fixture that compares the Rust output with the recorded Swift output. Run `cargo test -p <crate>` and see them fail.

5. **Write the code.** Keep the Swift app's limits: transcripts read only their tail (2 MB) and skip lines over 100 KB; files under about 400 lines. Only MIT, Apache or BSD crates. Zed's `terminal`, `terminal_view` and `markdown` are GPL-3; use `alacritty_terminal`, `pulldown-cmark` and `tree-sitter`. Web content opens in its own `wry` window.

6. **Make the tests pass.** Then make the behavior fixtures match. A difference is a bug in the port unless the Swift behavior was itself the bug. Then fix the Swift side first, reproduction-first (the `fix-bug-reproduction-first` skill), and re-record.

7. **Ownership.** If the part owns a single-owner resource (hook entries in `~/.claude/settings.json`, the hostd socket, bridge ports 47821/47822, `vault.sock`, limit watching, lanes and account switching), the Swift app and the Rust daemon must never both act on it. Update the ownership table in `ARCHITECTURE.md` with the milestone it moves in. Add the Swift app's stand-down setting for that area, as the bridge switch does, and test both sides of it. Both are protected.

8. **Run the checks.**
   ```sh
   cargo test
   cargo clippy --all-targets -- -D warnings
   cargo deny check
   swift test                                   # the Swift app still passes
   PYTHONPATH=tools/guard python3 -m guard lint
   ```

9. **Update the map.** Add the Rust path to the area's row in `docs/feature-map.md`. In the PR, list each Swift test and its Rust port, each behavior fixture and whether it matches, and any ownership change.

Tried: not yet
