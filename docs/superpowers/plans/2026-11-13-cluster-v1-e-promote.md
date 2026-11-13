# Cluster v1, plan E: Promote and Fencing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the home is gone (the laptop asleep in a bag, crashed, off the network), the maintainer makes a Standby the home from the phone or the app; the Standby resumes the main sessions from its last set; and a main session can never run tool calls on two machines at once, even when the old home wakes up hours later with turns still running in Claude Code's background host.

**Architecture:** Fencing happens at the tool call. Each node with main sessions writes a fence file (term, boot id, the main sessions, valid until a time on the suspend-aware boot clock); a global `PreToolUse` hook (`notchling hook --fence`) reads only that file and passes, waits or denies, with no socket on the pass path. The home's validity is a lease granted by every other voter (the Standbys), measured from when the home asked, on the home's own boot clock, so sleep counts and clocks need not agree. A Standby promotes safely only after it revoked its grant and waited it out (or the old home confirmed it stopped), so the two can't overlap; "Take Over Anyway" is the maintainer's explicit override when safety can't be shown. The promote is a saga in `engine::cluster::promote`; cut outward steps become inbox items, never reruns. Everything is pure and checked by the simulator before any daemon code.

**Tech Stack:** Rust (engine, notchlingd, local-api, GPUI app), plan B's core and simulator, plan D's sets and restore, Claude Code hooks (`install/hooks.rs`), `agentconsole.v1` (additive), Flutter.

