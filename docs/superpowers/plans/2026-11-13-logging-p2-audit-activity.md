# Logging, Phase 2: Bus, Audit and Activity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The daemon becomes the only writer of the Audit and of Activity. Every audit call site moves to a registered kind; Audit lines are sealed in monthly files under a key from the Keychain (or a file under a fixture root), with an unsealed mode, checks, a checkpoint and pruning with an anchor. Activity rows live in `daemon/events.db`, and `activity.list` keeps its reply. Old logs are moved and imported once.

**Architecture:** `notchling_events` gains the thread-local scope (actor, `op_id`), ids, the salt file and the pure seal and check. `notchling_engine::logs` holds the Audit's files, the key stores and the checkpoint; `notchling_engine::activity` gains `events.db` and the importer. In `notchlingd`, `LogBus` implements `Emit` (two bounded queues, one writer thread) and owns three sinks: `AuditSink`, `ActivitySink` (with the `log` topic), and nothing else until Phase 4. `local-api`'s `conn.rs` emits the refusals, unknown peers, `op.called`, `op.failed` and app presence; each area emits its own kinds through an injected `Arc<dyn Emit>`.

**Tech Stack:** Rust; `notchling-events` (Phase 1); `hmac` 0.12.1 and `getrandom` 0.3.4 (already in `Cargo.lock`, new direct dependencies of `notchling-events`); `rusqlite` (engine, as `store.rs`); `security-framework` (engine, macOS, as `vault/keychain_mac.rs`).

**Spec:** `docs/superpowers/specs/2026-11-13-logging-design.md`, sections 6.1, 6.3, 7.1, 7.2, 7.4, 8.1, 8.2, 10, 11 (salt), 12, 13, 14, 16 (`log.verify`, `log.chain.start`, `log.audit.delete`, `activity.list`, topic `log`), 17 (Phase 2 and the list of edited tests).

**Branch:** `agent/logging-p2-audit-activity`, from `agent/logging-p1-registry` (or the integration branch that holds Phase 1). Task 2.19 also needs `agent/account-log-fix` (32706ff) in the base; see that task.

**Calls:** C-numbers refer to "Calls these plans make" in `2026-11-13-logging-p1-registry.md`.

## Global Constraints

- Every rule of the Phase 1 plan's Global Constraints.
- `push()` does no I/O and holds no lock across I/O; under 5 µs at p99. `commit()` waits at most 2 s for the writer's acknowledgement that the line is written and flushed (`F_FULLFSYNC` on macOS, `fsync` elsewhere); on timeout or error the op answers `failed` with the message `the audit log can't be written`, and nothing is released.
- Commit only for agent-tier events of `COMMIT_KINDS` that release or act: `vault.secret_used` (given), `vault.secret_requested` (saved), `settings.agent` (set or allowed), agent-tier `session.reply` and `session.control`. Everything else pushes.
- Queues: `Toggle` 16,384 events; `Always` 4,096, then an in-memory overflow up to 8 MB. A full `Toggle` queue drops and counts (`log.dropped`, at most one row a minute).
- The writer takes a batch of up to 250 ms or 500 events; flushes after a batch that holds an `Always` kind and at least once a second; a commit ends the batch at once and commits waiting together share one flush.
- Audit: `<state>/audit/YYYY-MM.jsonl`, 0600 in a 0700 folder, `O_APPEND`, never through a symlink. Line = `{"n":N,` + Phase 1's `write_body` + `,"prev":<seal or null>,"seal":<seal or null>}` (unsealed lines add `,"unsealed":"key_missing"|"key_denied"|"key_waiting"` before `"seal"`). The seal is HMAC-SHA256 under the 32-byte key over the line's bytes up to and not including `,"seal":`, base64 (44 characters).
- Identical refusals within a second fold into one line with `repeat`.
- Keychain items: service `dev.notchling.audit` (key) and `dev.notchling.audit.head` (checkpoint), account = first 16 hex of SHA-256 of the data folder's canonical path. Under `NOTCHLING_DATA_ROOT`, with `--socket`, and on Linux and Windows the key is a file, `<state>/daemon/audit-key` and `<state>/daemon/audit-head`. Unit tests use `MemoryKeyStore`.
- The daemon's start never waits on the Keychain: lines wait in memory at most 2 s, then go out unsealed (`key_waiting`).
- Only a first run (no key item, no checkpoint item, no Audit month, no `audit/legacy/`) makes a key by itself; the run that moves a legacy `audit.jsonl` into `audit/legacy/` counts as that first run (call C15).
- Checkpoint `{n, seal, month, settings_hash, prune_anchor, user_deleted, at}`: after a batch with an `Always` kind (at most every 10 s), every 5 minutes, and at every prune.
- Retention: Audit 1 year by whole months, cap 500 MB; Activity 30 days per kind (`Always` kinds follow the Audit, `account.switched` 90, `usage.daily` 365), cap 200 MB. `log.audit.delete` only for months that ended more than 30 days ago, never the current one.
- `activity.list` keeps `{rows: [{time_ms, source, kind, op, who, result, detail?}], truncated}`, the six `kind` names, `limit` default 200 and max 1,000, and the 900 KB cap; rows gain `event`, `group`, `title`, `outcome`, `refs`, `op_id`, `seq`, `audit_n`, `spooled`; new optional args `groups`, `event_kinds`, `session`, `before_seq`.
- The local API changes additively only. `log.verify` is `AGENT`; `log.chain.start` and `log.audit.delete` are `APP`; topic `log` is app tier.
- Secret names show in Activity and stay in the Audit (the user's decision 1); a secret's value is never written.

## Review Focus

1. The Keychain prompt left open for a minute after an update: the daemon serves at once, lines go out `key_waiting`, and when the user allows, sealing resumes with `audit.checked{resumed, from_n, to_n}` and the next sealed line's `prev` is the last sealed line's seal. (Task 2.8 test `waiting_then_allowed_resumes_and_names_the_stretch`.)
2. A disk that fills while an agent asks for a secret: the commit fails, the agent gets `failed` and no value; the user's own app-tier actions keep working; `log.dropped{sink: audit, reason: write_failed}` is written once writing works again. (Task 2.7 test `commit_fails_on_write_error_and_releases_nothing`; Task 2.16 test `secret_not_released_when_commit_fails`.)
3. A refusal flood (an agent looping on a forbidden op 10,000 times): the Audit gets one line per second per distinct refusal with `repeat`, and no security line is pushed out of the queue. (Task 2.8 test `refusals_fold_into_repeat`.)
4. A daemon killed between writing a line and writing the checkpoint, then started: the next check is `intact`, not `tail_missing`, because the checkpoint lags the file and a file longer than its checkpoint is fine. (Task 2.10 test `file_ahead_of_checkpoint_is_intact`.)
5. An `events.db` damaged or replaced by a symlink: it is set aside as `events.db.damaged` (a symlink is refused, never followed), a new one is made and the audited kinds are rebuilt from the months; `activity.list` never fails the Settings tab. (Task 2.5 test `damaged_db_is_set_aside`; Task 2.6 test `rebuild_from_months_restores_audited_rows`.)

## Order and lanes

```
E: 2.1 scope, ids, salt ──► 2.2 seal and check
G: 2.3 audit files, legacy ─┐
   2.4 key stores ──────────┤
   2.5 events.db ──► 2.6 importer and activity.list over SQL
L: 2.13 local-api conn kinds (after 2.1)
D: 2.7 bus ──► 2.8 audit sink ──► 2.10 checks and ops ──► 2.11 retention ──► 2.12 daemon wiring ──► 2.14 op homes
               2.9 activity sink (after 2.6 and 2.7, beside 2.8)
