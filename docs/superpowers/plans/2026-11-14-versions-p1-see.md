# Version sync, Phase 1: See Versions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Notchling knows, without starting any tool, which version of Claude Code, Codex, Antigravity and Notchling the Mac has installed and with what binary, and which versions each worker in Machines runs. Machines and Settings > Agents show both, with a mark per tool (same, behind, ahead, Not Installed, Can't Pin), and a pure mismatch check exists for the cluster's handshake and `delegate --machine` to warn with. Nothing installs, writes a worker or rolls out.

**Architecture:** A pure `engine::versions` (tool, version, method, the version pattern, CLI output parsers, binary identity, Mac detection over a root, the worker probe's parser, marks, mismatch). The daemon reads the Mac's files with `platform::watch` and a 10-minute re-read and serves `versions.get` (agent tier) and the `versions` topic. Machines adds a read-only **Tool Versions** step to its batched probe, saves each worker's facts in `notchling.db` (`versions/workers`), and reads the Mac's side from the daemon; with no daemon it says "Notchling isn't running". Settings > Agents gets a read-only Client part.

**Tech Stack:** Rust (engine, notchlingd, local-api, GPUI app with `crates/kit`), `sha2` (already an engine dependency), SQLite through `notchling_engine::store`.

**Spec:** `docs/superpowers/specs/2026-11-13-version-sync-design.md` (branch `agent/version-sync-design`, 344d8a5): sections 3, 5, 6, 7, 17.1, 17.2, 17.5, 18, 19, 21, 22 (Parsing, Mac detection, Mismatch, Tool list and drift), 23 (Phase 1), and coordinator's calls 1, 4 and 10. Read the overview `2026-11-14-versions-overview.md` first.

**Branch:** `agent/versions-p1-see`, off `origin/merge/settings-batch` (or `agent-run-oss` once that batch is merged into it).

## Global Constraints

- No comments in code, doc comments included; the why goes in the crate's `AGENTS.md` (`crates/engine/AGENTS.md`, `crates/notchlingd/AGENTS.md`, `crates/app/AGENTS.md`) and `crates/AGENTS.md`.
- Package names: `notchling-engine`, `notchling-notchlingd`, `notchling-local-api`, `notchling-app`. One test filter per `cargo test` run. Give each parallel lane its own `CARGO_TARGET_DIR`.
- Nothing in this phase starts a Claude Code, Codex or Notchling process on the Mac. The one exception is `agy --version`, run by the daemon at most every 10 minutes, only when no file gives agy's version (Task 5, `CAP-PROCESS`).
- Tests never run ssh, an installer, a real CLI or the network, and never read the real `~/.claude`, `~/.codex`, `~/.gemini` or `~/.local`. Roots are parameters; temp homes come from `TempDir`/`Scratch` (`testutil.rs`). Every remote call goes through `ScriptedRunner`; every local command through `FakeLauncher`.
- A version string is kept only if it matches `^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$` and is at most 32 characters. Anything else is "can't tell", never a version. A path from a worker is kept only if it passes `shell::is_safe_machine_path`.
- The local API changes additively only. `versions.get` is agent tier (`OpSpec::AGENT`), `versions.workers` app tier and quiet (`OpSpec::APP_QUIET`).
- Machines stays a separate binary (decision 0004): it never reads the Mac's tool files itself; it asks the daemon.
- Files under 1,000 lines (600 warns); tests in `*_tests.rs` / `tests_<area>.rs`. Every new folder under `crates/*/src/` gets its `docs/feature-map.md` row in the task that makes it (`GUARD-FEATURE-MAP`).
- Views use kit tokens and `.part("…")` tags; Title Case for titles; text short and plain.
- No log text: facts go through `versions::record::Recorder` (Task 5), whose daemon side writes `audit.jsonl` lines with fixed words until the logging design's bus lands (overview, "Logging").

## Review Focus

1. Nothing is started to read a Mac version: the launcher fake records no call for Claude Code, Codex or Notchling, and agy's at most once per 10 minutes. (Task 3 `detect_starts_nothing`, Task 5 `agy_version_asked_at_most_every_ten_minutes`.)
2. A hostile worker output never becomes a version or a path: a version with `;`, a 10 KB line, a vault value, a path with `..` or a newline. (Task 1 `hostile_text_is_never_a_version`, Task 6 `hostile_probe_output_keeps_nothing`.)
3. With no daemon, Machines shows "Notchling isn't running", draws no mark against the Mac and offers nothing. (Task 8 `no_daemon_shows_not_running_and_no_marks`.)
4. A tool a worker lacks is Not Installed, never "behind" and never a mismatch. (Task 4 `absent_tool_is_never_a_mismatch`, Task 8 `absent_tool_shows_not_installed`.)
5. The Tool Versions probe is read only and runs no Fix: it has no `StepFix::Run`, and Set Up All doesn't contain it. (Task 6 `tool_versions_has_no_fix_and_is_not_in_set_up_all`.)

---

### Task 1: Tool, version, method and the CLI output parsers

**Zone:** maintainer (`crates/engine`). No CAP.

**Files:**
- Create: `crates/engine/src/versions/mod.rs`, `crates/engine/src/versions/version.rs`, `crates/engine/src/versions/method.rs`, `crates/engine/src/versions/parse.rs`
- Test: `crates/engine/src/versions/version_tests.rs`, `crates/engine/src/versions/parse_tests.rs`, `crates/engine/src/versions/method_tests.rs`
- Modify: `crates/engine/src/lib.rs` (`pub mod versions;`), `docs/feature-map.md` (a row for `crates/engine/src/versions/`)

**Interfaces:**
- Produces:
  - `pub enum Tool { Claude, Codex, Agy, Notchling }` (serde `snake_case`: `claude`, `codex`, `agy`, `notchling`; `Ord`), `fn title(self) -> &'static str` ("Claude Code", "Codex", "Antigravity", "Notchling"), `fn command(self) -> &'static str` (`claude`, `codex`, `agy`, `notchling`), `const ALL: [Tool; 4]`.
  - `pub struct Version(String)` (serde transparent): `fn parse(s: &str) -> Option<Version>` (the pattern above, at most 32), `fn as_str(&self) -> &str`, `Display`, `Ord` comparing the three numbers, then a prerelease below its release, then prerelease text.
  - `pub enum Method { Native, Standalone, Npm, Brew, Apt, Release, Unknown }` (serde `snake_case`), `fn title(self) -> &'static str` ("native installer", "standalone installer", "npm", "Homebrew", "apt", "release", "unknown"), `fn from_path(tool: Tool, path: &str, link: Option<&str>) -> Method`: `.local/share/claude/versions/` is Native, `node_modules` is Npm, `Caskroom` or `Cellar` is Brew, `.codex/packages/standalone` is Standalone (Codex), `.local/bin/agy` is Standalone (agy), `.local/share/notchling/versions/` is Release, else Unknown.
  - `fn can_pin(tool: Tool, method: Method) -> bool`: Native (Claude), Standalone (Codex), Npm (Claude, Codex), Release (Notchling) are true; Brew, Apt, Unknown false; agy Standalone false (it installs only the latest; Phase 4 handles it).
  - `pub fn cli_output(tool: Tool, text: &str) -> Option<Version>`: the first line only, at most 200 characters read; `2.1.322 (Claude Code)`, `codex-cli 0.166.0`, `agy 1.4.2` and `notchling 0.9.0` each give the number; anything else `None`.
- Cluster B (plan `2026-11-13-cluster-v1-b-nodes.md`, Task 3) creates these files if it lands first. Whichever lands second uses the other's names and adds what's missing; B's `Version` pattern is looser, and this one (spec §3) wins, since a version enters scripts.

- [ ] **Step 1: Write the failing tests:**
  - `version_tests.rs`: `accepts_plain_and_prerelease` (`2.1.324`, `1.3.0-beta.2`), `rejects_long_or_odd` (33 characters, `2.1`, `v2.1.3`, `2.1.3;rm`, `2.1.3 `, `stable`, `latest`), `orders_numerically` (`2.1.89 < 2.1.322`), `prerelease_is_below_release`.
  - `parse_tests.rs`: `reads_each_tool` (the four forms above), `hostile_text_is_never_a_version` (`2.1.3; rm -rf ~`, a 10 KB line of digits, a line with `\u{2028}`, a planted vault value `sk-ant-oat01-XXXX`), `reads_only_the_first_line`.
  - `method_tests.rs`: `paths_map_to_methods` (one case per row above, including an npm Codex at `/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js` and a brew cask at `/opt/homebrew/Caskroom/claude-code/2.1.324/claude`), `brew_and_agy_cannot_pin`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::`. Expected: FAIL (no module).
- [ ] **Step 3: Implement** the types and parsers in the three files; `mod.rs` re-exports `Tool`, `Version`, `Method`.
- [ ] **Step 4: Run** `cargo test -p notchling-engine --lib versions::`, then `PYTHONPATH=tools/guard python3 -m guard featuremap`. Expected: PASS, clean.
- [ ] **Step 5: Commit** `engine: tool versions, methods and their parsers`.

### Task 2: Binary identity

**Zone:** maintainer (`crates/engine`). No CAP (reads files only).

**Files:**
- Create: `crates/engine/src/versions/identity.rs`
- Test: `crates/engine/src/versions/identity_tests.rs`

**Interfaces:**
- Produces:
  - `pub struct Identity { pub sha256: String }` (64 lowercase hex; serde).
  - `pub struct IdentityCache` with `fn new() -> IdentityCache` and `fn identity(&mut self, path: &Path) -> Option<Identity>`: resolves symlinks, keys the cache by the resolved path, size, mtime and inode (`std::os::unix::fs::MetadataExt` on unix; size and mtime elsewhere), hashes only on a miss, streaming in 64 KiB reads, and gives `None` for a missing or unreadable file. `fn hashed(&self) -> u64` counts real hashes (for tests).
- [ ] **Step 1: Write the failing tests:** `same_file_is_hashed_once` (two calls, `hashed() == 1`), `changed_file_is_hashed_again` (rewrite with other bytes and a later mtime), `symlink_is_followed` (a link to a temp file gives that file's hash), `missing_file_is_none`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::identity`. Expected: FAIL.
- [ ] **Step 3: Implement** with `sha2::Sha256`.
- [ ] **Step 4: Run** the same command. Expected: PASS.
- [ ] **Step 5: Commit** `engine: binary identity for tool versions`.

### Task 3: Mac detection over a root

**Zone:** maintainer (`crates/engine`). No CAP.

**Files:**
- Create: `crates/engine/src/versions/mac.rs`
- Test: `crates/engine/src/versions/mac_tests.rs`

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces:
  - `pub struct Roots { pub home: PathBuf, pub npm_prefixes: Vec<PathBuf>, pub brew_prefixes: Vec<PathBuf> }` with `fn for_home(home: &Path) -> Roots` (npm prefixes `<home>/.npm-global`, `/opt/homebrew`, `/usr/local`; brew prefixes `/opt/homebrew`, `/usr/local`). Under a fixture root the daemon passes `for_home(<root>)` with every prefix inside the root (Task 5), so a fixture never reads `/opt/homebrew`.
  - `pub enum Problem { TwoInstalls { other: Method }, CantTell }` (serde `snake_case`).
  - `pub struct Installed { pub tool: Tool, pub version: Option<Version>, pub method: Method, pub path: String, pub identity: Option<Identity>, pub problems: Vec<Problem> }` (serde camelCase).
  - `pub fn detect(roots: &Roots, tool: Tool, cache: &mut IdentityCache) -> Option<Installed>`: reads files only, per spec §7's table:
    - Claude Code: `<home>/.local/bin/claude` → its link target's last part under `.local/share/claude/versions/<v>`; else `<prefix>/lib/node_modules/@anthropic-ai/claude-code/package.json`'s `version`; else a cask folder `<brew>/Caskroom/claude-code*/<v>`. Two found: the first is the one used, `TwoInstalls` names the other.
    - Codex: the `version` in `codex-package.json` under `<home>/.codex/packages/standalone/` (the newest folder holding one; the exact layout is checked against the installer by the builder, spec §25); else npm's `@openai/codex/package.json`; else the `codex` cask.
    - agy: `<home>/.local/bin/agy` found gives `Installed` with `version: None` and `method: Standalone`; Task 5 asks `agy --version`.
    - Notchling: `None` here; the daemon adds its own version (Task 5).
    - `identity` is the launcher's resolved binary (for npm, the package's `bin` entry).
  - `pub fn watch_paths(roots: &Roots) -> Vec<PathBuf>`: the folders to watch (`<home>/.local/bin`, `<home>/.local/share/claude/versions`, `<home>/.codex/packages/standalone`, each npm `lib/node_modules/@anthropic-ai` and `@openai`, each `Caskroom`), only those that exist.
