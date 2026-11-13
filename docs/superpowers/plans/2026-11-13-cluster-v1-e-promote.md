# Cluster v1, plan E: Failover, Promote and Fencing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the primary is gone (the laptop m5 asleep in a bag, crashed, off the network), the maintainer's work isn't stuck. With **Automatic Failover** on (Settings, Off by default, 30 minutes or 1 hour) the failover Standby (m1) takes over at the deadline, and the primary has already paused, by itself, the tool calls of the sessions that would move. Off, or sooner, the maintainer moves the primary from the phone or the app (Move to <machine> Now). A session never runs tool calls on two machines at once unless the maintainer confirmed that risk; terms fence every machine; and when the old primary comes back nothing it did is lost: it steps down, offers Take Back, and keeps any work after its last snapshot as a side copy, never deleted or merged.

**Architecture:** Fencing happens at the tool call. Each node with cluster sessions writes a fence file (term, boot id, a gate per session, valid until a time on the suspend-aware boot clock); a global `PreToolUse` hook (`notchling hook --fence`) reads only that file and passes, waits or denies, with no socket on the pass path. A session's gate is `Lease` (it would move: passes until the lease runs out), `Free` (it won't move: always passes), `Moved` (it continues on another machine) or `Waiting` (another machine has newer work in it). With failover on, the primary holds a long lease from one Standby, the failover target: asked and measured on the primary's own boot clock, so sleep counts and clocks need not agree; the primary's `Lease` gates run out `FENCE_LEAD_MS` (2 minutes) before the target may promote. With failover off there is no lease and nothing promotes by itself. A manual promote first checks whether the candidate reaches the primary (directly or through another Can Be Master machine) and relays instead if it does; otherwise it promotes, asking the maintainer to confirm the risk when safety can't be shown. Ownership of each moving session is a **claim** with its own counter, so an old primary that comes back, a Take Back and Continue From This Copy all settle on one owner, and the losing copy is kept. The promote is a saga in `engine::cluster::promote`, the return paths are pure in `engine::cluster::returns`; cut outward steps become inbox items, never reruns. Everything is pure and checked by the simulator before any daemon code.

**Tech Stack:** Rust (engine, notchlingd, local-api, GPUI app), plan B's core and simulator, plan D's sets, restore, handover and held queue, Claude Code hooks (`install/hooks.rs`), settings classes (decision 0020), `agentconsole.v1` (additive), Flutter.

**Spec:** `docs/superpowers/specs/2026-11-13-notchling-cluster-design.md`: "Scope: v1 first" (Failover, Fencing, Notifications rows), "Failover", "Cut turns", review issues 1 (self-fencing doesn't reach the tools), 2 (clocks across sleep), 11 (notifications), invariants I1, I2, I7. The overview's "Decisions 2026-11-14" win where they differ (no short lease; the failover setting; manual promote; return of the old primary; proposals 4 to 6). Plans B, C (inbox federation) and D merged first. Plan F's decision (F Task 1) must be accepted before this plan ships.

## Global Constraints

- Plan B's constraints hold, including its words (Primary, Standby, Can Be Master). Every time in this plan is boot-clock milliseconds (`platform::clock::boot_ms`, counts sleep) unless named wall time.
- **No short lease.** The 15-second lease of the first draft is gone: a primary that can't reach its Standby is never paused unless the maintainer turned Automatic Failover on, and then only shortly before the deadline.
- `clusterFailover`: `off` (default), `30m`, `1h`; class **Ask** (decision 0020: an agent may only ask, the maintainer allows in the inbox); one value for the cluster, journaled by the primary.
- With failover on, the **failover target** is one machine: the first ready Can Be Master member by join order that isn't the primary (m1 in the maintainer's setup). Only it promotes by itself, and only once it has granted the primary a lease at the current term (it is **armed**). The primary asks only the target. Every other Can Be Master machine can be promoted by hand only.
- A fence decision on the pass path opens no socket and starts no process: it reads one file of at most 64 KB. The fence file is written atomically (temp file, then rename, no follow), 0600. An agent with a shell can rewrite it: the same accepted risk as every other file (decision 0007).
- Terms are `(n, node id)` ordered by `n` then id; a node adopts any higher term it sees, Workers and Standbys refuse an older one (plan B), and a primary that sees a higher one steps down at once and stops the sessions it lost.
- Resumed sessions are never sent a message. A cut step that may have acted outward is never rerun; the maintainer answers Continue (one reply) or Done.
- Promote, Take Back, Continue From This Copy and the risk confirm are maintainer acts only: app tier, or the phone with `acting` and the bridge on (as `inbox.answer`). An agent never gets them.
- Side copies: work an old primary did after the set its successor resumed from stays on that machine (its transcript, moved under `<data>/cluster/copies/` only when a restore needs the path; its code in its folder). Notchling never deletes or merges one; Dismiss only hides it from the list.

## Review Focus