2.12 also needs 2.13 (`ServerConfig.events`). Areas, after 2.12 and 2.13, in parallel: 2.15 settings and switches | 2.16 vault | 2.17 inbox and phone | 2.18 sessions and install | 2.19 accounts and cost
Then: 2.20 e2e and canary ──► 2.21 app Activity model (auto; can start after 2.6) ──► 2.22 old writers removed, docs
```

- Lanes E, G and L run in parallel (different crates). 2.3, 2.4 and 2.5 run in parallel (different files under `crates/engine/src/logs/` and `activity/`; each adds one line to `crates/engine/src/lib.rs` or `activity/mod.rs`: rebase, don't merge by hand).
- 2.8 and 2.9 run in parallel (`log_audit.rs` vs `log_activity.rs`); both register in `log_bus.rs`'s `Sinks` only through `daemon_wiring.rs` in 2.12.
- The five area tasks touch disjoint files: 2.15 `settings_ops.rs`, `settings_asks.rs`, `switches.rs`, `crates/app/src/switches/restart.rs`; 2.16 `vault_*.rs`; 2.17 `inbox_ops.rs`, `bridge_backend.rs`, `bridge_ops.rs`, `crates/engine/src/bridge/**`; 2.18 `acting_ops.rs`, `control_ops.rs`, `held_ops.rs`, `install_ops.rs`; 2.19 `accounts_*.rs`, `lanes`, `cost_ops.rs`, `crates/engine/src/accounts/**`, `crates/engine/src/cost/**`. Each adds its `with_events` call in `daemon_wiring.rs`: those one-line additions are the only shared file; land them in task order.

## Zones and gates

| Task | Zone | Gates expected | Existing tests edited (why) |
| --- | --- | --- | --- |
| 2.1, 2.2 | Maintainer (`crates/events`, its `Cargo.toml`, `Cargo.lock`) | `CAP-FILEWRITE` (salt file: `OpenOptions`, `create_new`) | none |
| 2.3, 2.4 | Maintainer (engine outside `transcript/`, `search/`) | `CAP-FILEWRITE` (Audit months, legacy move `rename`, `set_permissions`; key file). Keychain use: `security_framework::passwords` doesn't match the `CAP-KEYCHAIN` patterns, so the gate may not fire; the pull request names it as Keychain use anyway | none |
| 2.5, 2.6 | Maintainer | `CAP-FILEWRITE` (`events.db`, set-aside `rename`) | `crates/engine/src/activity/tests.rs` (`WEAK-TEST-EDIT` by rule; in `src/`, so not caught by the path check): it read the three files; it now feeds the importer the same lines and asserts the same rows |
| 2.7 to 2.12, 2.14 | Maintainer (`notchlingd`, `platform`) | `CAP-FILEWRITE` (host id file) | `crates/notchlingd/src/activity_ops_tests.rs` (reads `events.db` rows instead of files); `quiet_ops_tests.rs` deleted, replaced by `op_kinds_tests.rs` (every assertion kept: own-quiet, never-quiet, every op listed); `daemon_tests.rs` if it reads `audit.jsonl` |
| 2.13 | Maintainer (`local-api`) | none new | `server_audit_tests.rs`, `server_audit_reason_tests.rs`, the audit parts of `server_tests.rs` (lines near 106, 279, 306, 534): `ServerConfig.audit` becomes `events`, and they read a `MemorySink` instead of `audit.jsonl`; each keeps its claim (a refusal is recorded, a typed message never is) |
| 2.15 | Maintainer, plus `crates/app/src/switches/restart.rs` (auto) | none | `settings_ops_tests.rs` (asserts `settings.agent` events, and that a commit failure leaves the store unchanged); the inline tests in `switches.rs` (line 299 builds an `AuditLog`) |
| 2.16 | Maintainer | none | `vault_ops_tests.rs` (it asserts `"result":"ok:gh,reg"`; now `vault.secret_used{secrets: [gh, reg], result: given}`), `vault_ops_inbox_tests.rs` only if it reads the audit |
| 2.17 | Maintainer | none | `crates/engine/src/bridge/router_tests.rs` only if `MemoryAudit` changes (it shouldn't: the new sink is another `backend::Audit`) |
| 2.18 | Maintainer | none | none expected |
| 2.19 | Maintainer | none | `crates/engine/src/accounts/claude/tests_log.rs`, `crates/engine/src/cost/account_log_tests.rs`, `crates/notchlingd/src/accounts_ops_log_tests.rs` (from `agent/account-log-fix`: they assert the file grows; now they assert `account.switched` and that the file no longer grows), `crates/engine/src/cost/tracker_tests.rs` (switches from `events.db` first, the file as fallback) |
| 2.20 | Maintainer | `WEAK-TEST-EDIT` fires (path `crates/*/tests/`) | `tests/vault_e2e.rs:123`, `tests/settings_e2e.rs:166`, `tests/install_e2e.rs:137`, `tests/hook_cli.rs:191,253` read `audit.jsonl`; they now read `audit/<month>.jsonl` through a helper and the same facts. New: `tests/log_audit_e2e.rs`, `tests/log_canary_e2e.rs` |
| 2.21 | auto (`crates/app/src/activity/**`) | none | `crates/app/src/activity/model_tests.rs` only gains tests |
| 2.22 | Maintainer (local-api, every `AGENTS.md`, `docs/policies/privacy.md`, `README.md`, `docs/feature-map.md`); `docs/local-api-ops.md` is auto | none | the inline tests of `crates/local-api/src/audit.rs` go with the file (`AuditLog` has no caller left) |

## Maintainer only (Phase 2)

- Approve every test edit in the table, the `Cargo.toml` changes (`hmac`, `getrandom` in `notchling-events`; `notchling-events` in engine, local-api, notchlingd) and the Keychain use.
- On a signed or ad hoc build of this branch, over the real data folder (only the maintainer installs): the first start moves `audit.jsonl` into `audit/legacy/`, makes the key without a prompt and imports old logs; check Settings > Activity shows the old rows. Then update the build: macOS asks once to read the key; answer Allow and check `audit.checked{resumed}`; answer Deny on a second update and check the unsealed banner path (Phase 3 shows it; until then `log.verify` says `unsealed`).
- Measure `commit` p99 with `F_FULLFSYNC` on the Mac (`bench_log`, Phase 5); if over 50 ms, switch to `fsync` and note it in decision 0021.
- Merge `agent/account-log-fix` into the base before Task 2.19 lands.

---

### Task 2.1: Scope, ids and the salt

**Files:**
- Create: `crates/events/src/scope.rs`, `crates/events/src/ids.rs`, `crates/events/src/salt.rs`
- Modify: `crates/events/Cargo.toml` (`getrandom = "0.3"`), `crates/events/src/lib.rs`, `Cargo.lock`
- Test: `crates/events/src/scope_tests.rs`, `crates/events/src/salt_tests.rs`

**Interfaces:**
- Produces:
  - `scope.rs`: `#[derive(Clone, Default)] pub struct Scope { pub op_id: Option<Hex>, pub actor: Option<Actor> }`; `pub fn enter(scope: Scope) -> ScopeGuard` (restores the previous scope on drop); `pub fn current() -> Scope`; `pub fn mark_recorded()`; `pub fn take_recorded() -> bool` (true once after `mark_recorded` in this scope, then reset); `Scope::run<R>(self, f: impl FnOnce() -> R) -> R` (for work moved to another thread: capture `current()` before spawning, `run` inside).
  - `ids.rs`: `pub fn random_bytes<const N: usize>() -> Option<[u8; N]>` (getrandom; `None` on failure), `pub fn new_boot() -> Hex` (8 hex), `pub fn new_op_id() -> Hex` (16 hex), both falling back to a hash of time and pid only when the OS refuses randomness.
  - `salt.rs`: `pub enum SaltChange { Same, Created, Replaced { old_id: Option<Hex> } }`; `pub fn load_or_create(path: &Path) -> std::io::Result<(Salt, SaltChange)>`: reads 32 bytes; missing → creates with `create_new`, 0600, `O_NOFOLLOW` (two processes racing: the loser reads the winner's); unreadable or wrong length → replaces it (write to `log-salt.tmp`, rename) and reports `Replaced` with the old id when the old bytes were 32 long. `pub fn read(path: &Path) -> Option<Salt>`.
- [ ] **Step 1: Write the failing tests.** `scope_tests.rs`: `enter_and_drop_restore_previous`, `mark_recorded_is_taken_once`, `run_carries_scope_to_another_thread` (spawn a thread, `scope.run(|| current().op_id)` equals the outer id). `salt_tests.rs` (in a `TempDir` from `std::env::temp_dir()` plus a random folder): `creates_a_32_byte_0600_salt`, `same_salt_on_second_load`, `short_file_is_replaced_and_reported`, `symlink_in_its_place_is_refused` (unix only, `#[cfg(unix)]`: a symlink at the path gives an error, and the target file is untouched).
- [ ] **Step 2: Run** `cargo test -p notchling-events -- scope_tests salt_tests`; expected FAIL.
- [ ] **Step 3: Implement** the three modules.
- [ ] **Step 4: Run** the same and `cargo clippy -p notchling-events --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/events Cargo.lock` then `git commit -m "events: scope, ids and the salt file"`.

### Task 2.2: The seal and the check

**Files:**
- Create: `crates/events/src/audit_line.rs`, `crates/events/src/audit_check.rs`
- Modify: `crates/events/Cargo.toml` (`hmac = "0.12"`), `crates/events/src/lib.rs`, `Cargo.lock`
- Test: `crates/events/src/audit_line_tests.rs`, `crates/events/src/audit_check_tests.rs`

**Interfaces:**
- Consumes: `write_body`, `Envelope`, `Event`, `Seal`, `RefMode`.
- Produces:
  - `pub enum UnsealedWhy { KeyMissing, KeyDenied, KeyWaiting }` (`key_missing`, `key_denied`, `key_waiting`)
  - `pub fn sealed_line(key: &[u8; 32], n: u64, prev: Option<&Seal>, env: &Envelope, event: &Event, mode: &RefMode) -> (String, Seal)` (no trailing newline)
  - `pub fn unsealed_line(n: u64, why: UnsealedWhy, env: &Envelope, event: &Event, mode: &RefMode) -> String`
  - `pub struct LineHead { pub n: u64, pub kind: String, pub prev: Option<Seal>, pub seal: Option<Seal>, pub unsealed: Option<UnsealedWhy> }`; `pub fn read_head(line: &str) -> Option<LineHead>` (parses with `serde_json`, for the check and the importer)
  - `pub enum CheckResult { Intact, Broken, Gap, TailMissing, FileMissing, Unanchored, Unsealed, Resumed }` with `name()`
  - `pub struct CheckInput<'a> { pub key: Option<&'a [u8; 32]>, pub months: &'a [(u32, Option<&'a str>)], pub anchor: Option<&'a Seal>, pub checkpoint: Option<(u64, &'a Seal)>, pub from_n: Option<u64> }` (`months` in order, `None` text for a month the checkpoint or the file list says should exist but is gone)
  - `pub struct CheckOutcome { pub result: CheckResult, pub from_n: u64, pub to_n: u64, pub first_bad_n: Option<u64>, pub last_sealed: Option<(u64, Seal)>, pub unsealed_stretch: Option<(u64, u64)> }`; `pub fn check(input: &CheckInput) -> CheckOutcome`. Rules: a sealed line whose HMAC doesn't match, or whose `prev` isn't the last sealed line's seal, is `Broken`; an `n` that skips is `Gap`; the last `n` below the checkpoint's `n`, or the checkpoint's seal not found at its `n`, is `TailMissing`; a `None` month is `FileMissing`; the oldest month's first sealed `prev` not equal to `anchor` (when an anchor is given) is `Unanchored`; an `audit.chain_started` line with `prev: null` starts a new chain (lines before it are "sealed with a key that is gone" and not judged); unsealed lines followed by a sealed line give `Resumed` with the stretch; unsealed lines at the end give `Unsealed`; a file longer than the checkpoint is fine.
- [ ] **Step 1: Write the failing tests.** `audit_line_tests.rs`: `line_shape_matches_the_spec` (starts `{"n":7,"v":1,"kind":`, ends `,"prev":"…","seal":"…"}`, the seal is 44 characters), `seal_covers_bytes_up_to_seal_key` (recomputing HMAC over `&line[..line.find(",\"seal\":")]` equals the seal), `unsealed_line_has_null_seal_and_reason`, `read_head_round_trips`. `audit_check_tests.rs`, over a helper that builds a chain of N lines with `MemorySink`-free test events and `[9; 32]` as the key: `intact_chain`, `edited_line_is_broken` (change one character in line 3's `f`: `first_bad_n == 3`), `inserted_line_is_broken`, `deleted_line_is_gap`, `reordered_lines_are_broken`, `cut_before_checkpoint_is_tail_missing`, `cut_after_checkpoint_is_intact`, `month_removed_without_anchor_is_file_missing`, `month_removed_with_anchor_is_intact`, `wrong_anchor_is_unanchored`, `unsealed_tail_is_unsealed`, `unsealed_stretch_then_sealed_is_resumed`, `new_chain_after_chain_started_is_intact`, `wrong_key_is_broken`.
- [ ] **Step 2: Run** `cargo test -p notchling-events -- audit_line_tests audit_check_tests`; expected FAIL.
- [ ] **Step 3: Implement** with `hmac::Hmac<sha2::Sha256>` and `base64::engine::general_purpose::STANDARD`.
- [ ] **Step 4: Run** the same plus clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/events Cargo.lock` then `git commit -m "events: the Audit's seal and its check"`.

### Task 2.3: Audit months and the legacy move

**Files:**
- Create: `crates/engine/src/logs/mod.rs`, `crates/engine/src/logs/audit_files.rs`, `crates/engine/src/logs/legacy.rs`
- Modify: `crates/engine/src/lib.rs` (`pub mod logs;`), `crates/engine/Cargo.toml` (`notchling-events = { path = "../events" }`), `Cargo.lock`
- Test: `crates/engine/src/logs/audit_files_tests.rs`, `crates/engine/src/logs/legacy_tests.rs`

**Interfaces:**
- Produces:
  - `pub fn month_of(ms: i64) -> u32` (UTC, `yyyymm`), `pub fn month_name(m: u32) -> String` (`2026-11`), `pub fn month_end_ms(m: u32) -> i64`
  - `pub struct AuditDir { root: PathBuf }`: `new(root)`, `ensure() -> io::Result<()>` (0700), `months() -> io::Result<Vec<u32>>` (sorted, from `YYYY-MM.jsonl` names only), `append(m: u32, bytes: &[u8]) -> io::Result<()>` (opens `O_APPEND | O_CREAT | O_NOFOLLOW`, 0600, keeps the handle for the current month), `flush(full: bool) -> io::Result<()>` (`fcntl(F_FULLFSYNC)` on macOS when `full`, else `sync_data`), `read(m) -> io::Result<String>`, `remove(m) -> io::Result<()>`, `size() -> u64`, `tail_line() -> io::Result<Option<String>>` (last line of the newest month)
  - `pub struct LegacyFile { pub name: String, pub sha256: Hex, pub lines: u64 }`; `pub fn move_legacy(data_dir: &Path, audit_dir: &AuditDir) -> io::Result<Vec<LegacyFile>>`: moves `<data>/audit.jsonl` and `.1` (when present) into `<audit>/legacy/` with `rename`, sets them 0400, returns their digests; nothing to move gives an empty list; a second call is a no-op. `pub fn has_legacy(audit_dir: &AuditDir) -> bool`.
- [ ] **Step 1: Write the failing tests.** `month_of_and_names` (1794560523117 is `202611`, `2026-11`), `append_creates_0600_in_0700_folder` (unix), `append_refuses_symlink` (unix: a symlink named `2026-11.jsonl` makes `append` fail and leaves the target alone), `months_ignores_other_files`, `tail_line_reads_last_line`; `legacy_moves_both_files_and_counts_lines`, `legacy_move_twice_is_noop`, `legacy_files_are_read_only` (unix: mode 0400).
- [ ] **Step 2: Run** `cargo test -p notchling-engine logs::`; expected FAIL.
- [ ] **Step 3: Implement.** Use `std::os::unix::fs::OpenOptionsExt::custom_flags(libc::O_NOFOLLOW)` under `cfg(unix)` (add `libc` under `[target.'cfg(unix)'.dependencies]` only if the engine doesn't already reach it; it is in `Cargo.lock`).
- [ ] **Step 4: Run** `cargo test -p notchling-engine logs::` and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/engine/src/logs crates/engine/src/lib.rs crates/engine/Cargo.toml Cargo.lock` then `git commit -m "engine: Audit month files and the legacy move"`.

### Task 2.4: Key stores and the checkpoint

**Files:**
- Create: `crates/engine/src/logs/keys.rs`, `crates/engine/src/logs/keys_mac.rs` (`#[cfg(target_os = "macos")]`), `crates/engine/src/logs/checkpoint.rs`
- Test: `crates/engine/src/logs/keys_tests.rs`

**Interfaces:**
- Produces:
  - `pub enum KeyRead { Found([u8; 32]), Missing, Denied }`
  - `pub trait KeyStore: Send + Sync { fn read_key(&self) -> KeyRead; fn create_key(&self) -> Result<[u8; 32], KeyError>; fn read_head(&self) -> Option<Checkpoint>; fn write_head(&self, cp: &Checkpoint) -> Result<(), KeyError>; fn head_exists(&self) -> bool; }`; `pub enum KeyError { Denied, Io(std::io::ErrorKind), NoRandom }`
  - `MemoryKeyStore` (`new()`, `with_key([u8; 32])`, `deny_reads(bool)`, `block_reads(Duration)` to stand in for a prompt), `FileKeyStore::new(dir)` (`audit-key` 32 raw bytes, `audit-head` JSON, both 0600, written without following a symlink), `KeychainKeyStore::for_data_dir(&Path)` (macOS only: service `dev.notchling.audit` and `dev.notchling.audit.head`, account `account_for(data_dir)`; status codes as `vault/keychain_mac.rs`: `-25300` is `Missing`, `-128`, `-25293`, `-25308` are `Denied`)
  - `pub fn account_for(data_dir: &Path) -> String` (first 16 hex of SHA-256 of `canonicalize(data_dir)`, the path as given when it can't be canonicalized)
  - `#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)] pub struct Checkpoint { pub n: u64, pub seal: String, pub month: u32, pub settings_hash: Option<String>, pub prune_anchor: Option<String>, pub user_deleted: UserDeleted, pub at: i64 }`; `pub struct UserDeleted { pub months: Vec<u32>, pub at: Vec<i64>, pub count: u64 }` with `push(month, at)` keeping the newest 16.
- [ ] **Step 1: Write the failing tests.** `memory_store_round_trip`, `memory_store_denies_when_told`, `file_store_round_trip_0600` (unix), `file_store_missing_is_missing`, `file_store_refuses_symlink` (unix), `account_is_16_hex_and_differs_per_folder`, `user_deleted_keeps_newest_16_and_counts_all`. No test calls `KeychainKeyStore`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine logs::keys`; expected FAIL.
- [ ] **Step 3: Implement** with `security_framework::passwords::{get_generic_password, set_generic_password}` in `keys_mac.rs`, as `vault/keychain_mac.rs` does.
- [ ] **Step 4: Run** the same and clippy (on the Mac too, since `keys_mac.rs` only builds there). Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/engine/src/logs` then `git commit -m "engine: the Audit's key stores and checkpoint"`.

### Task 2.5: `daemon/events.db`

**Files:**
- Create: `crates/engine/src/activity/db.rs`, `crates/engine/src/activity/row.rs`
- Modify: `crates/engine/src/activity/mod.rs` (modules)
- Test: `crates/engine/src/activity/db_tests.rs`

**Interfaces:**
- Produces:
  - `pub struct ActivityRow { pub seq: u64, pub ts: i64, pub kind: String, pub kv: u32, pub grp: String, pub outcome: String, pub actor_tier: Option<String>, pub actor_via: Option<String>, pub actor_exe: Option<String>, pub actor_session: Option<String>, pub session: Option<String>, pub folder: Option<String>, pub account: Option<String>, pub device: Option<String>, pub op_id: Option<String>, pub host: String, pub audit_n: Option<u64>, pub spooled: bool, pub legacy: bool, pub f: String, pub expires: i64 }` (`f` is the kind's fields as JSON text; `legacy` is stored in `f` as `"legacy":true` is not allowed, so it is a column: add `legacy INTEGER NOT NULL DEFAULT 0` to the spec's table)
  - `pub struct EventsDb`: `open(path: &Path) -> Result<EventsDb, DbError>` (WAL, `busy_timeout` 2 s, 0600, a symlink in its place refused; a file SQLite can't open as a database, or that fails `PRAGMA quick_check`, is renamed `events.db.damaged` (replacing an older one) and a new one made, reporting `Opened::Rebuilt`); `insert(&mut self, rows: &[ActivityRow]) -> Result<(), DbError>` (one transaction, prepared statement); `list(&self, q: &DbQuery) -> Result<Vec<ActivityRow>, DbError>`; `prune(&mut self, now_ms: i64) -> Result<u64, DbError>` (by `expires`); `size_bytes()`; `trim_to(bytes: u64) -> Result<u64, DbError>` (oldest first); `clear(groups: &[String], include_locked: bool) -> Result<u64, DbError>`; `meta_get(&str) -> Option<String>`, `meta_set(&str, &str)`; `max_seq() -> u64`
  - `pub struct DbQuery { pub since: Option<i64>, pub before_seq: Option<u64>, pub kinds: Vec<String>, pub groups: Vec<String>, pub session: Option<String>, pub limit: usize }`
  - The schema is the spec's 8.2 SQL plus the `legacy` column and an index `activity_seq_desc` is not needed (`seq` is the primary key).
- [ ] **Step 1: Write the failing tests.** `insert_and_list_newest_first`, `list_filters_by_group_kind_session_and_before_seq`, `prune_removes_expired_only`, `trim_to_drops_oldest_first`, `meta_round_trip`, `damaged_db_is_set_aside` (write 100 bytes of junk to `events.db`; `open` gives `Rebuilt`, `events.db.damaged` holds the junk), `symlink_db_is_refused` (unix), `file_is_0600` (unix).
- [ ] **Step 2: Run** `cargo test -p notchling-engine activity::db`; expected FAIL.
- [ ] **Step 3: Implement**, following `store.rs` for opening and `search/fts.rs` for setting a damaged file aside.
- [ ] **Step 4: Run** the same and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/engine/src/activity` then `git commit -m "engine: Activity in daemon/events.db"`.

### Task 2.6: The importer and `activity.list` over SQL

**Files:**
- Create: `crates/engine/src/activity/import.rs`, `crates/engine/src/activity/reply.rs`
- Modify: `crates/engine/src/activity/mod.rs` (`list` reads `EventsDb`), `crates/engine/src/activity/normalize.rs` (becomes the importer's line readers; the rule hiding a secret's name after `ok:` is removed, the user's decision 1)
- Test: `crates/engine/src/activity/import_tests.rs`, `crates/engine/src/activity/reply_tests.rs`; edit `crates/engine/src/activity/tests.rs`
- Fixtures (new files): `crates/engine/tests/fixtures/activity/audit.jsonl`, `remote-log.jsonl`, `account-log.json`, `2026-11.jsonl` (a sealed month made by Task 2.2's helper with key `[9; 32]`)

**Interfaces:**
- Consumes: `EventsDb`, `ActivityRow`, `read_head`, `registry::find`.
- Produces:
  - `pub struct Imported { pub rows: Vec<ActivityRow>, pub skipped: u64 }`
  - `pub fn import_legacy_audit(text: &str, host: &str) -> Imported`: maps `{time_ms, op, tier, peer, result, reason}` to kinds: `result` `forbidden`/`peer_changed`/`needs_approval` → `api.refused`; `connect` with `peer_unknown`/`other_user` → `api.unknown_peer`; `vault.use` → `vault.secret_used` (`ok:A,B` → `result: given`, `secrets: [A, B]`; `denied:…` → `denied`); `settings.set_switch <name>` → `switch.changed`; `settings.set` → `settings.agent` (the `key=value` reason kept only as `key` when it is a known key, else `unknown`; the value never); `file_said_on_ignored` → `switch.changed{cause: file_said_on_ignored}`; anything else → `op.called` or `op.failed` by `result`.
  - `pub fn import_remote_log(text: &str, host: &str) -> Imported`: `pair` → `phone.paired{paired}`, `revoke` → `phone.paired{revoked}`, `message`, `sendNow`, `image`, `answer`, `newSession`, `takeover` → `phone.acted` (`op` `message`, `send_now`, `image`, `answer`, `new_session`, `take_over`; `length` → `chars`), `inbox.answer:<action>` → `phone.answered{result: taken}`, `inbox.answer:refused:<reason>` → `phone.answered{result: <reason>}`. The branch's rule against a phone's session id (a phone's id isn't a session) stays as part of this mapping.
  - `pub fn import_account_log(text: &str, host: &str) -> Imported`: each `{at, id}` (seconds since 2001) → `account.switched{provider: claude, to: id, cause: other}`.
  - `pub fn rebuild_from_months(months: &[(u32, String)], host: &str) -> Imported` (audited kinds back from sealed lines: each line's `f` copied, `audit_n` set).
  - Every imported row has `legacy: true` (except the rebuild), a fresh `seq` assigned by the caller, and `expires` from the kind's `keep_days` (0: the Audit's 365).
  - `reply.rs`: `pub fn reply(rows: &[ActivityRow], q: &Query, names: &dyn Fn(&str, &str) -> Option<String>) -> serde_json::Value` building the branch's reply. The six-kind mapping (C28): outcome `refused` → `refused`; outcome `failed` → `failed`; group `settings` → `setting`; group `inbox` → `inbox`; kind `account.switched` → `account_switch`; else `acted`. `source` is `audit` for rows with `audit_n`, `legacy` for imported rows, else `activity`. `op` is `f.op` when present, else the kind's name. `who` is `app` for `actor_tier = app`, the exe for `agent`, the device's name (from `names("device", id)`, else the id) for `phone`, `daemon` otherwise. `result` is `f.result` or the outcome. `detail` is a short line made from the kind's fields (secret names, a key, a switch and on/off), never more than 200 characters.
  - `Query::from_args` keeps today's args and adds `groups`, `event_kinds`, `session`, `before_seq`.
- [ ] **Step 1: Write the failing tests.** `import_tests.rs`: `legacy_audit_maps_each_shape` (one line per shape in the fixture gives the expected kind and fields), `vault_use_names_are_kept`, `settings_set_value_is_never_imported` (the fixture's line has `reason: "theme=SECRET-MARKER"` and no row's `f` contains `SECRET-MARKER`), `remote_log_maps_each_op`, `account_log_maps_to_switches`, `unreadable_lines_are_counted_and_skipped`, `rebuild_from_months_restores_audited_rows`. `reply_tests.rs`: `reply_shape_is_unchanged` (keys `rows` and `truncated`; each row has `time_ms`, `source`, `kind`, `op`, `who`, `result`), `six_kind_mapping`, `who_resolves_device_names`, `reply_is_capped_at_900k`. In `tests.rs`, change the three file-reading tests to import the same lines into an `EventsDb` in a temp folder and call `list`; keep every asserted row.
- [ ] **Step 2: Run** `cargo test -p notchling-engine activity::`; expected FAIL.
- [ ] **Step 3: Implement.** `list(db: &EventsDb, args: &Value, names: &dyn Fn(&str, &str) -> Option<String>) -> Value` replaces the file-reading `list`.
- [ ] **Step 4: Run** `cargo test -p notchling-engine activity::` and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/engine/src/activity crates/engine/tests/fixtures/activity` then `git commit -m "engine: import old logs into events.db; activity.list over SQL, same reply"`.

### Task 2.7: The bus

**Files:**
- Create: `crates/notchlingd/src/log_bus.rs`, `crates/notchlingd/src/log_queue.rs`
- Modify: `crates/notchlingd/Cargo.toml` (`notchling-events`), `crates/notchlingd/src/lib.rs`, `Cargo.lock`
- Test: `crates/notchlingd/src/log_bus_tests.rs`

**Interfaces:**
- Consumes: `Emit`, `Event`, `Envelope`, `scope::current`, `ids::new_boot`.
- Produces:
  - `pub trait Sink: Send { fn write(&mut self, batch: &[Stamped], audit_n: &[Option<u64>]) -> SinkResult; fn flush(&mut self, full: bool) -> std::io::Result<()>; fn tick(&mut self, now_ms: i64) -> Vec<Event>; }` (`tick` runs once a second on the writer thread and may return events the sink wants written, such as `log.dropped` or `audit.checked`); `pub struct SinkResult { pub audit_n: Vec<Option<u64>>, pub failed: bool }`. Sinks run in order, the Audit sink first; the bus passes the Audit sink's `audit_n` to every later sink's `write` (the Audit sink gets all `None`)
  - `pub struct Stamped { pub env: Envelope, pub event: Event, pub commit: Option<CommitTicket> }`
  - `pub struct BusConfig { pub toggle_cap: usize, pub always_cap: usize, pub overflow_bytes: usize, pub batch_ms: u64, pub batch_max: usize, pub commit_wait: Duration, pub host: Hex, pub boot: Hex, pub pid: u32, pub clock: Arc<dyn Fn() -> i64 + Send + Sync> }` with `BusConfig::daemon(host)` giving 16,384, 4,096, 8 MB, 250 ms, 500, 2 s
  - `pub struct LogBus` implementing `Emit`; `LogBus::start(cfg: BusConfig, sinks: Vec<Box<dyn Sink>>, enabled: Arc<dyn Fn(&KindSpec) -> bool + Send + Sync>) -> Arc<LogBus>`; `LogBus::close(&self, wait: Duration)` (drains, flushes, joins); `LogBus::stats() -> BusStats { pushed, dropped_toggle, dropped_always, commits_failed, overflow_bytes }`
  - `push`: validates `event.json_len() <= MAX_EVENT_BYTES` (debug builds `debug_assert!`, release counts `log.dropped{invalid}`), drops a kind `enabled` says is off, stamps `seq` (an `AtomicU64` started from the sinks' highest known `seq` + 1, passed in `cfg`), `host`, `boot`, `pid`, `proc: Daemon`, the scope's `op_id` and `actor` (when the event carries none), and sends to the `Always` or `Toggle` queue by the kind's lock. A full `Toggle` queue counts; a full `Always` queue goes to the overflow (bytes counted by `json_len`); past 8 MB an `Always` push is dropped and counted (call C31), and the writer writes `log.dropped{sink: audit, reason: queue_full, count}` when it drains.
  - `commit`: the same, plus a ticket; waits `commit_wait` for the writer's answer; `Err(Unwritable { why: Timeout | WriteFailed | Overflow | Closed })`.
  - The writer thread: takes from the `Always` queue first, batches by `batch_ms`/`batch_max`, ends the batch at once on a commit, calls every sink's `write`, flushes (`full` when the batch held a commit) after a batch with an `Always` kind and at least once a second, then answers the tickets (`Ok` only when every sink with that kind's sinks wrote and flushed it); it remembers a failed commit and, once a write works again, pushes `api.refused{why: audit_unwritable, op, exe}` for it and `log.dropped{sink: audit, reason: write_failed, count}`.
- [ ] **Step 1: Write the failing tests** with a `FakeSink` (records batches; can fail writes or stall): `push_stamps_seq_host_boot_and_scope` (inside `scope::enter` with an op id and an agent actor, the stamped envelope has them), `push_never_blocks_when_writer_stalls` (stall the sink, push 20,000 `Toggle` events in under 200 ms; `dropped_toggle == 20,000 - 16,384 - in_flight`), `always_events_overflow_not_drop` (stall, push 5,000 `Always` events: none dropped), `commit_waits_for_flush` (the ticket returns after the fake's `flush(true)`), `commit_fails_on_write_error_and_releases_nothing` (the fake fails: `Err(WriteFailed)`; after it recovers, the sink receives `api.refused{why: audit_unwritable}` and `log.dropped{reason: write_failed}`), `commit_times_out_after_two_seconds` (stalled sink and `commit_wait` 100 ms in the test config: `Err(Timeout)`), `disabled_kind_is_not_written`, `close_drains_everything`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd log_bus`; expected FAIL.
- [ ] **Step 3: Implement** with `std::sync::mpsc::sync_channel` for each queue and a `Mutex<VecDeque>` overflow taken only for `push_back`/`pop_front` (no I/O under it).
- [ ] **Step 4: Run** the same and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/notchlingd/src/log_bus.rs crates/notchlingd/src/log_queue.rs crates/notchlingd/src/log_bus_tests.rs crates/notchlingd/src/lib.rs crates/notchlingd/Cargo.toml Cargo.lock` then `git commit -m "notchlingd: the log bus, push and commit"`.

### Task 2.8: The Audit sink

**Files:**
- Create: `crates/notchlingd/src/log_audit.rs`, `crates/notchlingd/src/log_fold.rs`
- Test: `crates/notchlingd/src/log_audit_tests.rs`

**Interfaces:**
- Consumes: `Sink`, `AuditDir`, `KeyStore`, `KeyRead`, `sealed_line`, `unsealed_line`, `read_head`, `Checkpoint`, `LegacyFile`.
- Produces:
  - `pub struct AuditSink` implementing `Sink`; `AuditSink::new(dir: AuditDir, keys: Arc<dyn KeyStore>, start: StartInfo, legacy: Vec<LegacyFile>, salt: Salt) -> AuditSink`
  - `pub struct StartInfo { pub daemon: Version, pub macos: Version, pub data_root: DataRoot, pub key_wait: Duration }` (`DataRoot` is the `daemon.started` choice)
  - `pub enum Sealing { Starting, Sealed, Unsealed(UnsealedWhy) }`; `AuditSink::sealing(&self) -> Sealing`; `AuditSink::start_new_chain(&mut self, reason) -> Result<u64, KeyError>` (used by `log.chain.start` through the writer, Task 2.10)
  - State machine: at `new`, a thread calls `keys.read_key()` and sends the answer back. While `Starting` (at most `key_wait`, 2 s), lines wait in memory. The first line written in a run is `daemon.started` (made by the sink from `StartInfo`, with `after: clean_stop` when the tail line's kind is `daemon.stopped`, else `unknown`, and `sealing` as settled). `Found` → `Sealed`; `Missing` with no checkpoint, no month and no legacy marker, or with `legacy` non-empty from this start → create the key and write `audit.chain_started{reason: first_run}` (with the legacy files' digests in its fields `legacy_files`, `legacy_lines`, `legacy_sha`, declared in Phase 1); `Missing` otherwise → `Unsealed(KeyMissing)`; `Denied` → `Unsealed(KeyDenied)`; no answer by `key_wait` → `Unsealed(KeyWaiting)`, and a later `Found` → `Sealed`, writing `audit.checked{resumed, from_n, to_n}` first. Entering `Unsealed` writes `audit.checked{unsealed}` once.
  - `n` continues from the tail line's `n`; `prev` from the last sealed line (read back at start with `read_head`). A month change writes to the new file; its first line's `prev` is the previous month's last seal.
  - Checkpoint: after a batch with an `Always` kind (at most every 10 s), every 5 minutes (from `tick`), through `keys.write_head`.
  - `log_fold.rs`: `pub struct Fold`; `Fold::offer(&mut self, ev: &Stamped, now_ms) -> FoldAction { Write, Hold }`; `Fold::due(&mut self, now_ms) -> Vec<Stamped>` (a held refusal's line with `repeat`). Key: kind (`api.refused`, `api.unknown_peer`) plus `op`, `exe`, `why`; window 1 s.
- [ ] **Step 1: Write the failing tests** with `MemoryKeyStore`, a temp `AuditDir` and a fake clock: `first_run_makes_key_and_writes_chain_started`, `legacy_move_counts_as_first_run`, `missing_key_with_history_is_unsealed_missing`, `denied_key_is_unsealed_denied`, `waiting_then_allowed_resumes_and_names_the_stretch` (`block_reads` 3 s with `key_wait` 50 ms: the first lines are `key_waiting`; after the read returns, `audit.checked{resumed}` names their `n` range and the next sealed line's `prev` is the last sealed seal or `null` for a new chain), `daemon_started_is_first_line_with_after` (a tail of `daemon.stopped` gives `clean_stop`), `n_and_prev_continue_across_restart`, `month_change_carries_prev`, `checkpoint_written_after_always_batch_and_every_five_minutes`, `refusals_fold_into_repeat` (1,000 identical refusals in 1 s give 2 lines: the first and one with `repeat: 999`), `start_new_chain_writes_sealed_chain_started_with_previous_head`, `every_written_line_checks_intact` (run Task 2.2's `check` over the files at the end of each test).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd log_audit`; expected FAIL.
- [ ] **Step 3: Implement.** The key read thread never blocks the writer; the sink polls its channel in `write` and `tick`.
- [ ] **Step 4: Run** the same and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/notchlingd/src/log_audit.rs crates/notchlingd/src/log_fold.rs crates/notchlingd/src/log_audit_tests.rs crates/notchlingd/src/lib.rs` then `git commit -m "notchlingd: the Audit sink, sealed months and unsealed mode"`.

### Task 2.9: The Activity sink and the `log` topic

**Files:**
- Create: `crates/notchlingd/src/log_activity.rs`
- Modify: `crates/local-api/src/wire.rs` (`topics::LOG = "log"`, `topics::LOG_EVENT = "log.event"`, `topics::LOG_CHECK = "log.check"`), `crates/notchlingd/src/activity_ops.rs` (reads `EventsDb`), `crates/notchlingd/src/activity_ops_tests.rs`
- Test: `crates/notchlingd/src/log_activity_tests.rs`

**Interfaces:**
- Consumes: `EventsDb`, `ActivityRow`, `Sink`, `Room::publish`.
- Produces:
  - `pub struct ActivitySink` implementing `Sink`; `ActivitySink::new(db: Arc<Mutex<EventsDb>>, salt: Salt, publish: Arc<dyn Fn(&str, &str, Value) + Send + Sync>) -> ActivitySink`. Writes every stamped event whose kind has `ACTIVITY` as one row in one transaction per batch (`RefMode::Plain(salt)`: `Ref`s as ids, `inbox.item` hashed by its `RefKind`), with `audit_n` from the `write` argument, the columns `session`, `folder`, `account`, `device` taken from the fields of those names, `expires = ts + keep_days` (0: 365 days). Publishes each row on topic `log` as `log.event` with the reply row shape of Task 2.6.
  - `tick`: hourly `prune` and `trim_to(200 MB)`, each writing `log.pruned{sink: activity, from, to, entries, reason}` when it removed rows.
  - `ActivityOps::for_daemon(layout)` now opens nothing itself: `ActivityOps::new(db: Arc<Mutex<EventsDb>>, names: Arc<dyn Fn(&str, &str) -> Option<String> + Send + Sync>)`; `activity.list` stays `APP_QUIET` and reads with Task 2.6's `list`. Device names come from the bridge's `devices.json` through a closure the daemon passes in Task 2.12.
- [ ] **Step 1: Write the failing tests.** `log_activity_tests.rs`: `writes_activity_kinds_only` (a `perf.op` and a `daemon.stopped` give no row; `api.refused` gives one), `audit_n_follows_the_audit_sink`, `item_refs_are_hashed`, `publishes_log_event`, `hourly_prune_writes_log_pruned`. In `activity_ops_tests.rs`: replace the file fixtures with rows inserted into an `EventsDb` and keep each assertion about the reply.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd -- log_activity activity_ops`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same plus `cargo test -p notchling-local-api` and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/notchlingd/src/log_activity.rs crates/notchlingd/src/log_activity_tests.rs crates/notchlingd/src/activity_ops.rs crates/notchlingd/src/activity_ops_tests.rs crates/notchlingd/src/lib.rs crates/local-api/src/wire.rs` then `git commit -m "notchlingd: Activity rows in events.db and the log topic"`.

### Task 2.10: Checks, `log.verify`, `log.chain.start` and the Inbox item

**Files:**
- Create: `crates/notchlingd/src/log_checks.rs`, `crates/notchlingd/src/log_ops.rs`
- Modify: `crates/local-api/src/wire.rs` (`ops::LOG_VERIFY = "log.verify"`, `ops::LOG_CHAIN_START = "log.chain.start"`)
- Test: `crates/notchlingd/src/log_checks_tests.rs`, `crates/notchlingd/src/log_ops_tests.rs`

**Interfaces:**
- Consumes: `check`, `AuditDir`, `KeyStore`, `AuditSink::start_new_chain`, `InboxOps::raise`/`resolve`.
- Produces:
  - `pub struct Checker` on the writer's side thread: `run(full: bool) -> CheckOutcome` (from the last checked `n` to the end, or every kept month when `full`), at daemon start and hourly; it pushes `audit.checked` with the result and publishes `log.check` (`{result, sealing, from_n, to_n, first_bad_n, checkpoint_n, chain_started, at}`) when the result or the sealing changes.
  - The Inbox item (C13): on a result other than `intact` and `resumed`, and on `Unsealed`, raise `Kind::Notice` with id `log-warning:<n>` (a fresh `n` per event), title "Audit Log Not Sealed" or "Audit Log Changed Outside Notchling", detail in the spec's 9 wording, action `Open` with `open: {"settings": "logs"}` (Phase 3 opens it); resolve it when the next check is `intact` with sealing on. Writing failures (a sink `failed`) raise "Audit Log Can't Be Written" the same way.
  - `log_ops.rs`: `LogOps` handling `log.verify` (`OpSpec::AGENT`; args `{full?, n?}`; with `n` (app tier only) it answers `{line: {n, sealed, seal_ok, f}}` for that one line, so the Activity tab can check a row against its sealed line; reply as `log.check`'s data; runs on the side thread, so a full check never holds an op thread longer than the check) and `log.chain.start` (`OpSpec::APP`; args `{confirm: true}` else `bad_request`; reply `{chain_started_n}`; sends a command to the writer, which calls `start_new_chain(user)` between batches). An agent-tier `log.chain.start` is refused by the tier check and recorded by `conn.rs` (Task 2.13).
- [ ] **Step 1: Write the failing tests.** `log_checks_tests.rs`: `start_check_is_intact_for_a_clean_chain`, `file_ahead_of_checkpoint_is_intact`, `edited_month_raises_inbox_item_and_publishes_check`, `unsealed_raises_item_once`, `item_resolves_when_intact_again`, `full_check_reads_every_month`. `log_ops_tests.rs`: `verify_reply_shape`, `chain_start_needs_confirm`, `chain_start_writes_chain_started_and_seals_again` (from `Unsealed(KeyMissing)` with `MemoryKeyStore`: afterwards `sealing` is `Sealed` and the line is sealed with `prev: null`).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd -- log_checks log_ops`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/notchlingd/src/log_checks.rs crates/notchlingd/src/log_checks_tests.rs crates/notchlingd/src/log_ops.rs crates/notchlingd/src/log_ops_tests.rs crates/notchlingd/src/lib.rs crates/local-api/src/wire.rs` then `git commit -m "notchlingd: Audit checks, log.verify, log.chain.start, the Inbox warning"`.

### Task 2.11: Retention, the prune anchor and `log.audit.delete`

**Files:**
- Create: `crates/notchlingd/src/log_retention.rs`
- Modify: `crates/notchlingd/src/log_ops.rs` (the op), `crates/local-api/src/wire.rs` (`ops::LOG_AUDIT_DELETE = "log.audit.delete"`)
- Test: `crates/notchlingd/src/log_retention_tests.rs`

**Interfaces:**
- Consumes: `AuditDir`, `Checkpoint`, `UserDeleted`, `month_end_ms`.
- Produces:
  - `pub fn months_to_prune(months: &[u32], sizes: &[u64], now_ms: i64, keep: KeepAudit, cap_bytes: u64) -> Vec<(u32, PruneReason)>` (pure: whole months older than `keep` (`days90`, `year1` default, `years2`), then oldest first past 500 MB; never the current month)
  - `AuditSink::prune(&mut self, month: u32, reason: PruneReason) -> Result<u64, PruneError>`: writes a sealed `audit.pruned{through_month, last_seal, lines, reason, user_deleted_*}` into the current month (the `last_seal` is that month's final seal), flushes, writes the checkpoint with `prune_anchor = last_seal` and the carried `user_deleted`, then deletes the file. With `reason: user`, `user_deleted` gains `(month, now)`; every later `audit.pruned` and checkpoint carries it.
  - `log.audit.delete` (`OpSpec::APP`; args `{through_month: "2026-09"}`): every month up to and including it, each one only if it ended more than 30 days ago and isn't the current month (else `bad_request` with `that month is too recent`); reply `{months_removed, audit_pruned_n}`.
  - Retention runs in the writer's `tick` at start and hourly.
- [ ] **Step 1: Write the failing tests.** `pure_picks_months_older_than_a_year`, `pure_cap_drops_oldest_first`, `pure_never_picks_current_month`, `prune_writes_anchor_then_deletes` (afterwards Task 2.2's `check` with the checkpoint's anchor is `intact`; without it, `unanchored`), `user_delete_is_carried_forward` (two user deletions, then a retention prune: the third `audit.pruned` has `user_deleted_count == 2` and both months), `delete_refuses_recent_month`, `delete_refuses_current_month`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd log_retention`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/notchlingd/src/log_retention.rs crates/notchlingd/src/log_retention_tests.rs crates/notchlingd/src/log_audit.rs crates/notchlingd/src/log_ops.rs crates/notchlingd/src/lib.rs crates/local-api/src/wire.rs` then `git commit -m "notchlingd: Audit retention with the prune anchor; log.audit.delete"`.

### Task 2.12: Daemon wiring, lifecycle and the machine id

**Files:**
- Create: `crates/notchlingd/src/host_id.rs`, `crates/notchlingd/src/log_start.rs`, `crates/notchlingd/src/late_emit.rs`
- Modify: `crates/notchlingd/src/daemon.rs`, `crates/notchlingd/src/daemon_wiring.rs`, `crates/notchlingd/src/layout.rs`, `crates/platform/src/lib.rs` and a new `crates/platform/src/os_version.rs`
- Test: `crates/notchlingd/src/host_id_tests.rs`, `crates/notchlingd/src/log_start_tests.rs`, `crates/platform/src/os_version_tests.rs`

**Interfaces:**
- Consumes: everything from 2.1 to 2.11.
- Produces:
  - `Layout`: `audit_dir() -> PathBuf` (`<state>/audit`), `events_db_path()` (`<state>/daemon/events.db`), `salt_path()` (`<state>/daemon/log-salt`), `host_id_path()` (`<state>/daemon/host-id`), `key_store() -> Arc<dyn KeyStore>` (fixture root or `socket_given` or not macOS: `FileKeyStore::new(<state>/daemon)`; else `KeychainKeyStore::for_data_dir(&data_dir)`), `audit_path()` unchanged (the legacy file).
  - `platform::os_version() -> Option<String>` (macOS: `sysctlbyname("kern.osproductversion")` through `libc`; Linux: `/etc/os-release` `VERSION_ID`; Windows: `None`). No process is started (C23).
  - `host_id.rs`: `pub fn resolve(bridge_dir: &Path, file: &Path) -> io::Result<(Hex, Option<(Hex, Hex)>)>`: the bridge certificate's fingerprint's first 16 hex when `BridgeIdentity::exists`, else the file's id, else a new random 16 hex written 0600; returns `Some((old, new))` when the id changed (the caller pushes `host.identity`).
  - `log_start.rs`: `pub struct LogStart { pub bus: Arc<LogBus>, pub db: Arc<Mutex<EventsDb>>, pub salt: Salt }`; `pub fn start(layout: &Layout, version: &str) -> LogStart`: ensures folders, moves the legacy files (Task 2.3), loads or makes the salt (pushes `log.salt_rotated` on `Replaced`), opens `events.db` (on `Rebuilt` or first open, imports: legacy audit, `remote-log.jsonl`, `account-log.json`, each once by `meta.imported_*`, and rebuilds from months when `Rebuilt`), builds the Audit and Activity sinks and the bus, resolves the host id (pushes `host.identity` on change). It never fails the daemon's start: an error leaves that sink off and is pushed as `log.dropped{reason: write_failed}` once a sink works.
  - `daemon.rs::run`: `log_start::start` right after the lock and before `Server::bind`; `ServerConfig { events: Some(bus.clone()), .. }` (Task 2.13 renames the field); the `file_said_on` line becomes `switch.changed{switch: acting, on: false, cause: file_said_on_ignored}`; on stop, push `daemon.stopped{cause: signal | quit, uptime_ms}` (signal when `signals::stop_requested()`), then `bus.close(2 s)` after the server stops. `Wired::build` keeps its signature (`daemon_wiring_tests.rs:21` calls it): it makes one `Arc<LateEmit>` (`late_emit.rs`, an `Emit` that forwards to a bus set later and counts what it drops before then) and hands it to each area's `with_events` (the area tasks add those lines); `daemon.rs` calls `wired.set_events(bus.clone())` right after `build`, before `Server::bind`, so nothing is lost in a real run.
- [ ] **Step 1: Write the failing tests.** `host_id_tests.rs`: `random_id_made_once_and_kept`, `certificate_id_replaces_random_and_reports_change` (copy the bridge test identity from `crates/engine/src/bridge/net/testdata/` into a temp bridge folder). `late_emit_forwards_after_set_and_counts_before`; `log_start_tests.rs` (temp fixture root): `first_start_moves_legacy_and_imports_once`, `second_start_imports_nothing`, `damaged_db_rebuilds_from_months`, `salt_replaced_pushes_salt_rotated`. `os_version_tests.rs`: `version_is_dotted_digits_when_present`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd -- host_id log_start` and `cargo test -p notchling-platform os_version`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd`, `cargo test -p notchling-platform`, and clippy. Expected: PASS (the e2e tests that read `audit.jsonl` are fixed in Task 2.20; until then run them with `--lib` only).
- [ ] **Step 5: Commit** `git add crates/notchlingd/src crates/platform/src` (by path: the files listed above) then `git commit -m "notchlingd: start the log at daemon start; daemon.started and stopped; host id"`.

### Task 2.13: `local-api` records refusals, unknown peers, calls, failures and the app's presence

**Files:**
- Create: `crates/local-api/src/presence.rs`
- Modify: `crates/local-api/Cargo.toml` (`notchling-events`), `crates/local-api/src/server.rs` (`ServerConfig.audit` becomes `events: Option<Arc<dyn Emit>>`; `Handler` gains two default methods), `crates/local-api/src/conn.rs`
- Test: `crates/local-api/src/presence_tests.rs`, `crates/local-api/src/server_events_tests.rs`; edit `server_audit_tests.rs`, `server_audit_reason_tests.rs`, `server_tests.rs`

**Interfaces:**
- Consumes: `Emit`, `scope`, Phase 1's `ApiRefused`, `ApiUnknownPeer`, `AppAttached`, `AppDetached`, `OpCalled`, `OpFailed`.
- Produces:
  - `Handler::session_of(&self, pid: u32) -> Option<String>` (default `None`), called once at hello for an agent-tier peer and kept on the `Session` (C10).
  - `Handler::home(&self, op: &str) -> OpHome` (default `OpHome::Called`); `pub enum OpHome { Kind, Called, Quiet }`.
  - `conn.rs`: each request runs inside `scope::enter(Scope { op_id: None, actor: Some(Actor { tier, via: <hello's client kind>, exe: Exe::from_path(peer), device: None, session }) })`; after `handler.call` returns: if `scope::take_recorded()`, nothing more; else a refusal (`forbidden`, `peer_changed`, `needs_approval`) → `api.refused{op, exe, tier, why}`; another error → `op.failed{op, code, why: None, duration_ms}`; an app-tier success with `home == Called` → `op.called{op, tier, result: ok, ms}`; `Kind` and `Quiet` successes → nothing (Phase 5 counts quiet ones). Connection refusals (`peer_unknown`, `other_user`) → `api.unknown_peer{why, exe}`; subscribe refusals → `api.refused` with `op: subscribe`. The 200-character message kept by `audit_reason` is gone: no event holds a message.
  - `presence.rs`: `pub struct Presence` (pure, keyed by the app's pid): `on_hello(pid, version, exe, now) -> Option<AppAttached>`, `on_close(pid, now)`, `on_quit(pid)` (Phase 5), `due(now) -> Vec<AppDetached>` (a process with no connection for 10 s → `app.detached{cause: dropped, attached_ms}`). The server's accept loop calls `due` once a second.
- [ ] **Step 1: Write the failing tests.** `presence_tests.rs`: `first_connection_attaches_once`, `many_connections_one_attach`, `close_then_reopen_within_ten_seconds_stays_attached`, `close_for_ten_seconds_detaches_dropped`. `server_events_tests.rs` (a real server in a short `/tmp` folder with a `MemorySink`): `forbidden_op_records_api_refused_with_exe`, `unknown_peer_records_api_unknown_peer`, `app_success_records_op_called_for_called_home`, `quiet_success_records_nothing`, `area_marked_recorded_suppresses_generic_event`, `agent_session_is_resolved_once_at_hello`. Edit the three existing files: build `ServerConfig { events: Some(sink.clone()), .. }` and assert the same facts on `sink.events()`; `server_audit_reason_tests.rs`'s assertion that the typed text appears becomes the assertion that it appears nowhere in any event (`format!("{:?}", events)` doesn't contain the marker), which keeps its privacy claim.
- [ ] **Step 2: Run** `cargo test -p notchling-local-api`; expected FAIL.
- [ ] **Step 3: Implement.** `local-api` must not depend on `notchling-engine`.
- [ ] **Step 4: Run** `cargo test -p notchling-local-api` and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/local-api Cargo.lock` then `git commit -m "local-api: refusals, unknown peers, op.called, op.failed and app presence as events"`.

### Task 2.14: Every op has a home

**Files:**
- Create: `crates/notchlingd/src/op_kinds.rs`, `crates/notchlingd/src/op_kinds_tests.rs`, `crates/notchlingd/src/session_of_tests.rs`
- Delete: `crates/notchlingd/src/quiet_ops_tests.rs` (replaced)
- Modify: `crates/notchlingd/src/daemon_wiring.rs` (the `Handler` impl: `home` from `op_kinds::home`, `session_of` from `ActingOps::session_of`), `crates/notchlingd/src/hook_ops.rs` (`pub(crate) fn session_of(&self, pid: u32) -> Option<String>`: walks the parents as `started_by_session` does, `LINEAGE_DEPTH` 16 over every config folder's `sessions/<p>.json`, and returns the first `sessionId` found; `started_by_session` keeps its behavior and shares the file reader)

**Interfaces:**
- Produces: `pub const HOMES: &[(&str, Home)]` with `pub enum Home { Kind(&'static str), Called, Quiet }`; `pub fn home(op: &str) -> OpHome`; `pub fn known_ops() -> Vec<&'static str>` (for `Op::parse`). Named kinds: `vault.use` → `vault.secret_used`, `vault.request` → `vault.secret_requested`, `vault.save`/`vault.remove` → `vault.entry_changed`, `vault.approval.answer`/`vault.request.cancel` → `inbox.closed` (the answer is also in the use's `answered_by`), `settings.set` → `settings.agent`, `settings.set_switch`/`settings.set_acting`/`bridge.set` → `switch.changed`, `session.reply` → `session.reply`, `session.permission`/`session.answer` → `session.answered`, `session.new`, `session.stop`, `session.remove`, `session.move`, `session.command`, `folder.trust`, `session.interrupt`, `session.take_back` → `session.control`, `inbox.answer` → `inbox.closed`, `install.apply`/`install.remove` → `install.hook`, `accounts.switch`/`accounts.move_all` → `account.switched`, `accounts.add.finish`/`accounts.remove` → `account.login`, `lanes.create`/`lanes.remove`/`lanes.switch` → `lane.changed`, `hello` → `app.attached`, `log.chain.start` → `audit.chain_started`, `log.audit.delete` → `audit.pruned`. Quiet: `QUIET_READS`, `git.*`, `activity.list`, `log.verify`, `log.settings.get` (Phase 3), and agent-tier reads (`sessions.list`, `catalog.get`, `daemon.status`, `settings.list`, `settings.get`, …). Everything else `Called`.
- [ ] **Step 1: Write the failing tests.** In `session_of_tests.rs`: `session_of_finds_the_ancestor_session` and `session_of_none_outside_a_session` (a fake `parent_of` and temp `sessions/` files, as `hook_ops_tests.rs` builds them). Then `op_kinds_tests.rs` with `every_op_name()` copied from `quiet_ops_tests.rs` (it parses `wire.rs` with `include_str!` and adds the limits ops), and: `every_op_has_a_home` (both directions: every name in `wire.rs` is in `HOMES`, every `HOMES` name is in `wire.rs`), `named_kinds_exist` (each `Home::Kind` name is in `notchling_events::registry::find`), `own_quiet_ops_are_quiet` (the old `OWN_QUIET` list), `never_quiet_ops_are_not_quiet` (the old `NEVER_QUIET` and `NEVER_QUIET_LIMITS` lists, kept as they were), `quiet_reads_match_quiet_ops_rs` (`quiet_ops::QUIET_READS` are all `Quiet`).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd -- op_kinds session_of`; expected FAIL.
- [ ] **Step 3: Implement**; delete `quiet_ops_tests.rs` and its `mod` line.
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd` (lib) and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/notchlingd/src/op_kinds.rs crates/notchlingd/src/op_kinds_tests.rs crates/notchlingd/src/session_of_tests.rs crates/notchlingd/src/hook_ops.rs crates/notchlingd/src/daemon_wiring.rs crates/notchlingd/src/lib.rs` and `git rm crates/notchlingd/src/quiet_ops_tests.rs`, then `git commit -m "notchlingd: every op has a home; op_kinds_tests replaces quiet_ops_tests"`.

### Task 2.15: Settings changed by agents, and switches

**Files:**
- Create: `crates/engine/src/settings/log_value.rs` (the key and value mapping, shared with the app's `settings.changed` in Phase 5)
- Modify: `crates/notchlingd/src/settings_ops.rs`, `crates/notchlingd/src/settings_asks.rs`, `crates/notchlingd/src/switches.rs`, `crates/notchlingd/src/daemon_wiring.rs` (one `with_events` line each), `crates/app/src/switches/restart.rs` (sends `"restored": true`)
- Test: edit `crates/notchlingd/src/settings_ops_tests.rs` and the inline tests in `switches.rs`; create `crates/engine/src/settings/keys_log_tests.rs`

**Interfaces:**
- Consumes: `Emit`, `SettingsAgent`, `SwitchChanged`, `SettingKey`, `SettingValue`.
- Produces:
  - `SettingsOps::with_events(Arc<dyn Emit>)` and `SwitchOps::with_events(Arc<dyn Emit>)` replace `with_audit`.
  - `settings.set` from the agent tier: Free class → `commit(settings.agent{result: set})`, then write the store; on `Err`, reply `failed` with `the audit log can't be written` and leave the store as it was. Ask class → `push(settings.agent{result: asked})`, and on the answer: allowed → `commit(..{result: allowed, answered_by, device})` then apply; denied, expired, no_app → push. Never class, unknown key, bad value, full, failed → push with that `result`, outcome `refused` or `failed`. The value is written by `settings::log_value::log_value(key: &str, value: &Value, salt: &Salt) -> LoggedSetting` (`LoggedSetting { key: SettingKey, key_hash: Option<Hex>, value_flag, value_number, value_choice, value_chars, value_hash }`, by the key's type, C7): flag → `value_flag`; number → `value_number`; choice → `value_choice` when it is in the key's allowed list; text → `value_chars` and `value_hash` (SHA-256 with the salt, 16 hex). An unknown key → `key: unknown`, `key_hash`. `answered_by` and `device` come from the Inbox answer (Task 2.17's `answered_by`).
  - Each area that records its own kind calls `scope::mark_recorded()`.
  - `switches.rs`: `switch.changed{switch, on, cause: user | restored_at_launch}` (the second when the app's call has `"restored": true`, an additive optional arg). `daemon_start` and `restore_failed` are reserved: nothing emits them in this plan (C30).
  - `keys_log_tests.rs` (engine): `every_key_is_a_setting_key_choice` (each key in `settings::keys` has a `SettingKey` value of the same name, and the reverse except `unknown` and `other`), `every_allowed_value_is_a_setting_value`.
- [ ] **Step 1: Write the failing tests.** In `settings_ops_tests.rs`, replace the `AuditLog` with a `MemorySink` and assert: `free_set_commits_before_writing`, `commit_failure_leaves_store_unchanged` (`fail_commits(true)`: reply `failed`, the store still holds the old value), `ask_allowed_commits_with_answered_by`, `text_value_is_chars_and_hash` (a marker string never appears in `format!("{:?}", events)`), `unknown_key_is_hashed`. In `switches.rs`'s inline tests: the `AuditLog` becomes a `MemorySink`; assert `switch.changed` with `cause: user`, and `restored_at_launch` when the arg says so. Plus `keys_log_tests.rs`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd -- settings_ops switches` and `cargo test -p notchling-engine keys_log`; expected FAIL.
- [ ] **Step 3: Implement**, and in `crates/app/src/switches/restart.rs` add `"restored": true` to the calls it makes at launch.
- [ ] **Step 4: Run** the same plus `cargo test -p notchling-app switches::` and clippy. Expected: PASS.
- [ ] **Step 5: Commit** the files by path, `git commit -m "settings, switches: settings.agent commits before the write; switch.changed"`.

### Task 2.16: The vault

**Files:**
- Modify: `crates/notchlingd/src/vault_ops.rs`, `vault_asks.rs`, `vault_inbox.rs`, `vault_wait.rs`, `crates/notchlingd/src/inbox_ops.rs` (only the `Responder` trait's new default method), `crates/notchlingd/src/daemon_wiring.rs` (one line)
- Test: edit `crates/notchlingd/src/vault_ops_tests.rs`; create `crates/notchlingd/src/vault_ops_events_tests.rs`

**Interfaces:**
- Consumes: `VaultSecretUsed`, `VaultSecretRequested`, `VaultEntryChanged`, `scope::current().actor`.
- Produces:
  - `Responder::answered_by(&self, item: &Item, action: ActionId, by: &str) -> bool` with a default that calls `answered` (C12); `InboxOps::decide` calls `answered_by`. `ApprovalResponder` and `RequestResponder` override it and hand `(by, device)` to the waiter, `device` from `scope::current().actor.device` (set by the bridge router for a phone, Task 2.17).
  - `Waiters<T>` carries `Answer<T> { value: T, by: Answerer, device: Option<String> }`; `Answerer` is `mac`, `phone` or `none` (Always Allow).
  - `vault.use` (agent tier): after names resolve, approval and value read, and before returning the environment: `commit(vault.secret_used{secrets, session: actor's, folder, command_exe, approval: always_allow | asked, answered_by, device, result: given, waited_ms})`; on `Err` the reply is `failed` with `the audit log can't be written` and the values are dropped. Denied, expired, cancelled, no_app and failed are pushed. Today's line was written for the agent tier only; an app-tier use, if a caller ever makes one, pushes `given` too, since the spec records every use.
  - `vault.request`: `commit(vault.secret_requested{name, session, folder, result: saved, answered_by})` before the reply that says saved; other results push.
  - `vault.save` and `vault.remove` (app tier): push `vault.entry_changed{name, action}` (`saved`, `removed`; `ask_first_on`/`ask_first_off` when a save changes only that flag).
- [ ] **Step 1: Write the failing tests.** In `vault_ops_tests.rs`: the `AuditLog` becomes a `MemorySink`; the assertion `"result":"ok:gh,reg"` becomes `vault.secret_used` with `secrets == [gh, reg]` and `result: given`; the assertion that the value never appears stays (now over the sink's events). New `vault_ops_events_tests.rs`: `secret_not_released_when_commit_fails` (reply `failed`, no `env` key in it), `answered_by_phone_carries_device` (answer through `InboxOps::decide(.., "phone")` inside a scope whose actor has `device: d1`), `always_allow_is_answered_by_none`, `request_saved_commits`, `entry_changed_on_save_and_remove`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd vault_ops`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd -- vault inbox` and clippy. Expected: PASS.
- [ ] **Step 5: Commit** the files by path, `git commit -m "vault: secret uses commit before release and say who allowed them"`.

### Task 2.17: The Inbox and the phone

**Files:**
- Create: `crates/engine/src/bridge/events_audit.rs`
- Modify: `crates/notchlingd/src/inbox_ops.rs`, `crates/notchlingd/src/bridge_ops.rs`, `crates/notchlingd/src/bridge_backend.rs`, `crates/engine/src/bridge/mod.rs`, `crates/engine/src/bridge/net/core.rs` (an audit sink passed in), `crates/engine/src/bridge/net/bridge.rs` (`Bridge::with_audit`), `crates/engine/src/bridge/router_inbox.rs` (scope with the device around `backend.inbox_answer`), `crates/notchlingd/src/daemon_wiring.rs` (one line)
- Test: `crates/engine/src/bridge/events_audit_tests.rs`, `crates/notchlingd/src/inbox_ops_events_tests.rs`

**Interfaces:**
- Produces:
  - `InboxOps::with_events(Arc<dyn Emit>)`: `raise` pushes `inbox.raised{item_kind, item, session}`; a close (answer, expiry, withdrawal, `resolve`) pushes `inbox.closed{item_kind, item, state, action, by, device, waited_ms}` (`by` from `decide`'s `by`, `device` from the scope).
  - `pub struct EventsAudit { emit: Arc<dyn Emit>, now: Clock }` implementing `backend::Audit`: `pair` → `phone.paired{paired, via}`, `revoke` → `phone.paired{revoked}`, `message`, `sendNow`, `image`, `answer`, `newSession`, `takeover` → `phone.acted{device, op, session, chars}`, `inbox.answer:<action>` → `phone.answered{result: taken, action}`, `inbox.answer:refused:<reason>` → `phone.answered{result: <reason>}`; every event's actor is `{tier: phone, via: bridge, device}`; `address` and `stopped_pid` are not written.
  - `Bridge::with_audit(self, Arc<dyn backend::Audit>) -> Bridge`; `Core` keeps `audit: Arc<dyn backend::Audit>` (today an `Arc<AuditLog>`) and uses it for `revoke`; `Core::new` keeps its signature and builds the `LogAudit` as today; a new `Core::with_audit(bridge_dir, parts, audit)` is what `Bridge::start` calls when an audit was given. So `CoreParts` literals in tests don't change, and `remote-log.jsonl` stops growing in the daemon.
  - `bridge_ops.rs`: `bridge.set` pushes `switch.changed{switch: bridge, on, cause}`.
  - `router_inbox.rs`: `scope::enter(Scope { actor: Some(Actor { tier: Phone, via: Some(Via::Bridge), device: Some(Ref::new(device_id)), .. }), .. })` around `backend.inbox_answer`.
- [ ] **Step 1: Write the failing tests.** `events_audit_tests.rs` (with `MemorySink`): `each_bridge_op_maps_to_its_kind` (a table over the eight op shapes), `phone_message_text_never_written` (the entry carries only `length`; assert `chars` and no other value), `revoke_goes_through_the_sink`. `inbox_ops_events_tests.rs`: `raise_and_answer_push_raised_and_closed`, `phone_answer_has_device_from_scope`, `expiry_pushes_closed_expired`, `item_id_is_hashed_in_activity` (through `ActivitySink` with a temp db).
- [ ] **Step 2: Run** `cargo test -p notchling-engine bridge::events_audit` and `cargo test -p notchling-notchlingd inbox_ops`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-engine bridge::`, `cargo test -p notchling-notchlingd -- inbox bridge` and clippy. Expected: PASS, with `router_tests.rs` and `core_tests.rs` unchanged.
- [ ] **Step 5: Commit** the files by path, `git commit -m "inbox, bridge: inbox.raised and closed; phone.* replace remote-log.jsonl"`.

### Task 2.18: Sessions, held messages and the hook install

**Files:**
- Modify: `crates/notchlingd/src/acting_ops.rs`, `crates/notchlingd/src/control_ops.rs`, `crates/notchlingd/src/held_ops.rs`, `crates/notchlingd/src/install_ops.rs`, `crates/notchlingd/src/daemon_wiring.rs` (one line each)
- Test: `crates/notchlingd/src/acting_events_tests.rs`, `crates/notchlingd/src/install_events_tests.rs`

**Interfaces:**
- Produces:
  - `ActingOps::with_events`: `settings.set_acting` → `switch.changed{switch: acting, on, cause}`; `session.reply` → `session.reply{session, via, chars, images, result, why, waited_ms}` (`via`: `chat` from the app unless the args say `notch`, an additive optional `"from": "notch"` the notch may send later; `phone` from the bridge backend's scope; `held` from `held_ops`; `agent` for an agent-tier call, which commits before typing); `session.permission` and `session.answer` → `session.answered{session, prompt, tool, answer, by, device, waited_ms}`; `session.interrupt`, `session.take_back` → `session.control`. `why` maps the reply engine's `Failure` (`reply/exec.rs`) by variant: `Pty` → `pty`, `Timeout` → `timeout`, `Dialog` → `dialog_open`, `QuestionGone` → `question_gone`, `QuestionMismatch` → `question_mismatch`, `NotPermission` → `not_permission`, `PermissionMismatch` → `permission_mismatch`, `NoAlways` → `no_always`, `PermissionStillOpen` → `permission_still_open`; acting off → `acting_off`; others → `other` (C24). The message's text is never read into an event: `chars` is its `chars().count()`.
  - `held_ops.rs`: a held message sent at turn end is `session.reply{via: held}`; `session.reply_held{session, then}` when a message is held, replaced, cancelled or its session is gone.
  - `ControlOps::with_events`: new, stop, remove, move, move all, trust folder, slash command → `session.control{action, command, session, folder, account, result}` (agent-tier calls commit first).
  - `InstallOps::with_events`: `install.apply`/`install.remove` → `install.hook{target, action, result}`.
- [ ] **Step 1: Write the failing tests** with `MemorySink` and the existing fakes (`FakeLauncher`, the scripted pty used by `acting_ops_tests.rs`): `reply_records_chars_not_text`, `reply_failure_maps_why`, `agent_reply_commits_first_and_fails_closed` (a test-only agent-tier caller, `fail_commits(true)`: nothing typed), `permission_answer_records_session_answered`, `held_then_sent_records_reply_held_and_reply_via_held`, `control_new_records_session_control`, `set_acting_records_switch_changed`, `install_apply_records_install_hook`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd -- acting_events install_events`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd` (lib) and clippy. Expected: PASS.
- [ ] **Step 5: Commit** the files by path, `git commit -m "acting, control, held, install: session and install kinds"`.

### Task 2.19: Accounts, lanes and cost

Needs `agent/account-log-fix` (32706ff: `account_log::{FILE_NAME, path, plan_record, record}`, `ClaudeAccounts::note_active`) in the base. If the base doesn't have it, stop after Step 1 and ask the maintainer to merge it first.

**Files:**
- Modify: `crates/engine/src/accounts/claude/mod.rs` (`ClaudeEnv` gains `events: Arc<dyn Emit>`, default `NoEmit` in every constructor helper), `crates/engine/src/cost/tracker.rs`, `crates/engine/src/cost/account_log.rs` (reader kept; `record` no longer called), `crates/notchlingd/src/accounts_ops.rs` (accounts and lanes ops), `crates/notchlingd/src/cost_ops.rs`, `crates/notchlingd/src/daemon_wiring.rs` (one line)
- Test: edit `crates/engine/src/accounts/claude/tests_log.rs`, `crates/engine/src/cost/account_log_tests.rs`, `crates/notchlingd/src/accounts_ops_log_tests.rs`, `crates/engine/src/cost/tracker_tests.rs`

**Interfaces:**
- Produces:
  - `note_active` pushes `account.switched{provider: claude, from, to, cause, window, used_pct, result: ok}` instead of writing `account-log.json`. `cause` comes from the caller: `switch_to` from `accounts.switch` → `user`, from the limits auto-switch → `limit_auto` (with `window` and `used_pct`), from Move All → `move_all`, from a lane → `lane`; a login changed outside Notchling, found by `capture` → `other` (a choice value the registry has through `log_choice!`'s `other`).
  - `accounts.add.finish` and `accounts.remove` push `account.login{provider, account, action, result}`; `lanes.create`, `lanes.remove` and `lanes.switch` push `lane.changed`.
  - `cost::tracker`: switches come from `events.db` (`account.switched` rows, newest first, through an `AccountSwitches` trait the daemon implements on `EventsDb`), merged with the imported rows of the old file; when `events.db` has none, it reads `account-log.json` as today (fallback).
- [ ] **Step 1: Write the failing tests** by editing the three `account-log-fix` tests: where they assert the file gained an entry, assert one `account.switched` with the same account id in a `MemorySink`, and that the file did not change; keep their other assertions (an unreadable log is never written over: now, never written at all). In `tracker_tests.rs`, add `switches_from_events_db_first` and `file_is_the_fallback`, keeping the existing cases.
- [ ] **Step 2: Run** `cargo test -p notchling-engine -- accounts:: cost::` and `cargo test -p notchling-notchlingd accounts_ops`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same and clippy. Expected: PASS.
- [ ] **Step 5: Commit** the files by path, `git commit -m "accounts, cost: account.switched replaces account-log.json's writer"`.

### Task 2.20: End to end, and the canary over Audit and Activity

**Files:**
- Create: `crates/notchlingd/tests/log_audit_e2e.rs`, `crates/notchlingd/tests/log_canary_e2e.rs`, `crates/notchlingd/tests/common/audit.rs` (helper: read every month's lines, run `notchling_events::audit_check::check` with the file key)
- Modify (protected edits): `crates/notchlingd/tests/vault_e2e.rs`, `tests/settings_e2e.rs`, `tests/install_e2e.rs`, `tests/hook_cli.rs`

**Interfaces:**
- Consumes: the built `notchling` binary over a copy of `crates/app/fixtures/notch` with `NOTCHLING_DATA_ROOT` and a fake `HOME`, as the existing e2e tests do; a guard stops every daemon.
- [ ] **Step 1: Write the failing tests.** `log_audit_e2e.rs`: `daemon_run_writes_sealed_month_and_checks_intact` (start, call a refused op as an agent, stop; the month has `daemon.started`, `api.refused`, `daemon.stopped`, all sealed with the file key, `check` is `intact`), `restart_continues_the_chain`, `edited_month_is_reported_by_log_verify` (edit a line between runs; `log.verify` answers `broken`), `missing_key_file_runs_unsealed` (delete `daemon/audit-key` after a run: the next run's lines are `key_missing`, `log.verify` answers `unsealed`), `legacy_audit_is_moved_and_imported` (put a 3-line `audit.jsonl` in the fixture: after start it is in `audit/legacy/`, `activity.list` has 3 `legacy` rows). `log_canary_e2e.rs`: plant `CANARY-7f3a` in a vault command's arguments and a secret's value (`notchling secret`/`vault.use` through the CLI with a fake keychain under the fixture root), a `settings.set` text value and an unknown key's name, a `session.reply` text (acting off: refused), and an error that would quote a path (a `page` op on `/tmp/CANARY-7f3a/missing`); then read every file under the data root (including `events.db` as bytes) and fail if `CANARY-7f3a` appears anywhere except `crash.log`. Edit the four existing tests: their `audit.jsonl` reads go through `common::audit::lines(root)` and assert the same facts in the new shape (a refusal is recorded; the value isn't).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --test log_audit_e2e --test log_canary_e2e`; expected FAIL until the wiring is complete, then PASS.
- [ ] **Step 3: Fix** whatever the e2e tests find in the earlier tasks' code (not in the tests).
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd` (every test target) and `cargo test --workspace --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/notchlingd/tests` then `git commit -m "notchlingd: Audit and Activity end to end; the canary"`.

### Task 2.21: The Activity tab reads the new fields

**Files:**
- Modify: `crates/app/src/activity/model.rs`, `crates/app/src/activity/view.rs`, `crates/app/src/activity/renders.rs`
- Test: `crates/app/src/activity/model_tests.rs` (new tests only)

**Interfaces:**
- Consumes: Task 2.6's reply (unchanged keys plus `event`, `group`, `title`, `outcome`, `refs`, `op_id`, `seq`, `spooled`).
- Produces: `Entry` gains `event: Option<String>`, `title: Option<String>`, `outcome: Option<String>`, `seq: Option<u64>`, `audit_n: Option<u64>`, `spooled: bool`; a row with `audit_n` offers "Check Against the Audit Log" in its details, which calls `log.verify {n}` off the UI thread and says "Matches the sealed entry" or "Doesn't match the sealed entry" (the row's fields compared with the line's `f`); a row shows `title` when present (else today's op label), and "Recorded by the app while the daemon was down" for spooled rows; `Entry::parse` still drops rows whose `kind` is not one of the six, and accepts rows without the new keys (an old daemon).
- [ ] **Step 1: Write the failing tests:** `parses_new_fields_when_present`, `old_reply_still_parses`, `title_wins_over_op_label`, `spooled_row_says_so`, `audit_check_compares_fields` (a canned `log.verify {n}` reply with matching and differing `f`).
- [ ] **Step 2: Run** `cargo test -p notchling-app activity::`; expected FAIL.
- [ ] **Step 3: Implement**; update the `activity` render fixture rows to carry titles.
- [ ] **Step 4: Run** `cargo test -p notchling-app activity::`, then on a Mac `cargo build --release -p notchling-app` and `target/release/notchling-app --render activity --out /tmp/activity.png` and look at it. Expected: PASS, rows show titles.
- [ ] **Step 5: Commit** `git add crates/app/src/activity` then `git commit -m "app: the Activity tab shows each event's title"`.

### Task 2.22: Old writers removed, notes and docs

**Files:**
- Delete: `crates/local-api/src/audit.rs` (with its inline tests), its `mod` and re-exports; `AUDIT_NAME` stays in `lib.rs` for the legacy move.
- Modify: `crates/notchlingd/src/layout.rs` (`audit_path` documented as legacy only, in `crates/notchlingd/AGENTS.md`), `docs/policies/privacy.md` ("Logs on this Mac": `audit/` months, `daemon/events.db`, `daemon/log-salt`, `daemon/host-id`, `daemon/audit-key` under a fixture root, `audit/legacy/`, `remote-log.jsonl` frozen, `account-log.json` frozen; the sentence that no log holds content and that `log_canary_e2e.rs` checks it), `README.md` (`## Logs` table), `crates/AGENTS.md` (Architecture: a "Logs" paragraph; Things that bit before: `preserve_order` and the hand-written line), `crates/notchlingd/AGENTS.md`, `crates/engine/AGENTS.md`, `crates/local-api/AGENTS.md`, `crates/events/AGENTS.md`, `docs/local-api-ops.md` (`log.verify`, `log.chain.start`, `log.audit.delete`, topic `log`, `activity.list`'s new fields), `docs/feature-map.md` (Logs and Activity row: new files and tests)
- [ ] **Step 1: Check** `grep -rn "AuditLog\|AuditEntry::now\|audit_reason" crates/*/src` finds nothing outside the files being deleted; `grep -rn "remote-log" crates/engine/src/bridge/net/core.rs` shows only the fallback path.
- [ ] **Step 2: Delete** `audit.rs` and its uses; run `cargo test --workspace --locked`; expected PASS.
- [ ] **Step 3: Write** the notes and docs above.
- [ ] **Step 4: Run** the full checkpoint below.
- [ ] **Step 5: Commit** by path, `git commit -m "local-api: drop audit.jsonl's writer; logs in privacy, README and notes"`.

## Checkpoint P2 (before the pull request)

- `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets -- -D warnings` (on the Mac too: `keys_mac.rs`), `cargo fmt --all --check`, `cargo deny check`.
- `tools/test-python.sh`; `PYTHONPATH=tools/guard python3 -m guard lint` (0 new: every new file uses no `eprintln!`), `rules`, `claudemd`, `codeowners --check`; `python3 tools/strip_comments.py --check`.
- `cargo test -p notchling-app -- --ignored` render tests unchanged; `crates/app/tests/bench_chat.rs` is not affected (the app doesn't log yet) and needn't run.
- The pull request lists every edited test from the table above with one line each on what it still proves, and names the maintainer-only steps.