- [ ] **Step 1: Write the failing tests** (temp homes; package files and symlinks made by the test):
  - `native_layout_reads_the_link`, `npm_layout_reads_package_json`, `brew_layout_reads_the_cask_folder`, `codex_standalone_reads_its_package_file`, `two_installs_are_a_problem`, `agy_found_without_version`, `nothing_installed_is_none`
  - `bad_version_in_package_json_is_cant_tell` (`"version": "2.1.3; rm"` gives `version: None` and `Problem::CantTell`)
  - `detect_starts_nothing` (`#[cfg(unix)]`): every binary in the temp layouts is an executable shell script that creates `<tmp>/ran` when started; after `detect` for all four tools, `<tmp>/ran` doesn't exist. (`detect` also takes no `Launcher`, so it can't start anything through Notchling's own path.)
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::mac`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same command. Expected: PASS.
- [ ] **Step 5: Commit** `engine: read the Mac's tool versions from files`.

### Task 4: Worker facts, marks and the mismatch check

**Zone:** maintainer (`crates/engine`). No CAP.

**Files:**
- Create: `crates/engine/src/versions/worker.rs`, `crates/engine/src/versions/mark.rs`, `crates/engine/src/versions/mismatch.rs`
- Test: `crates/engine/src/versions/worker_tests.rs`, `crates/engine/src/versions/mismatch_tests.rs`