1. Failover on, the laptop asleep for an hour mid-turn: its next tool call is denied before it learns anything, and m1 promoted only after the laptop's gates had run out. (Task 3 `expired_after_sleep_waits_then_denies`; Task 1 `target_promotes_only_after_primary_fence`; Task 7 `fence_passes_on_one_node_at_a_time` over every seed.)
2. Failover off, or before the deadline: nothing promotes by itself; a manual promote while the primary is reachable relays instead; one while it isn't needs the risk confirmed. (Task 1 `off_never_promotes`; Task 5 `reachable_primary_relays_instead`, `unsafe_promote_needs_confirm`.)
3. A cut `git push` in the restored transcript: an item, never a rerun. (Task 4 `cut_git_push_is_outward`, Task 5 `cut_outward_step_raises_item_and_sends_nothing`, Task 7 invariant I7.)
4. Two promotes at once, or an old primary coming back: one home per term, the loser steps down, its newer work kept as a side copy. (Task 7 `two_promotes_one_home_per_term`; Task 6 `stepped_down_primary_keeps_side_copy`.)
5. Old primary back while the newer one is off: sessions the newer one never touched continue; touched ones wait unless the maintainer continues from this copy, and the other copy is kept when it returns. (Task 6 `untouched_session_continues`, `touched_session_waits_by_default`, `continue_from_this_copy_keeps_the_other`.)
6. A fresh daemon start on the primary with failover on: moving sessions wait for the first lease, never pass on a stale file from before a reboot. (Task 3 `other_boot_id_never_passes`, Task 8 `start_waits_for_first_lease_with_failover_on`.)

---

### Task 1: Failover timing, the pure part

**Zone:** maintainer. No CAP.