**Spec:** `docs/superpowers/specs/2026-11-13-notchling-cluster-design.md`: "Scope: v1 first" (Failover, Fencing, Notifications rows), "Failover", "Cut turns", review issues 1 (self-fencing doesn't reach the tools), 2 (clocks across sleep), 11 (notifications), invariants I1, I2, I7. Plans B, C (inbox federation) and D merged first. Plan F's decision (F Task 1) must be accepted before this plan ships.

## Global Constraints

- Plan B's constraints hold. Every time in this plan is boot-clock milliseconds (`platform::clock::boot_ms`, counts sleep) unless named wall time.
- Safety before availability for main sessions: a home with an expired lease denies its main sessions' tool calls. Sessions that aren't main are never fenced.
- A fence decision on the pass path opens no socket and starts no process: it reads one file of at most 64 KB.
- The fence file is written atomically (temp file, then rename, no follow), 0600. An agent with a shell can rewrite it: the same accepted risk as every other file (decision 0007).
- Terms are `(n, node id)` ordered by `n` then id; a node adopts any higher term it sees and a home that sees a higher one steps down at once.
- Resumed main sessions are never sent a message. A cut step that may have acted outward is never rerun; the maintainer answers Continue (one reply) or Done.
- Promote and the override are maintainer acts only: app tier or the phone (with `acting` and the bridge on, as `inbox.answer`). An agent never gets them.

## Review Focus

1. The old home wakes after hours with a `claude --bg` turn mid-way: its next tool call is denied before it learns anything. (Task 2 `expired_after_sleep_denies`, Task 7 `hook_denies_on_expired_fence_without_socket`.)
2. A promote that overlaps a still-valid home lease: impossible unless forced. (Task 1 `standby_takes_over_only_after_its_grant_expired`, Task 5 `fence_passes_on_one_node_at_a_time` over every seed.)
3. A cut `git push` in the restored transcript: an item, never a rerun. (Task 3 `cut_git_push_is_outward`, Task 4 `cut_outward_step_raises_item_and_sends_nothing`, Task 5 invariant I7.)
4. Two promotes at once (phone and app, or two Standbys): one home per term, the loser steps down. (Task 5 `two_promotes_one_home_per_term`.)
5. A fresh daemon start on the home: main sessions wait for the first lease, never pass on a stale file from before a reboot. (Task 2 `other_boot_id_never_passes`, Task 6 `start_denies_until_first_lease`.)

---

### Task 1: Leases, the pure part

**Zone:** maintainer. No CAP.

**Files:**
- Create: `crates/engine/src/cluster/lease.rs`
- Test: `crates/engine/src/cluster/lease_tests.rs`
- Modify: `crates/engine/src/cluster/state.rs` (`voters: BTreeSet<NodeId>`; `Command::Ack { node, term }`, `Command::DropVoter { node, forced: bool }`; `Term` is plan B's)

**Interfaces:**
- Produces:
  - consts `LEASE_MS = 15_000`, `RENEW_MS = 3_000`, `SKEW_MS = 2_000`, `BOOT_WAIT_MS = LEASE_MS + SKEW_MS`
  - `pub struct HomeLease { asked: BTreeMap<NodeId, u64>, until: BTreeMap<NodeId, u64> }`: `fn ask(&mut self, voter: &NodeId, now: u64)` (records the send time), `fn granted(&mut self, voter: &NodeId, echo: u64)` (until = echo + `LEASE_MS`; an echo that isn't the last asked time is ignored), `fn valid_until(&self, voters: &BTreeSet<NodeId>, me: &NodeId) -> Option<u64>` (the smallest `until` over every voter but me; `Some(u64::MAX)` when there is no other voter; `None` when one hasn't granted)
  - `pub struct Grantor { booted_at: u64, last_grant: Option<u64>, revoked: bool, highest: Term }`: `fn may_grant(&self, term: &Term, home: &NodeId, i_am_voter: bool, now: u64) -> bool` (term is the highest seen, `home` is its home, not revoked, `now >= booted_at + BOOT_WAIT_MS`), `fn grant(&mut self, now: u64)`, `fn revoke(&mut self)`, `fn safe_after(&self) -> u64` (the time from which this voter's grants have all expired: `last_grant + LEASE_MS + SKEW_MS`, or `booted_at + BOOT_WAIT_MS` with no grant since boot)
  - `pub enum Safety { Safe, WaitUntil(u64), NeedsForce(String) }`; `pub fn takeover_safety(me: &NodeId, state: &ClusterState, grantor: &Grantor, reachable: &BTreeSet<NodeId>, fenced_by_home: bool, now: u64) -> Safety`: not a voter at the current term → `NeedsForce("m1 isn't a Standby at the current term")`; another voter (not the home) unreachable → `NeedsForce("<node> can't be reached")`; `fenced_by_home` (the old home confirmed `fence_now`) or `now >= safe_after()` → `Safe`; else `WaitUntil(safe_after)`
- [ ] **Step 1: Write the failing tests:**
  - `lease_runs_from_the_ask_not_the_answer`, `stale_echo_is_ignored`, `lease_needs_every_other_voter`, `no_other_voter_never_expires`
  - `grantor_waits_after_boot`, `grantor_grants_only_highest_term_home`, `revoked_never_grants`
  - `standby_takes_over_only_after_its_grant_expired`, `fence_now_makes_takeover_safe_at_once`, `non_voter_needs_force`, `unreachable_other_voter_needs_force`
  - `terms_order_by_n_then_node`
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::lease`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it and `cargo test -p notchling-engine --lib cluster::`. Expected: PASS.
- [ ] **Step 5: Commit** `engine: home leases granted by Standbys, on boot clocks`.

### Task 2: The fence file and its decision

**Zone:** maintainer. No CAP (pure; the write is Task 6).

**Files:**
- Create: `crates/engine/src/cluster/fence.rs`
- Test: `crates/engine/src/cluster/fence_tests.rs`

**Interfaces:**
- Produces:
  - `pub struct FenceFile { pub v: u32, pub term: Term, pub node: NodeId, pub boot_id: String, pub valid_until: u64, pub sessions: BTreeSet<String>, pub stopped: Option<StoppedBy> }`, `pub struct StoppedBy { pub home: String, pub term: Term }`; `pub const FENCE_FILE: &str = "cluster/fence.json"`; `pub const MAX_FENCE_BYTES: usize = 65_536`
  - `pub enum Fence { Pass, Wait, Deny(String) }`; `pub fn decide(f: Option<&FenceFile>, session: &str, boot_id: &str, now: u64) -> Fence`:
    - no file → Pass (not in a cluster)
    - session not in `sessions` → Pass
    - `stopped` → Deny `"This session moved to <home>. Continue it there."`
    - `boot_id` differs → Wait (the daemon rewrites after its first lease)
    - `now < valid_until` → Pass
    - else Wait
  - `pub const WAIT_MAX_MS: u64 = 20_000`, `pub const POLL_MS: u64 = 250`; `pub fn after_wait() -> Fence` = Deny `"Notchling can't reach this session's Standby, so it's paused to keep it from running on two machines. Check the cluster in Notchling."`
  - `pub fn parse(bytes: &[u8]) -> Option<FenceFile>` (over the cap, bad JSON or a newer `v` → `None`; a file that exists and doesn't parse is treated as Wait, never Pass: `decide_bytes(Option<&[u8]>, ..)`)
- [ ] **Step 1: Write the failing tests:** `no_file_passes`, `other_session_passes`, `valid_lease_passes`, `expired_after_sleep_denies` (valid_until in the past by hours: Wait, then `after_wait` text), `other_boot_id_never_passes`, `stopped_denies_with_new_home`, `garbage_file_waits_never_passes`, `oversized_file_waits`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::fence`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `engine: the fence file and its decision`.

### Task 3: Outward steps and cut steps

**Zone:** maintainer (`engine/src/cluster/`). No CAP.

**Files:**
- Create: `crates/engine/src/cluster/outward.rs`
- Test: `crates/engine/src/cluster/outward_tests.rs`

**Interfaces:**
- Produces:
  - `pub fn is_outward(tool: &str, input: &Value) -> bool`: one table. Bash commands (each `&&`, `;`, `|` segment): `git push`, `git send-email`, `gh pr create|merge|close|comment`, `gh release`, `gh api` with `-X` other than GET, `npm|pnpm|yarn publish`, `cargo publish`, `docker push`, `kubectl apply|delete|rollout`, `helm install|upgrade`, `terraform apply|destroy`, `fly deploy`, `vercel` with `--prod`, `firebase deploy`, `curl`/`wget` with a method other than GET or with `-d`/`--data`, `ssh`, `scp`, `rsync` to a `host:` path, `aws`/`gcloud`/`az` other than `describe|list|get`; tools: `Artifact` (publish), any `mcp__*` tool whose name holds `send`, `post`, `create`, `update`, `delete`, `publish`, `deploy`, `merge`, `write`, `reply`; everything else not outward
  - `pub struct CutStep { pub tool: String, pub summary: String, pub outward: bool }`; `pub fn cut_steps(conv: &Conversation) -> Vec<CutStep>` (tool calls in the last turn with no result, via `transcript::conversation`)
- [ ] **Step 1: Write the failing tests:** `cut_git_push_is_outward`, `git_status_is_not`, `chained_command_with_push_is_outward`, `curl_get_is_not_curl_post_is`, `mcp_send_is_outward`, `artifact_publish_is_outward`, `read_and_edit_are_not`, `cut_steps_reads_unanswered_calls` (a fixture transcript ending in a `tool_use` with no result).
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::outward`. Expected: FAIL.
- [ ] **Step 3: Implement.** Add the fixture under `crates/engine/tests/fixtures/cluster/cut-push.jsonl` (new file).
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `engine: classify outward steps and find cut ones`.

### Task 4: The promote saga

**Zone:** maintainer. No CAP (pure).

**Files:**
- Create: `crates/engine/src/cluster/promote.rs`
- Test: `crates/engine/src/cluster/promote_tests.rs`

**Interfaces:**
- Consumes: `takeover_safety` (Task 1), `plan_restore` (plan D Task 8), `cut_steps` (Task 3).
- Produces:
  - `pub enum PromoteStep { Check, TryStop, Revoke, Wait, Bump, Restore, Resume, Items, Announce, Done, Failed(String) }`; `pub struct Promote { pub node: NodeId, pub old_home: NodeId, pub forced: bool, pub step: PromoteStep, pub started: u64 }`; `pub fn next(p: &Promote, ev: PromoteEvent, now: u64) -> (Promote, Vec<PromoteAction>)`
  - Check: this node is a Standby with a current set whose term is the current one, `compat(Promote)` with the set's versions Ok; else Failed with the reason
  - TryStop: if the old home is reachable, ask `cluster.fence_now` (it marks its fence stopped, runs `claude stop` on main sessions, answers); 20 s at most
  - Revoke: stop granting; ask every other voter to stop granting the old home (`lease.revoke`)
  - Wait: until `takeover_safety` is Safe; `NeedsForce` fails the promote with that reason unless `forced`
  - Bump: `Term { n: n + 1, node: me }`, home = me, voters = the other voters that acknowledged; journaled before any session starts
  - Restore and Resume: plan D's restore of the current set, then each Claude Code main session `claude --bg --resume <id>` with its flags, idle
  - Items: per resumed session with cut steps, a `paused_step` item (Task 8); with none, nothing
  - Announce: a `failover` notice "`<node>` is the home now" and `cluster.changed`
- [ ] **Step 1: Write the failing tests:** `check_refuses_old_set`, `reachable_home_is_stopped_first`, `waits_out_the_grant`, `needs_force_fails_without_it`, `forced_skips_only_the_wait`, `bump_before_resume`, `cut_outward_step_raises_item_and_sends_nothing`, `no_cut_steps_no_items`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::promote`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `engine: promote a Standby, as a saga`.

### Task 5: Leases and terms in the core; the simulator proves them

**Zone:** maintainer. No CAP.

**Files:**
- Modify: `crates/engine/src/cluster/wire.rs` (`LeaseAsk { term, sent: u64 }`, `LeaseGrant { term, echo: u64 }`, `LeaseRevoke { term }`), `crates/engine/src/cluster/core/` (a new `core/lease.rs`: renew every `RENEW_MS`, `Effect::WriteFence(FenceFile)` on every change of validity, sessions or term; a home that sees a higher term writes `stopped` and emits `Effect::StopMain`), `crates/engine/src/cluster/policy.rs`, `crates/engine/src/cluster/sim/` (scenario ops `Promote { node, forced }`, a fake `claude --bg` per main session on the home that keeps calling tools through the node's fence, including while its node is suspended and right after resume; clock drift on)
- Test: `crates/engine/src/cluster/sim/sim_tests.rs`, `crates/engine/src/cluster/core/lease_tests.rs`

**Interfaces:**
- Produces `Violation::{TwoFencesPass { session, a, b, at }, CutStepRerun { session }}`: I1 (one home per term, terms only grow), I2' (for each main session, the fence passes on at most one node at any instant of simulated real time, except within a window that starts at a forced promote), I7 (a step marked cut is never started again before its item is answered).
- [ ] **Step 1: Write the failing tests:** `fence_passes_on_one_node_at_a_time` (every seed; suspend the home for 1 to 600 s, promote, resume), `two_promotes_one_home_per_term`, `old_home_steps_down_on_contact`, `a_planted_early_takeover_is_caught` (a test flag skips Wait: `TwoFencesPass`), `forced_overlap_is_reported_not_failed`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::sim` and `cluster::core`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** with `NOTCHLING_SIM_SEEDS=5000`. Expected: PASS.
- [ ] **Step 5: Commit** `cluster: leases and terms in the node core, proven by the simulator`.

### Checkpoint 1: the fencing logic

- [ ] `cargo test -p notchling-engine --lib cluster::` with `NOTCHLING_SIM_SEEDS=5000`: PASS. fmt, clippy, guard: clean.
- [ ] Controller (and the maintainer, as a review gate before Task 6): read `lease.rs`, `fence.rs` and the trace of `a_planted_early_takeover_is_caught`.

### Task 6: The fence in the daemon

**Zone:** maintainer. CAP-FILEWRITE, CAP-PROCESS (`claude stop`).

**Files:**
- Create: `crates/notchlingd/src/cluster_fence.rs`
- Test: `crates/notchlingd/src/cluster_fence_tests.rs`
- Modify: `crates/notchlingd/src/cluster_node.rs` (runs `WriteFence`, `StopMain`), `crates/notchlingd/src/codex_ops.rs` and `agy_ops.rs` (a main Codex session's `codex.send` asks `fence::decide` first; an expired fence interrupts a running turn with `codex.stop`), `crates/local-api/src/wire.rs` (node-only `CLUSTER_FENCE_NOW`, `LEASE_*`)

**Interfaces:**
- Produces: `pub fn write_fence(data: &Path, f: &FenceFile) -> Result<(), String>` (temp file in the same folder, 0600, `rename`, a link at the path refused); at start the daemon writes the fence with `valid_until = 0` for its main sessions before anything else, so they wait for the first lease; `cluster.fence_now {term}` from a Standby promoting: writes `stopped`, runs `claude stop` for each main session through `control::session`, answers when done.
- [ ] **Step 1: Write the failing tests:** `start_denies_until_first_lease`, `fence_file_is_atomic_and_0600`, `planted_link_is_refused`, `fence_now_stops_main_sessions`, `codex_send_on_expired_fence_is_refused`, `expired_fence_interrupts_codex_turn`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib cluster_fence`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it and `codex_ops`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: write the fence and stop main sessions on demand`.

### Task 7: The fence hook

**Zone:** maintainer (`notchlingd/src/hook*`, `engine/src/install/`). No new CAP. Live use gated by M8.

**Files:**
- Create: `crates/notchlingd/src/hook_fence.rs`
- Test: `crates/notchlingd/src/hook_fence_tests.rs`, `crates/notchlingd/tests/hook_fence_cli.rs`
- Modify: `crates/notchlingd/src/hook.rs` (`--fence` runs `hook_fence::answer` and nothing else), `crates/engine/src/install/hooks.rs` (a sixth slot `PreToolUse` / `*` with the command `notchling hook --fence` and `timeout: 30`; plans and removal treat it like the others), `crates/engine/src/install/` tests (the slot count)

**Interfaces:**
- Produces: `pub fn answer(stdin: &[u8], data: &Path, boot_id: &str, now: &dyn Fn() -> u64, sleep: &dyn Fn(u64)) -> Option<String>`: reads `session_id` from the event; `None` (print nothing, exit 0) on Pass; on Wait polls every `POLL_MS` up to `WAIT_MAX_MS`; on Deny the `PreToolUse` deny JSON (`hookSpecificOutput.permissionDecision = "deny"` with the reason). Any error reading stdin is a pass; a fence file that exists but can't be read is Wait, then Deny.
- [ ] **Step 1: Write the failing tests:** `pass_prints_nothing`, `deny_prints_pretooluse_deny`, `wait_then_pass_when_lease_arrives`, `wait_then_deny_after_twenty_seconds` (fake clock), `hook_denies_on_expired_fence_without_socket` (CLI e2e: no daemon, a fence file in a temp data root, the built `notchling hook --fence`), `install_plan_adds_the_fence_slot`, `remove_takes_it_out`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib hook_fence`, `cargo test -p notchling-notchlingd --test hook_fence_cli`, `cargo test -p notchling-engine --lib install::`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three and `hook_tests`. Expected: PASS.
- [ ] **Step 5: Commit** `hook: fence main sessions' tool calls`.

### Task 8: Promote ops and inbox items

**Zone:** maintainer. No CAP.

**Files:**
- Create: `crates/notchlingd/src/cluster_promote.rs`
- Test: `crates/notchlingd/src/cluster_promote_tests.rs`
- Modify: `crates/local-api/src/wire.rs` (`CLUSTER_PROMOTE = "cluster.promote"`), `crates/engine/src/inbox/model.rs` (`Kind::HomeUnreachable`, `Failover`, `PausedStep`, `NoHome`; `ActionId::Promote`, `TakeOver`), `crates/notchlingd/src/bridge_backend.rs` (the phone's `inbox.answer` with `promote` or `take_over` runs `cluster.promote`, with the bridge and `acting` on)

**Interfaces:**
- Produces:
  - `cluster.promote {node, force?}` (`OpSpec::APP`; from the phone through the inbox action) → `{step}` at once; progress as `cluster.changed`; an agent never reaches it
  - each Standby raises `home_unreachable` after 60 s without the home ("The laptop hasn't answered for 1 minute") with Promote, and closes it when the home is back; a `NeedsForce` reply turns it into the same item with Take Over Anyway and the reason
  - `paused_step` per cut step: "The laptop dropped while `git push` was running. Check, then Continue." with Continue (sends one reply "Continue" to the session) and Done
  - clients raise `no_home` when neither the home nor any Standby answers for 2 minutes
- [ ] **Step 1: Write the failing tests:** `standby_raises_home_unreachable_after_60s`, `item_closes_when_home_returns`, `promote_from_item_runs_saga`, `needs_force_turns_into_take_over_anyway`, `continue_sends_one_reply`, `agent_cannot_promote`, `phone_promote_needs_acting`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib cluster_promote`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it and `cargo test -p notchling-engine --lib inbox::`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: promote ops and failover items`.

### Task 9: The fence hook on headless nodes

**Zone:** maintainer. No new CAP (`install.apply` exists).

**Files:**
- Modify: `crates/notchlingd/src/install_ops.rs` (`install.apply` forwarded from the home to a headless Standby runs there with that node's own roots), `crates/engine/src/cluster/policy.rs` (`install.plan`, `install.apply` from `Home` origin `App`), `crates/app/src/cluster/view.rs` (a node row's "Install Hook" with the plan shown first)
- Test: `crates/notchlingd/src/install_ops_cluster_tests.rs`
- [ ] **Step 1: Write the failing tests:** `forwarded_apply_installs_on_node_with_install_on`, `install_off_refuses_forwarded_apply`, `only_app_origin_from_home`, `plan_shows_before_apply` (temp roots only; never the real `~/.claude`).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib install_ops_cluster_tests`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: install the hook on a headless Standby from the home`.

### Task 10: Promote from the phone

**Zone:** auto (`apps/flutter/lib/features/inbox/`), new Flutter tests. No CAP.

**Files:**
- Modify: `apps/flutter/lib/features/inbox/` (the `promote` and `take_over` actions open a confirm dialog before answering: "Make m1 the home? Main sessions continue there from 2 minutes ago. If the laptop wakes up, it stops them." / Take Over Anyway adds the reason and "Both may run until the laptop hears of it.")
- Test: `apps/flutter/test/promote_confirm_test.dart`
- [ ] **Step 1: Write the failing tests:** `promote_asks_before_answering`, `cancel_sends_nothing`, `take_over_shows_reason`.
- [ ] **Step 2: Run** `~/flutter/bin/flutter test test/promote_confirm_test.dart` in `apps/flutter`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it and `~/flutter/bin/dart analyze`. Expected: PASS.
- [ ] **Step 5: Commit** `phone: confirm before promoting a Standby`.

### Task 11: Promote in the app

**Zone:** auto. No CAP.

**Files:**
- Modify: `crates/app/src/cluster/model.rs`, `view.rs` (a Standby row: Make Home; Move Home when the home is online (plan D), Promote when it isn't, Take Over Anyway only after a `NeedsForce` answer, each with a confirm), `crates/app/src/cluster/renders.rs`
- Test: `crates/app/src/cluster/promote_tests.rs`
- [ ] **Step 1: Write the failing tests:** `online_home_offers_move_not_promote`, `offline_home_offers_promote`, `take_over_only_after_needs_force`, `paused_step_item_shows_continue_and_done`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib cluster::promote_tests`. Expected: FAIL.
- [ ] **Step 3: Implement**; `ViewSpec` `cluster-promote`.
- [ ] **Step 4: Run**, render (release) and look.
- [ ] **Step 5: Commit** `app: promote a Standby`.

### Task 12: Docs and the ship gate

**Zone:** maintainer. No CAP.

**Files:**
- Create: `docs/decisions/0024-cluster-promote-and-fencing.md` (Proposed: the lease from every other voter on boot clocks; the fence at the tool call; terms with node ids; Take Over Anyway; Codex turns fenced in the daemon only; Codex CLI sessions started outside Notchling are not fenceable and can't be main), `docs/cluster-failover.md` (what to do when the laptop is gone: the steps on the phone, what waits, how to Continue)
- Modify: AGENTS notes, `docs/local-api-ops.md`, `docs/feature-map.md`, `CHANGELOG.md`
- [ ] **Step 1: Write.**
- [ ] **Step 2: Run** the full checks. Expected: clean, PASS.
- [ ] **Step 3: Commit** `docs: promote and fencing`.
- [ ] **Ship gate:** plan F Task 1's decision accepted by the maintainer (spec: push decided before Promote ships).

### Checkpoint 2: the live check (maintainer only)

Gated by M5, M8 (the fence hook installed on the home and m1), M12 (the phone reaches m1). With one main session working on the laptop: close the lid; on the phone, Promote m1 from the item; continue the session on m1; open the lid; the laptop's next tool call in that session is denied and the session stops there.