**Interfaces:**
- Consumes: Task 1.
- Produces:
  - `pub enum ToolState { NotInstalled, Installed { version: Option<Version>, method: Method, path: String } }` (serde, tag `state`).
  - `pub struct WorkerTools { pub tools: BTreeMap<Tool, ToolState>, pub at_ms: i64 }` (serde camelCase).
  - `pub fn parse_probe(text: &str, now_ms: i64) -> WorkerTools`: reads the keyed lines of the Tool Versions probe (`claude.path=`, `claude.link=`, `claude.version=`, same for `codex`, `agy`, `notchling`); a tool with an empty `path` is `NotInstalled`; a path that fails `machines::shell::is_safe_machine_path` keeps the tool `Installed` with `path: ""` and `method: Unknown`; the version goes through `cli_output`; a key seen twice keeps the first; at most 64 lines read.
  - `pub enum Mark { Same, Behind, Ahead, Held, CantPin, NotInstalled, Unknown }` (serde `snake_case`), `pub fn mark(mac: Option<&Version>, worker: &ToolState, held: bool) -> Mark`: NotInstalled first, then Held, then `can_pin` false gives CantPin, then no version on either side gives Unknown, then the comparison.
  - `pub struct Mismatch { pub tool: Tool, pub machine: String, pub worker: Version, pub mac: Version }` with `fn text(&self) -> String` = `"<machine> runs <Tool title> <worker>, the Mac <mac>"` (exactly spec §17.5's "m1 runs Claude Code 2.1.323, the Mac 2.1.324").
  - `pub fn mismatch(mac: &BTreeMap<Tool, Version>, machine: &str, worker: &WorkerTools) -> Vec<Mismatch>`: one per tool both sides have with different versions; absent tools and unknown versions are left out.
- [ ] **Step 1: Write the failing tests:**
  - `worker_tests.rs`: `probe_lines_make_tools` (a recorded output with native Claude, npm Codex, no agy), `empty_path_is_not_installed`, `hostile_probe_output_keeps_nothing` (a `claude.version=2.1.3;id`, a `codex.path=/tmp/../etc`, 200 lines), `marks_cover_every_case` (one assertion per `Mark`).
  - `mismatch_tests.rs`: `names_both_versions` (asserts the exact text above), `absent_tool_is_never_a_mismatch`, `unknown_version_is_left_out`, `equal_versions_are_no_mismatch`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::worker` and then `cargo test -p notchling-engine --lib versions::mismatch`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both. Expected: PASS.
- [ ] **Step 5: Commit** `engine: worker tool facts, version marks and the mismatch check`.

### Checkpoint 1

- [ ] `cargo test -p notchling-engine --lib versions::`, `cargo clippy -p notchling-engine --all-targets -- -D warnings`, `cargo fmt --all --check`, `PYTHONPATH=tools/guard python3 -m guard lint`. Expected: clean. Push the branch. The engine half is reviewable alone.

### Task 5: The daemon reads the Mac's versions: `versions.get` and the `versions` topic

**Zone:** maintainer (`notchlingd`, `local-api`). `CAP-PROCESS` (`agy --version`).

**Files:**
- Create: `crates/notchlingd/src/versions_ops.rs`, `crates/notchlingd/src/versions_watch.rs`, `crates/engine/src/versions/record.rs`
- Test: `crates/notchlingd/src/versions_ops_tests.rs`, `crates/engine/src/versions/record_tests.rs`
- Modify: `crates/local-api/src/wire.rs` (ops `VERSIONS_GET = "versions.get"`, `VERSIONS_WORKERS = "versions.workers"`; topic `VERSIONS = "versions"`; event `VERSIONS_CHANGED = "versions.changed"`), `crates/notchlingd/src/daemon_wiring.rs` (build `VersionsOps`, `.with_group(..)`, start and close its watcher), `crates/notchlingd/src/lib.rs`, `crates/notchlingd/src/quiet_ops.rs` (`versions.workers` is quiet), `docs/local-api-ops.md`

**Interfaces:**
- Consumes: Tasks 1 to 3; `OpGroup`, `Room::publish`, `platform::watch`, `control::launcher::Launcher` (`FakeLauncher` in tests), `store::doc`.
- Produces:
  - `record.rs`: `pub enum Fact { Seen { tool: Tool, version: Version }, Changed { tool: Tool, machine: String, from: Option<Version>, to: Option<Version>, method: Method, cause: Cause } }`, `pub enum Cause { UserUpdate, SelfUpdate, Rollout, Rollback, Outside }`, `pub trait Recorder: Send + Sync { fn record(&self, fact: Fact); }`, `pub struct MemoryRecorder` (tests). Later phases add variants here, one task at a time. `Fact::audit_words(&self) -> (op: &'static str, result: String)` gives fixed words only (`versions.seen`, `claude 2.1.324`).
  - `VersionsOps`, `fn new(roots: Roots, data_dir: PathBuf, room: Arc<dyn Room>, launcher: Arc<dyn Launcher>, recorder: Arc<dyn Recorder>, clock: Arc<dyn Fn() -> i64 + Send + Sync>, own: Version) -> Arc<VersionsOps>`, `fn rescan(&self)`, `fn start(&self)` (watcher on `watch_paths` plus a 10-minute timer), `fn close(&self)`.
  - `versions.get {tool?}` (`OpSpec::AGENT`) → `{"tools": [{"tool", "installed": {"version", "sha256"} | null, "method", "path", "problems": [..], "running": [], "reference": null, "usable": false, "failed": [], "check": null}]}`. `reference`, `usable`, `failed` and `check` are filled by Phase 2; the shape is final now, so Machines and the app read it once. Notchling's row: `installed.version` is the daemon's own (`env!("CARGO_PKG_VERSION")` through `own`), `method: "release"`, `sha256` of `std::env::current_exe()`.
  - `versions.workers {}` (`OpSpec::APP_QUIET`) → `{"workers": {"<machine>": WorkerTools}}` from `store::doc(data, "versions/workers")` (Machines writes it, Task 7); missing gives `{}`.
  - topic `versions` (agent tier): `versions.changed {tool, installed}` when a rescan finds a different version or identity.
  - agy: `agy --version` through the launcher with a 5-second timeout, only when `detect` found agy without a version, at most once every 10 minutes (clock-based), its output through `cli_output`. Under a fixture root it runs only `<root>/.local/bin/agy` (the rule Codex follows in `crates/AGENTS.md`), else nothing.
  - Recorder facts: a version seen for the first time in this data folder (kept in `versions/seen`, a doc) gives `Seen`; a change gives `Changed { cause: Outside }` (Phase 2 tells user updates apart). The daemon's recorder appends `AuditEntry::now(op, None, None, &result)` lines to the daemon's `AuditLog`.
- [ ] **Step 1: Write the failing tests** (temp home as the root; a recording `Room`; `FakeLauncher`; a fake clock):
  - `get_is_agent_tier` (an agent-tier `Caller` gets the reply), `workers_is_app_tier` (agent tier gets `forbidden`)
  - `get_reads_a_native_layout` (version and a 64-hex `sha256`), `get_has_the_full_shape_with_empty_reference`
  - `rescan_after_a_link_moves_publishes_changed` (move the native link to `versions/2.1.325`, call `rescan`, one `versions.changed`)
  - `unchanged_rescan_publishes_nothing_and_hashes_nothing`
  - `agy_version_asked_at_most_every_ten_minutes` (three rescans within 10 minutes run `agy --version` once; at +10 min twice), `fixture_root_runs_only_its_own_agy`
  - `first_sighting_records_seen_once` (`MemoryRecorder` holds one `Seen` after two rescans)
  - `record_tests.rs`: `audit_words_hold_no_free_text` (every `Fact` variant's words match `^[a-z_.]+$` and `^[a-z0-9_. -]*$`)
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib versions_ops`. Expected: FAIL.
- [ ] **Step 3: Implement.** No lock held across a hash, a file read or the launcher (crates/AGENTS.md, "Locks held across blocking work"): rescan reads into locals, then swaps under the lock.
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd --lib versions_ops`, `cargo test -p notchling-engine --lib versions::record`, `cargo test -p notchling-notchlingd --lib quiet_ops`, `cargo test -p notchling-local-api`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: versions.get and the versions topic from the Mac's files`.

### Task 6: Machines' Tool Versions step (read only)

**Zone:** maintainer (`crates/engine/src/machines`). No CAP (a probe in the existing batched ssh); `WEAK-TEST-EDIT` if an existing test counts the catalog's steps.

**Files:**
- Create: `crates/engine/src/machines/steps_versions.rs`
- Test: `crates/engine/src/machines/tests_versions_probe.rs`
- Modify: `crates/engine/src/machines/step.rs` (`StepId::TOOL_VERSIONS = "tool-versions"`; `Detected.tools: Option<WorkerTools>` and its line in `merged`), `crates/engine/src/machines/catalog.rs` (the step right after Claude Code's login step), `crates/engine/src/machines/mod.rs`

**Interfaces:**
- Consumes: Task 4 (`parse_probe`, `WorkerTools`).
- Produces:
  - `pub fn tool_versions(m: &Machine) -> SetupStep`: needs `REACHABLE`, `Cadence::EveryRefresh`, `Probe::remote(TOOL_VERSIONS_SCRIPT, parse_tool_versions)`, `StepFix::HowTo("Notchling only reads this. Phase 4 adds Match the Mac.")`.
  - `TOOL_VERSIONS_SCRIPT`: spec §7's loop for `claude codex agy notchling` (`command -v`, `readlink`), then `claude --version`, `codex --version` and `notchling --version`, each `2>/dev/null | head -1`. **No `agy --version`** on a worker in this phase: spec §7 runs it only once agy's self-update is off there (Phase 3, behind a verified knob). agy shows its path and method with an unknown version.
  - `parse_tool_versions`: `CheckResult::pass("<n> tools found")` with one `SubRow` per tool (`Claude Code`, `2.1.324 · native installer` or `Not installed`), and `detected.tools = Some(parse_probe(..))`. A worker with none of the tools still passes ("No agent tools found"), since a tool list may be empty (coordinator's call 4).
- [ ] **Step 1: Write the failing tests** in `tests_versions_probe.rs` (the `World` of `tests_engine_world.rs`, its batched probes answered with recorded outputs):
  - `probe_reads_versions_and_methods`, `absent_tools_show_not_installed`
  - `hostile_probe_output_keeps_nothing` (the outputs from Task 4's test, through the step), `masked_output_never_shows_a_vault_value` (a vault value planted in `claude.version`)
  - `tool_versions_has_no_fix_and_is_not_in_set_up_all` (no `StepFix::Run`; `PlanKind::SetUpAll`'s steps don't include it)
  - `script_never_runs_agy_version_or_an_installer` (the script text holds no `agy --version`, `install`, `curl`, `npm`, `brew` or `>`)
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib machines::tests_versions_probe`. Expected: FAIL.
- [ ] **Step 3: Implement.** Then run `cargo test -p notchling-engine --lib machines::`; if an existing test counts the catalog's steps or batch probes, list it for the maintainer in the commit message body (`WEAK-TEST-EDIT`) and update it in a separate commit `machines: tests count the Tool Versions step`.
- [ ] **Step 4: Run** `cargo test -p notchling-engine --lib machines::`. Expected: PASS.
- [ ] **Step 5: Commit** `machines: a read-only Tool Versions step`.

### Task 7: Machines saves worker facts and asks the daemon for the Mac's

**Zone:** `crates/engine/src/machines` is maintainer; `crates/app/src/machines` is auto. No CAP (the socket client is the app's existing `notchling_local_api::Client`; the reviewer checks the gate's reading).

**Files:**
- Create: `crates/engine/src/machines/versions_facts.rs`, `crates/app/src/machines/versions_feed.rs`
- Test: `crates/engine/src/machines/tests_versions_facts.rs`, `crates/app/src/machines/versions_feed_tests.rs`
- Modify: `crates/engine/src/machines/engine.rs` (after a refresh that changed any `detected.tools`, call `versions_facts::save`), `crates/engine/src/machines/mod.rs`, `crates/app/src/machines/data.rs` (`Setup` builds a `MacVersions`: `DaemonVersions` when live, `FixedVersions` for a fixture root and renders), `crates/app/src/machines/mod.rs`

**Interfaces:**
- Consumes: Tasks 4 to 6; `store::update_doc`; `notchling_local_api::Client`, `ClientKind`, `ops::VERSIONS_GET`.
- Produces:
  - `versions_facts::DOC = "versions/workers"`, `pub fn save(data: &Path, machine: &str, tools: &WorkerTools) -> Result<(), String>` (read-modify-write of one map in one `update_doc`; machines no longer in the registry are dropped by `prune(data, names)`).
  - `pub struct MacSide { pub tools: BTreeMap<Tool, MacTool> }`, `pub struct MacTool { pub installed: Option<Version>, pub method: Method, pub reference: Option<Version>, pub usable: bool }` (from the `versions.get` reply).
  - `pub enum MacVersionsError { NotRunning, Bad(String) }`.
  - `pub trait MacVersions: Send + Sync { fn get(&self) -> Result<MacSide, MacVersionsError>; }`; `DaemonVersions { socket: PathBuf }` connects with `Client::connect_with_timeout` (the call timeout, 5 s), calls `versions.get`, and maps any connect or call failure to `NotRunning`; `FixedVersions(Result<MacSide, MacVersionsError>)` for tests and renders.
  - Calls run off the UI thread, the way `actions.rs` does.
- [ ] **Step 1: Write the failing tests:**
  - `tests_versions_facts.rs`: `save_keeps_other_machines`, `prune_drops_removed_machines`, `refresh_saves_facts_once_per_change` (World + a temp data folder: two identical refreshes write once)
  - `versions_feed_tests.rs`: `reply_maps_to_mac_side` (a recorded `versions.get` reply), `no_socket_is_not_running` (a socket path in an empty temp folder), `fixture_setup_uses_fixed_versions` (`Setup::for_run` over `crates/app/fixtures/machines` never builds `DaemonVersions`)
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib machines::tests_versions_facts` and `cargo test -p notchling-app --lib machines::versions_feed`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both. Expected: PASS.
- [ ] **Step 5: Commit** `machines: save worker tool facts and read the Mac's versions from the daemon`.

### Task 8: Machines shows versions against the Mac

**Zone:** auto (`crates/app/src/machines`). No CAP.

**Files:**
- Create: `crates/app/src/machines/versions_model.rs`
- Test: `crates/app/src/machines/versions_model_tests.rs`
- Modify: `crates/app/src/machines/model.rs` (hold the last `Result<MacSide, MacVersionsError>`, refreshed with each Machines refresh), `crates/app/src/machines/cards.rs` (the Tool Versions rows), `crates/app/src/machines/chrome_view.rs` (the banner line), `crates/app/src/machines/renders.rs` (states `machines-versions`, `machines-versions-no-daemon`)

**Interfaces:**
- Consumes: Tasks 4 and 7.
- Produces:
  - `pub struct VersionRow { pub tool: Tool, pub text: String, pub mark: Mark, pub mark_text: &'static str }`; `mark_text`: Same "Same as the Mac", Behind "Behind the Mac", Ahead "Ahead of the Mac", Held "Held", CantPin "Homebrew can't install an exact version", NotInstalled "Not installed", Unknown "Can't tell".
  - `pub fn rows(mac: &Result<MacSide, MacVersionsError>, worker: &WorkerTools, held: &[Tool]) -> Vec<VersionRow>`: with `Err`, every installed tool's mark is `Unknown` and its text is the worker's version alone.
  - `pub fn banner(mac: &Result<MacSide, MacVersionsError>) -> Option<&'static str>`: `NotRunning` gives "Notchling isn't running"; a tool with `reference` and `!usable` gives "The Mac's check is out of date" (from Phase 2; none yet in Phase 1); else `None`.
  - In Phase 1 the Mac side to compare with is `reference` when usable, else `installed`, labelled in the row text as "the Mac has 2.1.324 (not checked yet)". Phase 2 drops the fallback once checks exist (overview, call V2).
  - Part names: `"Tool Versions"`, `"Versions Banner"`.
- [ ] **Step 1: Write the failing tests:** `same_behind_ahead_marks`, `absent_tool_shows_not_installed`, `no_daemon_shows_not_running_and_no_marks`, `brew_shows_cant_pin`, `unchecked_mac_version_is_labelled`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib machines::versions_model`. Expected: FAIL.
- [ ] **Step 3: Implement** the model, then the rows in the card and the banner, with kit tokens.
- [ ] **Step 4: Run** the tests, then `cargo build --release -p notchling-app` and `NOTCHLING_RENDER_FORCE_FRAMES=1 target/release/notchling-machines --render machines-versions --out /tmp/mv.png` and `... --render machines-versions-no-daemon --out /tmp/mv2.png`, and look at both (find the binary with `cargo metadata --format-version 1 --no-deps`).
- [ ] **Step 5: Commit** `machines: tool versions against the Mac on each card`.

### Task 9: Settings > Agents, the Client part (read only)

**Zone:** auto (`crates/app/src/sheets`). No CAP.

**Files:**
- Create: `crates/app/src/sheets/client_model.rs`, `crates/app/src/sheets/client_view.rs`
- Test: `crates/app/src/sheets/client_model_tests.rs`
- Modify: `crates/app/src/sheets/agents.rs` (`Section::Client` first in each tool's sections; `Tool` gains `Codex` and `Antigravity` if decision 0019's branch hasn't added them: then use its names), `crates/app/src/sheets/agents_view.rs`, `crates/app/src/sheets/settings.rs` (fetch `versions.get` and `versions.workers` off the UI thread when the Agents page opens, and follow the `versions` topic while it is open), `crates/app/src/sheets/renders.rs` (`settings-agents-client`), the General page's Notchling row in `crates/app/src/sheets/settings_view.rs`

**Interfaces:**
- Consumes: the `versions.get` and `versions.workers` replies (Task 5), `Mark` (Task 4).
- Produces:
  - `pub struct ClientPart { pub found: String, pub workers: Vec<WorkerLine> }`: `found` like "Found at ~/.local/bin/claude · 2.1.324 · native installer", or "Not found", or "Version unknown" (agy); `WorkerLine { machine, text, state }` like "m1 2.1.324 Same as the Mac", "ub-3 Not installed".
  - `pub fn client_part(tool: Tool, get: &Value, workers: &Value) -> ClientPart`. The home folder is shown as `~`.
  - No Latest, Update, Check or rollout setting yet (Phases 2 and 4). Part name `"Client"`.
- [ ] **Step 1: Write the failing tests:** `found_line_names_path_version_and_method`, `home_is_shown_as_tilde`, `agy_without_version_says_unknown`, `worker_lines_follow_marks`, `no_daemon_says_not_running`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib sheets::client_model`. Expected: FAIL.
- [ ] **Step 3: Implement** the model, then the view inside the Agents page.
- [ ] **Step 4: Run** the tests and render `notchling-app --render settings-agents-client --out /tmp/sac.png` (light and dark with `NOTCHLING_RENDER_APPEARANCE`), and look.
- [ ] **Step 5: Commit** `settings: the Client part shows each tool's version and the workers'`.

### Task 10: The mismatch warning in the cluster's handshake and `delegate --machine`

**Zone:** maintainer (`crates/engine/src/cluster`, `notchlingd`). No CAP. **Runs only after cluster plan B Tasks 3 and 13 are merged**; until then this task is parked in the ledger and Phase 1 ships without it (the check itself shipped in Task 4).

**Files:**
- Modify: `crates/engine/src/cluster/versions.rs` (B's `compat`: the tool-version rule calls `versions::mismatch` and gives `Compat::Warn(text)`, not `Refuse`, for `Purpose::Dispatch`), `crates/notchlingd/src/delegate_cli.rs` (print the warning to stderr before the job's output; the job still goes)
- Test: `crates/engine/src/cluster/versions_warn_tests.rs`, `crates/notchlingd/src/delegate_cli_warn_tests.rs`

**Interfaces:**
- Consumes: `mismatch`, `Mismatch::text` (Task 4); B's `compat`, `Purpose`, `Compat`.
- Produces: `Purpose::Dispatch` on a tool mismatch is `Warn("m1 runs Claude Code 2.1.323, the Mac 2.1.324")` until Phase 6 flips it back to `Refuse` (coordinator's call 10). B's own test `dispatch_refuses_a_tool_mismatch_naming_both` asserts Refuse: changing it is a `WEAK-TEST-EDIT` for the maintainer; the new test `dispatch_warns_on_a_tool_mismatch_until_phase_6` holds the new rule, and B's old test is kept but `#[ignore]`d with the reason in `crates/engine/AGENTS.md` only if the maintainer agrees (overview, conflict C1).
- [ ] **Step 1: Write the failing tests:** `dispatch_warns_on_a_tool_mismatch_until_phase_6` (exact text), `absent_tool_still_refuses` (B's "isn't installed" rule stays a refusal: work that needs a tool can't run without it), `delegate_machine_prints_the_warning_and_runs`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::versions` and `cargo test -p notchling-notchlingd --lib delegate_cli`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both. Expected: PASS.
- [ ] **Step 5: Commit** `cluster: a tool-version mismatch warns until the daemon coordinates rollouts`.

### Task 11: Docs and checks

**Zone:** maintainer (`AGENTS.md` files, `docs/feature-map.md`, `docs/local-api-ops.md` is auto). No CAP.

**Files:**
- Modify: `crates/AGENTS.md` (Architecture in one pass: a **Versions** entry: files only on the Mac, `versions.get` agent tier, Machines asks the daemon, `versions/workers` written by Machines, agy asked at most every 10 minutes, nothing installs yet), `crates/engine/AGENTS.md` (`versions/`: the pattern and why it is strict, identity caching), `crates/notchlingd/AGENTS.md` (`versions_ops.rs`, `versions_watch.rs`), `crates/app/AGENTS.md` (Machines' feed and the Client part), `docs/feature-map.md` (a **Tool versions** feature row: folders, tests, zone), `CHANGELOG.md` (`[Unreleased]` → Added)
- [ ] **Step 1: Run** `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` (on the Mac: CI's clippy never sees GPUI code). Expected: clean.
- [ ] **Step 2: Run** `cargo test --workspace --locked`. Expected: PASS (note known-flaky tests by name if any fail and pass on a rerun).
- [ ] **Step 3: Run** `PYTHONPATH=tools/guard python3 -m guard lint`, `... guard featuremap`, `... guard rules`, `python3 tools/strip_comments.py --check`. Expected: 0 new.
- [ ] **Step 4: Write** the docs above.
- [ ] **Step 5: Commit** `docs: tool versions, phase 1`. Open the pull request; its body lists the `CAP-PROCESS` line (agy), any `WEAK-TEST-EDIT` from Task 6, and that no install runs. Never install the app; the report gives the maintainer `tools/bundle-next.sh` to run.

### Checkpoint 2 (maintainer, on the Mac)

After merge and the maintainer's own install: Settings > Agents shows the Mac's Claude Code and Codex with path, version and method; Machines (with `--live`) shows each worker's tools against the Mac; quitting the daemon makes Machines say "Notchling isn't running". Nothing on a worker changes.