**Files:**
- Create: `crates/engine/src/cluster/failover.rs`
- Test: `crates/engine/src/cluster/failover_tests.rs`
- Modify: `crates/engine/src/cluster/state.rs` (`ClusterState.failover: Failover`, `ClusterState.armed: Option<(NodeId, Term)>`; `Command::SetFailover { failover }`, `Command::Armed { node, term }`; `Term` is plan B's)

**Interfaces:**
- Produces:
  - `pub enum Failover { Off, After30m, After1h }` (serde `off`, `30m`, `1h`), `fn delay_ms(self) -> Option<u64>` (`None`, `1_800_000`, `3_600_000`)
  - consts `FENCE_LEAD_MS = 120_000`, `RENEW_MS = 30_000`, `REACH_MS = 10_000`
  - `pub fn failover_target(state: &ClusterState, ready: &dyn Fn(&NodeId) -> bool) -> Option<NodeId>` (the first member by `joined_ms`, then id, with Can Be Master on, ready, Active, not the home)
  - `pub struct PrimaryLease { asked: Option<(u64, u64)>, until: Option<u64>, delay: Option<u64> }`: `fn ask(&mut self, now: u64) -> u64` (a fresh seq; records the send time), `fn granted(&mut self, seq: u64, reported: Failover)` (until = the seq's send time + the delay the target reports; another seq is ignored; `Off` reported → no deadline), `fn gates_until(&self, armed: bool) -> u64` (`u64::MAX` when failover is off or the target isn't armed; else `until - FENCE_LEAD_MS`, or 0 before the first grant)
  - `pub struct Grantor { booted_at: u64, last_grant: Option<u64>, applied: Failover, revoked: bool }`: `fn may_grant(&self, term: &Term, home: &NodeId, highest: &Term) -> bool` (`term` is the highest seen and `home` its home, not revoked), `fn grant(&mut self, now: u64, applied: Failover)`, `fn revoke(&mut self)`, `fn safe_after(&self) -> Option<u64>` (`None` with `Off` applied or no grant at this term; else the later of `last_grant + delay` and `booted_at + delay`, so a Standby that rebooted waits a whole delay)
  - `pub fn auto_promote_due(me: &NodeId, state: &ClusterState, grantor: &Grantor, reaches_primary: bool, now: u64) -> bool` (failover on, `me` is the target and armed at the current term, `reaches_primary` false, `now >= safe_after`)
  - `pub enum Safety { Safe, Relay, NeedsConfirm(String) }`; `pub fn promote_safety(me: &NodeId, state: &ClusterState, grantor: &Grantor, reaches_primary: bool, now: u64) -> Safety`: `reaches_primary` → `Relay`; `auto_promote_due` → `Safe`; else `NeedsConfirm("<primary> can't be reached. If it's still running, its sessions keep going until it hears of this. Work it does after its last snapshot is kept as a copy on <primary>.")`
  - a setting change takes effect safely in both directions: the target applies a new value when it hears it and reports it in its next grant; the primary's gates follow the delay the target last reported, so turning failover off or shorter never opens a window where the target may promote before the primary's gates run out
- [ ] **Step 1: Write the failing tests:**
  - `failover_values_and_delays`, `target_is_first_ready_can_be_master_by_join_order`, `not_ready_machine_is_never_target`
  - `lease_runs_from_the_ask_not_the_answer`, `stale_echo_is_ignored`, `off_never_expires`, `unarmed_target_never_expires`
  - `off_never_promotes`, `unarmed_target_never_promotes`, `target_promotes_only_after_primary_fence` (for both delays and drift up to 1 ms per 10 s: the target's earliest promote is at least `FENCE_LEAD_MS` minus the drift after the primary's gates run out), `rebooted_grantor_waits_a_full_delay`, `grantor_grants_only_highest_term_home`, `revoked_never_grants`
  - `reachable_primary_means_relay`, `unsafe_promote_needs_confirm`
  - `gates_follow_the_delay_the_target_reported`, `turning_off_stops_the_target_at_once`, `turning_on_arms_only_after_a_grant`
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::failover`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it and `cargo test -p notchling-engine --lib cluster::`. Expected: PASS.
- [ ] **Step 5: Commit** `engine: failover timing, a long lease from one Standby on boot clocks`.

### Task 2: The `clusterFailover` setting

**Zone:** maintainer (`crates/engine/src/settings/keys.rs`, `notchlingd`); auto for the Settings row. No CAP. Needs decision 0020's key classes (`agent/agent-settings-cli`) merged; if it isn't, add the key with today's shape and give it class Ask in the task that lands 0020.

**Files:**
- Modify: `crates/engine/src/settings/keys.rs` (key `clusterFailover`: choices `off` (default), `30m`, `1h`; class Ask, title "Automatic Failover", description "When the primary Mac can't be reached for this long, a Standby takes over its sessions", reason for the class: it changes what runs where), `crates/local-api/src/wire.rs` (`CLUSTER_FAILOVER_SET = "cluster.failover.set"`), `crates/notchlingd/src/cluster_ops.rs` (`cluster.failover.set {value}`, `OpSpec::APP`, forwarded to the primary → `Command::SetFailover`; an allowed agent `settings.set` of the key, which arrives as `settings.changed`, does the same; a joining node takes the cluster's value and writes it to its own key), `crates/app/src/sheets/settings.rs` (Settings > Cluster, only on a Mac in a cluster: "Automatic Failover" Off / After 30 Minutes / After 1 Hour, with "m1 takes over when m5 can't be reached for this long. m5 pauses those sessions 2 minutes before." naming the real machines)
- Test: `crates/engine/src/settings/keys_tests.rs` (new cases), `crates/notchlingd/src/cluster_failover_setting_tests.rs`, `crates/app/src/sheets/settings_cluster_tests.rs`

- [ ] **Step 1: Write the failing tests:** `failover_key_defaults_off_with_three_choices`, `failover_key_is_class_ask`, `agent_set_raises_an_item_and_deny_keeps_off`, `app_change_is_journaled_on_primary`, `client_change_is_forwarded_to_primary`, `joining_node_takes_the_cluster_value`, `settings_row_only_in_a_cluster_and_names_the_target`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib settings::`, `cargo test -p notchling-notchlingd --lib cluster_failover_setting`, `cargo test -p notchling-app --lib sheets::settings_cluster_tests`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three. Expected: PASS.
- [ ] **Step 5: Commit** `settings: Automatic Failover, off by default`.

### Task 3: The fence file and its gates

**Zone:** maintainer. No CAP (pure; the write is Task 8).

**Files:**
- Create: `crates/engine/src/cluster/fence.rs`
- Test: `crates/engine/src/cluster/fence_tests.rs`

**Interfaces:**
- Produces:
  - `pub struct FenceFile { pub v: u32, pub term: Term, pub node: NodeId, pub boot_id: String, pub valid_until: u64, pub sessions: BTreeMap<String, Gate> }`, `pub enum Gate { Lease, Free, Moved { to: String }, Waiting { for_node: String } }`; `pub const FENCE_FILE: &str = "cluster/fence.json"`; `pub const MAX_FENCE_BYTES: usize = 65_536`
  - `pub enum Fence { Pass, Wait, Deny(String) }`; `pub fn decide(f: Option<&FenceFile>, session: &str, boot_id: &str, now: u64) -> Fence`:
    - no file → Pass (not in a cluster); session not listed → Pass (not a moving session)
    - `Free` → Pass
    - `Moved { to }` → Deny `"This session continues on <to>. Open it there."`
    - `Waiting { for_node }` → Deny `"This session waits for <for_node>, which has newer work in it. To go on here, choose Continue From This Copy in Notchling."`
    - `Lease`: `boot_id` differs → Wait (the daemon rewrites after its first lease); `now < valid_until` → Pass; else Wait
  - `pub const WAIT_MAX_MS: u64 = 20_000`, `pub const POLL_MS: u64 = 250`; `pub fn after_wait() -> Fence` = Deny `"Notchling can't reach the Standby that would take over this session, so it's paused to keep it from running on two machines. Check the cluster in Notchling."`
  - `pub fn parse(bytes: &[u8]) -> Option<FenceFile>` (over the cap, bad JSON or a newer `v` → `None`; a file that exists and doesn't parse is treated as Wait, never Pass: `decide_bytes(Option<&[u8]>, ..)`)
- [ ] **Step 1: Write the failing tests:** `no_file_passes`, `unlisted_session_passes`, `free_passes_after_any_sleep`, `valid_lease_passes`, `expired_after_sleep_waits_then_denies` (valid_until in the past by hours: Wait, then `after_wait` text), `other_boot_id_never_passes`, `moved_denies_naming_the_machine`, `waiting_denies_with_the_choice`, `garbage_file_waits_never_passes`, `oversized_file_waits`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::fence`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `engine: the fence file and its per-session gates`.

### Task 4: Outward steps and cut steps

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

### Task 5: The promote saga

**Zone:** maintainer. No CAP (pure).

**Files:**
- Create: `crates/engine/src/cluster/promote.rs`
- Test: `crates/engine/src/cluster/promote_tests.rs`

**Interfaces:**
- Consumes: `promote_safety`, `auto_promote_due` (Task 1), `plan_restore` (plan D Task 8), `HeldQueue` (plan D Task 10), `cut_steps` (Task 4).
- Produces:
  - `pub enum PromoteStep { Check, Reach, Confirm, Bump, Restore, Resume, Held, Items, Announce, Done, Relayed, Failed(String) }`; `pub struct Promote { pub node: NodeId, pub old_home: NodeId, pub auto: bool, pub confirmed: bool, pub step: PromoteStep, pub started: u64 }`; `pub fn next(p: &Promote, ev: PromoteEvent, now: u64) -> (Promote, Vec<PromoteAction>)`
  - Check: this node has Can Be Master on and is ready (plan D Task 12) and `compat(Promote)` against its current set's versions is Ok; else Failed with the reason. A ready machine with no current set may promote: it restores nothing ("controls jobs only": it controls Worker jobs and starts new sessions)
  - Reach: ask the primary directly and through each linked Can Be Master member (one hop), `REACH_MS` at most; any answer → `Relayed` and nothing moves ("`<primary>` is reachable through `<me>`; nothing moved"; the phone's commands keep going through plan D's relay)
  - Confirm: `promote_safety` Safe goes on; NeedsConfirm fails with its text unless `confirmed` (the maintainer saw that text and chose Move Anyway)
  - Bump: `Term { n: n + 1, node: me }`, home = me, journaled before any session starts and pushed to every reachable member (Workers adopt it and refuse the old term from then on); the grantor revokes first
  - Restore and Resume: plan D's restore of the main sessions that may move in the current set, then each Claude Code one `claude --bg --resume <id>` with its flags, idle; Codex on the next message; each gets a claim (Task 6) with `touched: false`
  - Held: plan D's held queue runs here, in order
  - Items: per resumed session with cut steps, a `paused_step` item (Task 10); with none, nothing
  - Announce: a `failover` notice "`<me>` is primary now" (with ", after `<old>` was unreachable for 30 minutes" when `auto`) and `cluster.changed`
- [ ] **Step 1: Write the failing tests:** `check_refuses_a_machine_that_isnt_ready`, `machine_without_a_set_controls_jobs_only`, `reachable_primary_relays_instead`, `reached_through_a_peer_relays_too`, `unsafe_promote_needs_confirm`, `confirmed_promote_goes_on`, `auto_promote_after_deadline_needs_no_confirm`, `bump_before_resume`, `only_movable_sessions_resume`, `held_commands_run_after_restore`, `cut_outward_step_raises_item_and_sends_nothing`, `no_cut_steps_no_items`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::promote`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `engine: promote a Standby, as a saga that relays when it can`.

### Task 6: Claims, side copies and the return paths, the pure part

**Zone:** maintainer. No CAP (pure).

**Files:**
- Create: `crates/engine/src/cluster/returns.rs`, `crates/engine/src/cluster/claims.rs`
- Test: `crates/engine/src/cluster/returns_tests.rs`, `crates/engine/src/cluster/claims_tests.rs`
- Modify: `crates/engine/src/cluster/state.rs` (`ClusterState.claims: BTreeMap<String, Claim>`, `ClusterState.side_copies: Vec<SideCopy>`; commands `Claim`, `Touched { session }`, `SideCopy`, `HideSideCopy`)

**Interfaces:**
- Produces:
  - `pub struct Claim { pub session: String, pub node: NodeId, pub n: u64, pub term: Term, pub base_set: String, pub base_len: u64, pub touched: bool }` (`n` grows by one each time the session changes machines; `base_len` is the transcript length it resumed from); `pub fn winner<'a>(a: &'a Claim, b: &'a Claim) -> &'a Claim` (higher `n`, then `term`, then `node`). Claims travel with plan B's `Members` and between Can Be Master machines, and every node keeps the winner per session, so a Worker can tell a returning machine what happened
  - the owner records `touched: true` (journaled, then pushed to every member it reaches) before the first tool call it lets through on a claimed session
  - `pub struct SideCopy { pub session: String, pub node: NodeId, pub base_set: String, pub extra_bytes: u64, pub repo_changed: bool, pub path: Option<String>, pub at_ms: i64, pub hidden: bool }`
  - `pub fn on_step_down(me: &NodeId, winner_claims: &[Claim], local: &dyn Fn(&str) -> LocalCopy) -> StepDown`; `StepDown { moved: Vec<(String, String)>, staying: Vec<String>, side_copies: Vec<SideCopy>, notice: Notice }`: a session another machine now owns gets `Moved { to }` and is stopped; one whose local transcript is longer than the claim's `base_len`, or whose folder changed since `base_set`, gets a side copy; sessions nobody else claimed stay here as worker-hosted sessions; the notice is `primary_back` "`<newer>` is primary" with Take Back and Keep `<newer>`
  - `pub enum ReturnChoice { Continue, Wait { for_node: String } }`; `pub fn on_return_with_primary_off(me: &NodeId, learned: &[Claim], mine: &[String]) -> Vec<(String, ReturnChoice)>`: the old primary learned a newer term from a Worker but can't reach the newer primary; a session whose newest claim (as the Workers hold it) is untouched continues here at once (a new claim, `n + 1`, made here); a touched one, or every one when no member holds a claim from the newer primary, is `Wait` (the default) and offers Continue From This Copy (a new claim here, after the maintainer confirms)
  - `pub fn on_meet(mine: &Claim, theirs: &Claim, my_len: u64) -> Meet`: the winner keeps the session; when the loser's copy grew past the winner's `base_len`, the loser records a side copy. So after Continue From This Copy, the other machine's newer copy is kept beside this one when it returns
  - Take Back: plan D's handover from the current primary to this machine with `aside: true`, so this machine's side copies are moved under `<data>/cluster/copies/` before the restore needs their paths
- [ ] **Step 1: Write the failing tests:** `higher_claim_wins_whatever_the_term_order`, `touched_is_recorded_before_the_first_call`, `newer_term_moves_claimed_sessions_and_stops_them`, `unclaimed_sessions_stay_worker_hosted`, `stepped_down_primary_keeps_side_copy` (a longer local transcript and a changed folder: both recorded, nothing deleted), `primary_back_notice_offers_take_back_and_keep`, `untouched_session_continues`, `touched_session_waits_by_default`, `no_claim_record_means_wait_for_every_session`, `continue_from_this_copy_claims_here`, `continue_from_this_copy_keeps_the_other`, `take_back_moves_side_copies_aside_first`, `hiding_a_side_copy_keeps_it`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::returns` and `cluster::claims`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `engine: claims, side copies and the return of an old primary`.

### Task 7: Terms, failover and returns in the core; the simulator proves them

**Zone:** maintainer. No CAP.

**Files:**
- Modify: `crates/engine/src/cluster/wire.rs` (`LeaseAsk { term, seq, via: Option<NodeId> }`, `LeaseGrant { term, seq, failover, via: Option<NodeId> }`, `Claims { claims }`; `Members` gains `claims`), `crates/engine/src/cluster/core/` (a new `core/failover.rs`: with failover on, the primary asks the target every `RENEW_MS`, directly or through one linked Can Be Master member that forwards it once (`via`); `Effect::WriteFence(FenceFile)` on every change of validity, gates, sessions or term; the target runs `auto_promote_due` each tick; on `SteppedDown` the core runs Task 6's step down and emits `Effect::StopSessions`; reachability of the primary counts answers through a peer), `crates/engine/src/cluster/policy.rs` (lease and claim messages between members, origin `Node`), `crates/engine/src/cluster/sim/` (scenario ops `Promote { node, confirmed }`, `SetFailover`, `ContinueHere`, `TakeBack`; a fake `claude --bg` per main session that keeps calling tools through its node's fence, including while its node is suspended and right after resume; clock drift on; failover delays as written, since simulated time is cheap)
- Test: `crates/engine/src/cluster/sim/sim_tests.rs`, `crates/engine/src/cluster/core/failover_tests.rs`

**Interfaces:**
- Produces `Violation::{TwoFencesPass { session, a, b, at }, CutStepRerun { session }, AutoPromoteBeforeFence { node, at }, SideCopyDeleted { session, node }, TwoOwnersAfterHeal { session }}`: I1 (one home per term, terms only grow), I2' (for each moving session, the fence passes on at most one node at any instant of simulated real time, except in a window that starts at a confirmed-risk promote or a Continue From This Copy: such windows are reported, not failed), I7 (a step marked cut is never started again before its item is answered), no automatic promote before the primary's gates ran out, side copies never removed, and one owner per session once the network heals.
- [ ] **Step 1: Write the failing tests:** `fence_passes_on_one_node_at_a_time` (every seed; failover 30m and 1h; suspend the primary from 1 s to 3 h; partitions; automatic promotes; resume), `failover_off_never_auto_promotes`, `two_promotes_one_home_per_term`, `old_primary_steps_down_on_contact`, `old_primary_back_with_newer_off_learns_from_workers`, `lease_through_a_third_machine_keeps_both_going`, `a_planted_early_promote_is_caught` (a test flag skips the deadline: `AutoPromoteBeforeFence` or `TwoFencesPass`), `confirmed_overlap_is_reported_not_failed`, `a_planted_side_copy_delete_is_caught`, `claims_converge_after_heal`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::sim` and `cluster::core`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** with `NOTCHLING_SIM_SEEDS=5000`. Expected: PASS.
- [ ] **Step 5: Commit** `cluster: failover, terms and returns in the node core, proven by the simulator`.

### Checkpoint 1: the fencing logic

- [ ] `cargo test -p notchling-engine --lib cluster::` with `NOTCHLING_SIM_SEEDS=5000`: PASS. fmt, clippy, guard: clean.
- [ ] Controller (and the maintainer, as a review gate before Task 8): read `failover.rs`, `fence.rs`, `claims.rs` and the trace of `a_planted_early_promote_is_caught`.

### Task 8: The fence in the daemon

**Zone:** maintainer. CAP-FILEWRITE, CAP-PROCESS (`claude stop`).

**Files:**
- Create: `crates/notchlingd/src/cluster_fence.rs`
- Test: `crates/notchlingd/src/cluster_fence_tests.rs`
- Modify: `crates/notchlingd/src/cluster_node.rs` (runs `WriteFence`, `StopSessions`), `crates/notchlingd/src/codex_ops.rs` (a gated Codex session's `codex.send` asks `fence::decide` first; a gate that runs out interrupts a running turn with `codex.stop`), `crates/local-api/src/wire.rs` (node-only `LEASE_ASK`, `LEASE_GRANT`, `CLUSTER_CLAIMS`)

**Interfaces:**
- Produces: `pub fn write_fence(data: &Path, f: &FenceFile) -> Result<(), String>` (temp file in the same folder, 0600, `rename`, a link at the path refused). Gates on the primary: a main session that may move in any set from the last one the failover target acknowledged onward is `Lease`; every other main session is `Free`; sessions another machine owns are `Moved`, sessions waiting for another machine `Waiting`. `valid_until`: `u64::MAX` with failover off or an unarmed target, else `PrimaryLease::gates_until`. At start with failover on, the daemon writes `valid_until = 0` for its `Lease` sessions before anything else, so they wait for the first lease; on a step down it writes `Moved` gates first, then stops those sessions through `control::session`.
- [ ] **Step 1: Write the failing tests:** `start_waits_for_first_lease_with_failover_on`, `failover_off_never_runs_out`, `movable_since_acked_set_is_lease_others_free`, `fence_file_is_atomic_and_0600`, `planted_link_is_refused`, `step_down_writes_moved_then_stops`, `codex_send_on_expired_fence_is_refused`, `expired_fence_interrupts_codex_turn`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib cluster_fence`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it and `codex_ops`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: write the fence and stop sessions that moved`.

### Task 9: The fence hook

**Zone:** maintainer (`notchlingd/src/hook*`, `engine/src/install/`). No new CAP. Live use gated by M8.

**Files:**
- Create: `crates/notchlingd/src/hook_fence.rs`
- Test: `crates/notchlingd/src/hook_fence_tests.rs`, `crates/notchlingd/tests/hook_fence_cli.rs`
- Modify: `crates/notchlingd/src/hook.rs` (`--fence` runs `hook_fence::answer` and nothing else), `crates/engine/src/install/hooks.rs` (a sixth slot `PreToolUse` / `*` with the command `notchling hook --fence` and `timeout: 30`; plans and removal treat it like the others), `crates/engine/src/install/` tests (the slot count)

**Interfaces:**
- Produces: `pub fn answer(stdin: &[u8], data: &Path, boot_id: &str, now: &dyn Fn() -> u64, sleep: &dyn Fn(u64)) -> Option<String>`: reads `session_id` from the event; `None` (print nothing, exit 0) on Pass; on Wait polls every `POLL_MS` up to `WAIT_MAX_MS`; on Deny the `PreToolUse` deny JSON (`hookSpecificOutput.permissionDecision = "deny"` with the reason). Any error reading stdin is a pass; a fence file that exists but can't be read is Wait, then Deny.
- [ ] **Step 1: Write the failing tests:** `pass_prints_nothing`, `deny_prints_pretooluse_deny`, `moved_session_is_denied_at_once`, `wait_then_pass_when_lease_arrives`, `wait_then_deny_after_twenty_seconds` (fake clock), `hook_denies_on_expired_fence_without_socket` (CLI e2e: no daemon, a fence file in a temp data root, the built `notchling hook --fence`), `install_plan_adds_the_fence_slot`, `remove_takes_it_out`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib hook_fence`, `cargo test -p notchling-notchlingd --test hook_fence_cli`, `cargo test -p notchling-engine --lib install::`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three and `hook_tests`. Expected: PASS.
- [ ] **Step 5: Commit** `hook: fence cluster sessions' tool calls`.

### Task 10: Promote, Take Back and Continue ops; inbox items

**Zone:** maintainer. No CAP.

**Files:**
- Create: `crates/notchlingd/src/cluster_promote.rs`, `crates/notchlingd/src/cluster_returns.rs`
- Test: `crates/notchlingd/src/cluster_promote_tests.rs`, `crates/notchlingd/src/cluster_returns_tests.rs`
- Modify: `crates/local-api/src/wire.rs` (`CLUSTER_PROMOTE = "cluster.promote"`, `CLUSTER_TAKE_BACK = "cluster.take_back"`, `CLUSTER_CONTINUE_HERE = "cluster.continue_here"`, `CLUSTER_SIDE_COPY_HIDE = "cluster.side_copy.hide"`), `crates/engine/src/inbox/model.rs` (`Kind::PrimaryUnreachable`, `Failover`, `PausedStep`, `NoPrimary`, `PrimaryBack`, `SessionWaits`; `ActionId::MoveHere`, `MoveAnyway`, `TakeBack`, `Keep`, `ContinueHere`, `WaitFor`), `crates/notchlingd/src/bridge_backend.rs` (the phone's `inbox.answer` with these actions runs the matching op, with the bridge and `acting` on), `crates/notchlingd/src/cluster_ops.rs` (`cluster.status` gains `failover`, `target`, `takesOverAt`, `sideCopies`, `waiting`)

**Interfaces:**
- Produces:
  - `cluster.promote {node, confirmed?}` (`OpSpec::APP`; from the phone through an inbox action or the Machines popup) → `{step}`, `{relayed: true}` or `{needsConfirm: <text>}` at once; progress as `cluster.changed`; an agent never reaches it
  - `cluster.take_back {}` (`OpSpec::APP`, on the old primary) → plan D's handover to this machine with `aside`
  - `cluster.continue_here {id}` (`OpSpec::APP`) → a new claim here for a waiting session, after the confirm
  - `cluster.side_copy.hide {session, node}` (`OpSpec::APP`) hides it from the list; nothing on disk changes
  - each Standby raises `primary_unreachable:<term>` after 60 s without the primary (directly or through a peer): "`<primary>` hasn't answered for 1 minute" with Move to `<self>` Now; on the failover target with failover on, the detail adds "`<self>` takes over at `<time>`". It closes when the primary is back. A `needsConfirm` answer turns it into the same item with Move Anyway and the text
  - `failover` notice when a promote finishes; `paused_step` per cut step: "`<old>` dropped while `git push` was running. Check, then Continue." with Continue (one reply "Continue" to the session) and Done
  - clients raise `no_primary` when no primary is reachable for 2 minutes (the phone's Machines popup, Task 12, lists machines from it)
  - on a stepped-down primary: `primary_back` "`<newer>` is primary" with Take Back and Keep `<newer>` (Keep or Dismiss closes it; Take Back stays in the cluster view)
  - per waiting session: `session_waits` "`<title>` waits for `<machine>`, which has newer work in it" with Wait for `<machine>` (closes the item, the session keeps waiting) and Continue From This Copy
- [ ] **Step 1: Write the failing tests:** `standby_raises_primary_unreachable_after_60s`, `target_item_says_when_it_takes_over`, `item_closes_when_primary_returns`, `promote_from_item_runs_saga`, `reachable_primary_answers_relayed`, `needs_confirm_turns_into_move_anyway`, `old_primary_raises_primary_back_with_take_back_and_keep`, `take_back_runs_the_handover_with_aside`, `continue_here_claims_the_session`, `hide_keeps_the_side_copy_on_disk`, `continue_sends_one_reply`, `agent_cannot_promote_take_back_or_continue`, `phone_promote_needs_acting`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib cluster_promote` and `cluster_returns`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both and `cargo test -p notchling-engine --lib inbox::`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: promote, Take Back and failover items`.

### Task 11: The fence hook on headless nodes

**Zone:** maintainer. No new CAP (`install.apply` exists).

**Files:**
- Modify: `crates/notchlingd/src/install_ops.rs` (`install.apply` forwarded from the primary to a headless Standby runs there with that node's own roots), `crates/engine/src/cluster/policy.rs` (`install.plan`, `install.apply` from `Home` origin `App`), `crates/app/src/cluster/view.rs` (a node row's "Install Hook" with the plan shown first)
- Test: `crates/notchlingd/src/install_ops_cluster_tests.rs`
- [ ] **Step 1: Write the failing tests:** `forwarded_apply_installs_on_node_with_install_on`, `install_off_refuses_forwarded_apply`, `only_app_origin_from_home`, `plan_shows_before_apply` (temp roots only; never the real `~/.claude`).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib install_ops_cluster_tests`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: install the hook on a headless Standby from the primary`.

### Task 12: The phone: the Machines popup and Move to <machine> Now

**Zone:** maintainer for `protocol/` and `crates/engine/src/bridge/`; auto for `apps/flutter/lib/features/inbox/`, a new `apps/flutter/lib/features/machines/` and new Flutter tests. No CAP.

**Files:** follow `.claude/skills/add-protocol-field/SKILL.md`:
- Modify: `protocol/agentconsole.v1.schema.json` (op `cluster.machines {}` → `[{id, name, online, canBeMaster, ready, setAt, canResume}]`, additive), fixtures, regenerate; `crates/engine/src/bridge/router.rs` and `bridge_backend.rs` (served from `cluster.status` on whichever machine the phone reached)
- Create: `apps/flutter/lib/features/machines/` (the popup)
- Modify: `apps/flutter/lib/features/inbox/` (the `move_here`, `move_anyway`, `take_back`, `continue_here` actions open a confirm before answering)
- Test: `crates/engine/src/bridge/router_machines_tests.rs`, `apps/flutter/test/machines_popup_test.dart`, `apps/flutter/test/promote_confirm_test.dart`

**Interfaces:**
- Produces: when no primary is reachable (the phone's own link, and `no_primary` from the machine it reached), a Machines popup lists online machines: a ready Can Be Master machine with a set says "Can resume old sessions · copy from 14:02"; one without a set "Controls jobs only"; a machine without Can Be Master is listed but can't be picked ("Can Be Master is off"). Picking one asks "Move to m1 now? Sessions continue there from 14:02. Work m5 did after that is kept on m5." and sends `cluster.promote {node}` through the machine the phone reached; `relayed` says "m5 is reachable through m1; nothing moved"; `needsConfirm` shows its text and asks again with Move Anyway. Continue From This Copy asks "Continue `<title>` here? The newer copy on `<machine>` is kept beside it."
- [ ] **Step 1: Write the failing tests:** `machines_op_lists_online_machines_with_set_time` (Rust), `old_phone_never_asks_machines_and_still_works`; Flutter `popup_lists_online_machines_with_snapshot_time`, `machine_without_set_says_controls_jobs_only`, `not_can_be_master_isnt_pickable`, `move_asks_before_sending`, `cancel_sends_nothing`, `relayed_answer_says_nothing_moved`, `needs_confirm_shows_the_risk_text`, `continue_from_this_copy_confirms`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib bridge::router_machines_tests`, `python3 tools/protocol/gen.py --check`, `~/flutter/bin/flutter test test/machines_popup_test.dart` and `test/promote_confirm_test.dart` (in `apps/flutter`). Expected: FAIL.
- [ ] **Step 3: Implement** schema first, regenerate, then the router and the Flutter screens.
- [ ] **Step 4: Run** them and `~/flutter/bin/dart analyze`. Expected: PASS.
- [ ] **Step 5: Commit** `phone: the Machines popup and Move to a machine now`.

### Task 13: The app: Move to, Take Back, side copies, waiting sessions

**Zone:** auto. No CAP.

**Files:**
- Modify: `crates/app/src/cluster/model.rs`, `view.rs` (a Standby row: Move to `<name>` Now runs plan D's handover when the primary is reachable and this plan's promote when it isn't, with the risk confirm only when the op asks for it; the old primary's own row: Take Back while another machine is primary; a Side Copies list: session, machine, since when, Open (the transcript read only in Chat) and Hide; waiting sessions with Wait and Continue From This Copy; the line "m1 takes over at 15:30 if m5 stays unreachable" while failover is on and the primary is silent), `crates/app/src/cluster/renders.rs`
- Test: `crates/app/src/cluster/promote_tests.rs`
- [ ] **Step 1: Write the failing tests:** `reachable_primary_move_runs_handover`, `unreachable_primary_move_runs_promote`, `risk_confirm_only_when_asked`, `take_back_on_old_primary_row`, `side_copy_rows_open_and_hide_never_delete`, `waiting_session_offers_wait_and_continue`, `takeover_time_shows_when_failover_on`, `paused_step_item_shows_continue_and_done`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib cluster::promote_tests`. Expected: FAIL.
- [ ] **Step 3: Implement**; `ViewSpec` `cluster-promote`.
- [ ] **Step 4: Run**, render (release) and look.
- [ ] **Step 5: Commit** `app: move the primary, Take Back and side copies`.

### Task 14: Docs and the ship gate

**Zone:** maintainer. No CAP.

**Files:**
- Create: `docs/decisions/XXXX-cluster-failover-and-fencing.md` (Proposed; the number is assigned at merge, in merge order: 0021 is search and 0022 localization): Automatic Failover off by default, 30 minutes or 1 hour, class Ask; the long lease from one failover target on boot clocks and the primary's own pause 2 minutes before; per-session gates at the tool call; terms with node ids; a manual promote relays when it can and asks for the risk when it can't show safety; claims, Take Back and side copies that are never deleted or merged; Codex turns fenced in the daemon only; Codex CLI sessions started outside Notchling can't be fenced and aren't main. `docs/cluster-failover.md` (what to do when the laptop is gone: the phone's Machines popup, what waits, Take Back, side copies, Continue From This Copy)
- Modify: AGENTS notes, `docs/local-api-ops.md`, `docs/feature-map.md`, `CHANGELOG.md`
- [ ] **Step 1: Write.**
- [ ] **Step 2: Run** the full checks, plus `~/flutter/bin/flutter test` and `~/flutter/bin/dart analyze`. Expected: clean, PASS.
- [ ] **Step 3: Commit** `docs: failover, promote and fencing`.
- [ ] **Ship gate:** plan F Task 1's decision accepted by the maintainer (spec: push decided before Promote ships).

### Checkpoint 2: the live check (maintainer only)

Gated by M5, M8 (the fence hook on m5 and m1), M12 (the phone reaches m1), M15 (failover setting for the second run). With a session working in a pushed repository on m5:
1. Failover Off: close the lid. The phone falls back to m1; after a minute it offers Move to m1 Now, with the risk text. Move; continue the session on m1. Open the lid: m5 says "m1 is primary · Take Back / Keep m1", and its next tool call in that session is denied ("continues on m1"). Take Back: the session comes home after its turn ends.
2. Failover 30 Minutes: close the lid for 40 minutes. m1 takes over at 30 without being asked; open the lid: the session had paused on m5 at 28 and now continues on m1.
3. m5 off, m1 primary, m1 off, then m5 on: m5 learns m1's term from a Worker; a session m1 never touched continues on m5; one it touched offers Wait for m1 and Continue From This Copy.
