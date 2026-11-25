# Cluster v1, plan C: Secrets and Worker Logins Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A job on a Worker gets the vault entries the maintainer attached to it, or asks for one by name, and only those its node is allowed; an entry marked Ask me first is released only after Allow in the inbox (Mac or phone); a value reaches the Worker as a mode-600 file that expires and is deleted at job end, is never in argv, a log, a snapshot or a mirrored transcript, and is masked in everything that streams back. Workers are signed in to Claude Code (setup-token), Codex and Antigravity as each tool allows, and nothing renews a login in use.

**Architecture:** Grants are pure decisions in `engine::cluster::grants` (allowlist, ask, expiry) plus a small private file store. The **vault node** (`ClusterState.vault`: the Mac the cluster was created on, m5, whose Keychain holds the vault; it never changes in v1) is the broker, and only while it is the primary: it reads values from its own vault through the existing `VaultOps` release path (vault switch, Ask me first, Always Allow), writes the audit line, then sends `grant.deliver` over the node link to the Worker running the job. While another machine is primary (after a failover or a move), no new secret is issued: a dispatch that attaches one waits, and a request from inside a running job is refused with the reason (decision 2026-11-14, proposal 8; copying the vault is a later decision). On the Worker, `notchling secret NAME -- cmd` keeps working: `vault.use` finds the caller's job from its process tree and answers from that job's grants, asking the vault node for one more by name when the allowlist permits. Inbox items from every node are mirrored to the primary and to every linked Can Be Master machine, so one list on the Mac and the phone holds them all even while the primary is silent, and an answer is forwarded to the node that owns the item.

**Tech Stack:** Rust (engine, notchlingd, local-api, GPUI app), the plan B node link, `agentconsole.v1` (no change), Flutter (no change beyond inbox kinds it already shows as notices).

**Spec:** `docs/superpowers/specs/2026-11-13-notchling-cluster-design.md`: "Scope: v1 first" (Logins, Secrets, Vault rows), "Secrets and the vault", "Logins and token renewal", "Maintainer's answers (2026-11-13)", invariants I3, I5, I6, data ownership ("Access tokens, delivered secrets"), and the overview's "Decisions 2026-11-14" (proposals 3a and 8), which win where they differ. Plan B must be merged first (node link, `RULES`, `ClusterNode`, jobs, `ClusterState.vault`). Plan A's inbox (`InboxOps`, `Responder`) is the approval path.

## Global Constraints

- Plan B's constraints hold (no comments, package names, pure core, fakes only, additive wire, files under 1,000 lines).
- A secret value never appears in: argv, a log line, the audit log, `cluster.db`, the journal, a `Publish`, an inbox item, a mirror file, a snapshot set (plan D), a `Debug` print, a panic message or a test's assertion text. Tests use planted marker values (`NL-MARKER-<n>`) and grep every output for them.
- Values travel only: home vault → home daemon memory → `grant.deliver` over pinned TLS → the Worker's grant file → the child environment of the one command `notchling secret` runs (today's vault rule), or `CLAUDE_CODE_OAUTH_TOKEN` in the environment of the `claude` process the daemon launches (Task 7). Nothing else.
- Grant files: `<data>/cluster/grants/<job>/<name>.json`, folder 0700, file 0600, created with `O_CREAT|O_EXCL|O_NOFOLLOW`, never through a link, never replaced in place (remove then create). Deleted at job end, at expiry and by `grant.revoke`; swept every 30 s.
- Expiry: the earlier of 12 hours from release and the job's end.
- Only the vault node releases, only while it is the primary, and only with its vault switch on (today's rule); a node with no app never turns its vault switch on (plan B: the cluster never sets `vault` or `accounts`). Standing grants already delivered (Task 7) keep working under another primary until they expire.
- Allowlist v1 (confirmed by the maintainer 2026-11-14, proposal 3a): per machine, empty by default, plus a per-machine **Any Secret** switch, off by default, that allows every vault entry by name on that machine.
- Logins: nothing in the cluster renews an OAuth token. A Worker whose login fails gets an inbox item telling the maintainer what to run there. Option (a) (Notchling as the one refresher) is not in v1.
- Approval items for grants are plan A's `secret_approval` kind with Allow Once, Always Allow and Deny (decision 0007's note of 2026-11-13: a phone may allow).

## Review Focus

1. A job asks for a name its node isn't allowed: refused, nothing read from the vault, audited. (Task 1 `name_off_the_allowlist_is_refused`, Task 4 `refused_grant_reads_no_value`.)
2. A value never leaks into argv, logs, audit, the journal, inbox items, mirrors or `Debug`. (Task 10 `marker_values_never_leave_their_paths` over every seed; Task 2 `grant_debug_hides_value`; Task 3 `tail_masks_json_escaped_multiline_value`.)
3. Two answers to one Ask (phone and Mac, or Mac and a mirrored copy): one release, the first answer. (Task 6 `answer_on_home_forwards_to_owner_once`, Task 10 invariant I5.)
4. A grant file outliving its job, or a planted link at its path. (Task 2 `planted_link_is_refused`, Task 5 `job_end_deletes_grants`, Task 10 invariant `no_grant_after_job_end`.)
5. A Worker renewing or overwriting a login: never. (Task 8 `probes_never_refresh`, Task 10 invariant I3.)
6. Another machine is primary (m1 after a failover): no value is read from any vault; an attached secret makes the dispatch wait, a request from a running job is refused naming the vault node. (Task 1 `grant_waits_while_vault_node_isnt_primary`, Task 4 `no_release_while_another_node_is_primary`, Task 10 `ReleasedOffPrimary`.)

---

### Task 1: Grants, the pure part

**Zone:** maintainer. No CAP.

**Files:**
- Create: `crates/engine/src/cluster/grants.rs`
- Test: `crates/engine/src/cluster/grants_tests.rs`
- Modify: `crates/engine/src/cluster/members.rs` (`Member.secrets: SecretAllow` with serde default), `crates/engine/src/cluster/state.rs` (`Command::SetAllow { node, allow }`; `vault` is plan B's), `crates/engine/src/cluster/policy.rs` (rules below)

**Interfaces:**
- Produces:
  - `pub struct SecretAllow { pub names: BTreeSet<String>, pub any: bool }` (names lowercased; `Default` is empty, `any: false`)
  - `pub struct GrantAsk<'a> { pub node: &'a Member, pub job: &'a Job, pub name: &'a str, pub exists: bool, pub asks: bool, pub vault_is_primary: bool, pub vault_name: &'a str }`
  - `pub enum GrantDecision { Release, Ask, Wait(String), Refuse(String) }`; `pub fn decide(a: &GrantAsk) -> GrantDecision`: job not Running or Dispatching → Refuse `"job <id> isn't running"`; node not Active → Refuse; name not allowed (not in `names`, `any` off) → Refuse `"<name> isn't allowed on <node>"`; the vault node isn't the primary → Wait `"<name> waits for <vault>: the vault is there"` (checked after the allowlist and before the vault is read, so nothing is read); entry missing → Refuse `"No secret named <name> in the vault on <vault>"`; `asks` → Ask; else Release
  - `pub const GRANT_TTL_MS: i64 = 12 * 3_600_000`; `pub fn expires_ms(released_ms: i64) -> i64`
  - `pub fn attachable(spec: &JobSpec, allow: &SecretAllow) -> Result<(), String>` (dispatch checks every attached name up front and names the first refused)
- `RULES` additions: from the vault node while it is `Home`, origin `Node`: `grant.deliver`, `grant.revoke` (a Worker refuses them from any other node, so a promoted m1 can't deliver); from any `Member`, origin `Agent`, to the vault node: `grant.request`; from `Client`, origin `App`, to the vault node: `vault.list`, `vault.save`, `vault.remove`; to the home: `cluster.allow.set`.

- [ ] **Step 1: Write the failing tests:** `allowlist_is_empty_and_any_is_off_by_default`, `name_off_the_allowlist_is_refused`, `any_allows_every_name`, `missing_entry_is_refused_with_its_name`, `ask_entry_needs_ask`, `grant_waits_while_vault_node_isnt_primary`, `off_list_name_is_refused_even_while_waiting` (the allowlist is checked first), `finished_job_gets_nothing`, `expiry_is_twelve_hours`, `dispatch_with_a_refused_attachment_fails_before_start`, `set_allow_is_journaled`, `names_are_case_insensitive`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::grants`. Expected: FAIL.
- [ ] **Step 3: Implement.** `Dispatch` (plan B Task 5) calls `attachable` before it records a job with secrets.
- [ ] **Step 4: Run** it and `cargo test -p notchling-engine --lib cluster::`. Expected: PASS.
- [ ] **Step 5: Commit** `engine: secret grants, allowlist and expiry`.

### Task 2: The grant file store

**Zone:** maintainer. CAP-FILEWRITE.

**Files:**
- Create: `crates/engine/src/cluster/grant_files.rs`
- Test: `crates/engine/src/cluster/grant_files_tests.rs` (`#[cfg(unix)]` for modes)

**Interfaces:**
- Produces:
  - `pub struct GrantValue(String)` (`Debug` and `Display` print `"<hidden>"`; `fn expose(&self) -> &str`; `Drop` overwrites the bytes)
  - `pub struct GrantFile { pub job: String, pub name: String, pub expires_ms: i64, pub value: GrantValue }` (serde for the file only: `{"v":1,"job","name","expiresMs","value"}`)
  - `pub struct GrantStore { root: PathBuf }`: `fn new(cluster_dir: &Path) -> GrantStore` (root `<cluster_dir>/grants`), `fn put(&self, g: &GrantFile) -> Result<(), String>`, `fn get(&self, job: &str, name: &str, now_ms: i64) -> Result<Option<GrantFile>, String>` (an expired one is deleted and `None`), `fn remove_job(&self, job: &str) -> Result<(), String>`, `fn remove(&self, job: &str, name: &str) -> Result<(), String>`, `fn sweep(&self, now_ms: i64, live_jobs: &BTreeSet<String>) -> Result<usize, String>` (deletes expired files and every job folder not in `live_jobs`)
  - `pub const EXCLUDED_DIRS: &[&str] = &["cluster/grants", "cluster/logins", "cluster/incoming"]` (plan D's snapshot planner refuses these)
  - job ids and names are checked (`^j-[0-9a-f]{12}$`, vault name rules) before any path is built
- [ ] **Step 1: Write the failing tests:** `put_then_get`, `files_are_0600_in_0700_folders`, `planted_link_is_refused` (a symlink at the job folder and at the file: `put` fails, the link's target is untouched), `expired_grant_is_deleted_on_get`, `sweep_removes_expired_and_dead_jobs`, `bad_job_id_builds_no_path` (`../x`), `grant_debug_hides_value`, `put_replaces_by_remove_then_create`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::grant_files`. Expected: FAIL.
- [ ] **Step 3: Implement** with `notchling_platform::files::write_private_no_follow` where it fits, `OpenOptions` with `custom_flags(O_EXCL | O_NOFOLLOW)` otherwise.
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `engine: private grant files with expiry`.

### Task 3: Masking what streams back

**Zone:** maintainer for `engine/src/vault/`; the tail and log wiring in `notchlingd`. No CAP.

**Files:**
- Modify: `crates/engine/src/vault/redact.rs` (`pub fn hide_forms(value: &str, mask: &str) -> BTreeMap<String, String>`: the value, its JSON-escaped form, and for a multi-line value each line of 8 characters or more, all mapped to the mask)
- Create: `crates/notchlingd/src/cluster_redact.rs` (one `Redactor` per (follower, file) and per shell log, built from the job's granted values)
- Test: `crates/engine/src/vault/tests_redact.rs` (new cases), `crates/notchlingd/src/cluster_redact_tests.rs`
- Modify: `crates/notchlingd/src/cluster_mirror.rs` (`node.tail` passes served bytes through the job's redactor; `next` stays a file offset), `crates/notchlingd/src/shell_jobs.rs` (the log is written through the redactor)

**Interfaces:**
- Produces: `pub struct TailMasks` with `fn for_job(&self, job: &str) -> Option<Redactor>`, `fn set(&self, job: &str, values: Vec<GrantValue>)`, `fn drop_job(&self, job: &str)`; `mask` is `"[secret <name>]"`.
- [ ] **Step 1: Write the failing tests:** `hide_forms_cover_json_escaped_and_lines`, `tail_masks_json_escaped_multiline_value` (a transcript line holding a PEM-like marker as JSON with `\n`), `mask_holds_across_chunk_boundaries` (a value split over two `node.tail` calls), `shell_log_is_masked_at_write`, `job_without_grants_is_served_as_is`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib vault::tests_redact` and `cargo test -p notchling-notchlingd --lib cluster_redact`. Expected: FAIL.
- [ ] **Step 3: Implement.** The mirror on the follower holds only masked bytes.
- [ ] **Step 4: Run** both and `cargo test -p notchling-notchlingd --lib cluster_mirror`. Expected: PASS.
- [ ] **Step 5: Commit** `cluster: mask granted values in tails and job logs`.

### Task 4: The vault node's grant broker

**Zone:** maintainer (`vault_ops.rs` is protected). CAP-NETWORK (delivery over the link), CAP-PROTECTED-TYPE likely (vault types outside their files): the maintainer gate applies.

**Files:**
- Create: `crates/notchlingd/src/grant_broker.rs`
- Test: `crates/notchlingd/src/grant_broker_tests.rs`
- Modify: `crates/notchlingd/src/vault_ops.rs` (extract `pub(crate) fn release(&self, who: &ReleaseFor, names: &[String]) -> Result<Vec<(VaultEntry, GrantValue)>, ApiError>` from `use_inner`: the same entry lookup, Ask me first through `approve` with `who` shown as the requester, Always Allow, switch checks; `vault.use` calls it and its tests pass unchanged), `crates/notchlingd/src/cluster_dispatch.rs` (attached secrets are released and delivered before the tool starts; a refusal or Deny fails the dispatch with the reason), `crates/local-api/src/wire.rs` (node-only `GRANT_DELIVER`, `GRANT_REVOKE`, `GRANT_REQUEST`; app ops `CLUSTER_ALLOW_SET = "cluster.allow.set"`)

**Interfaces:**
- Consumes: `grants::decide` (Task 1), `VaultOps::release`, `ClusterNode::local(Forward)` (plan B).
- Produces:
  - `pub struct ReleaseFor { pub node_name: String, pub job: String, pub command: String, pub folder: String }` (the approval card reads "`<command>` on `<node>` (job `<id>`) wants `<names>`")
  - `pub struct GrantBroker` (on the vault node only; anywhere else these ops are forwarded to the vault node): `fn grant(&self, job: &Job, names: &[String]) -> Result<Grant, ApiError>` with `enum Grant { Delivered(Vec<String>), Waiting(String) }` (decide each; on `Wait` read nothing and return `Waiting`; release the allowed ones through `VaultOps::release`; audit `grant.release:<node>:<job>:<name>` before sending; send `grant.deliver {job, name, value, expiresMs}`; return the names delivered), `fn revoke_job(&self, job: &str)` (`grant.revoke {job}` to the job's node)
  - a dispatch whose attached secrets get `Waiting` stays `Dispatching` with detail `"Waiting for <vault>"` and raises one `notice` item ("A job on `<node>` waits for a secret: the vault is on `<vault>`"); the primary retries it when the vault node holds the term again, and it fails when the maintainer stops the job
  - `grant.request {job, names, command, folder}` on the vault node (from the job's own node only: the sender must be the job's node) → `{delivered: [names]}`, or an error naming the refused one, or `waiting` with the reason at once (a command can't wait for hours; the Worker's `notchling secret` prints it)
  - `cluster.allow.set {node, names?, any?}` (`OpSpec::APP`, forwarded to the home) → `Command::SetAllow`
  - `vault.list` relayed from a client to the vault node returns names, flags and expiry only, never a value (today's shape); with the vault node offline the relay fails `"The vault is on <vault>, which is offline"`
- Note for the logging design (`agent/logging-design`, "commit before effect"): when the event log lands, the `grant.release` record is its first commit-before-effect event; until then the audit line is written first and a failed audit write stops the delivery.

- [ ] **Step 1: Write the failing tests** (fake vault store, recording node link, in-memory inbox):
  - `refused_grant_reads_no_value` (a counting store: zero reads), `released_grant_is_audited_before_delivery` (the audit sink records before the link sends), `failed_audit_stops_delivery`
  - `ask_entry_raises_secret_approval_and_allow_once_delivers`, `deny_fails_the_dispatch_with_reason`, `always_allow_stops_asking` (today's semantics through `release`)
  - `vault_switch_off_refuses`, `request_from_another_node_is_refused`
  - `no_release_while_another_node_is_primary` (a counting store: zero reads; the dispatch stays Dispatching with its detail and raises one notice), `waiting_dispatch_continues_when_vault_node_is_primary_again`, `in_job_request_while_waiting_fails_at_once`, `deliver_from_a_promoted_node_is_refused_by_the_worker`
  - `vault_list_relay_has_no_values`, `vault_use_unchanged` (the existing `vault_ops_tests` pass)
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib grant_broker` and `vault_ops`. Expected: FAIL for the new tests.
- [ ] **Step 3: Implement.** No lock held across the approval wait or the send.
- [ ] **Step 4: Run** both filters and `vault_ops_inbox_tests`, `vault_ops_pending_tests`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: the vault node releases granted secrets to a job's node`.

### Task 5: The Worker uses its grants

**Zone:** maintainer. CAP-FILEWRITE.

**Files:**
- Create: `crates/notchlingd/src/grant_use.rs`, `crates/notchlingd/src/grant_janitor.rs`
- Test: `crates/notchlingd/src/grant_use_tests.rs`, `crates/notchlingd/src/grant_janitor_tests.rs`
- Modify: `crates/notchlingd/src/vault_ops.rs` (`vault.use` on a node in a cluster that isn't the home: answer from `grant_use`), `crates/notchlingd/src/shell_jobs.rs` and the session start path of a dispatched job (record the job's root pid), `crates/notchlingd/src/daemon_wiring.rs`

**Interfaces:**
- Consumes: `GrantStore` (Task 2), `vault_caller::Processes` (parent chain), the caller's peer pid.
- Produces:
  - `pub struct JobRoots` (`fn set(&self, job: &str, pid: u32)`, `fn job_of(&self, procs: &dyn Processes, pid: u32) -> Option<String>`: walks the caller's ancestors to a recorded root)
  - `grant.deliver {job, name, value, expiresMs}` → `GrantStore::put` and `TailMasks::set`; `grant.revoke {job}` → `remove_job` and `TailMasks::drop_job`
  - `vault.use {names, folder, command}` from an agent on this node: the caller's job, else `"secret isn't available here: this command isn't part of a cluster job"`; names it holds → `{env, hide}` as today; a name it lacks → one `grant.request` to the vault node (waits up to the approval timeout), then answers; `waiting` → the error `"<name> waits for <vault>: the vault is there"`
  - `GrantJanitor` (every 30 s and on every job state change): `sweep(now, live_jobs)`
- [ ] **Step 1: Write the failing tests:** `use_answers_from_the_jobs_grants`, `caller_outside_any_job_is_refused`, `caller_in_another_job_gets_only_its_own`, `missing_name_requests_from_home_once`, `job_end_deletes_grants`, `revoke_deletes_at_once`, `expired_grant_is_not_used`, `deliver_for_unknown_job_is_refused`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib grant_use` and `grant_janitor`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: a job on a Worker uses its grants through notchling secret`.

### Task 6: One inbox for the cluster

**Zone:** maintainer (`inbox_ops.rs`, engine inbox). No CAP.

**Files:**
- Create: `crates/notchlingd/src/cluster_inbox.rs`
- Test: `crates/notchlingd/src/cluster_inbox_tests.rs`
- Modify: `crates/notchlingd/src/inbox_ops.rs` (`mirror(item)` and `unmirror(id)` for items owned by another node; `decide` on an item whose `source.node` is another node releases the inbox lock and forwards `inbox.answer` to that node, waits up to 5 s, and returns its result), `crates/engine/src/cluster/policy.rs` (`inbox.answer` and `inbox.list` from `Client` origin `App` and from `Home` origin `App` relaying a phone; `node.inbox` from any `Member` origin `Node` to the home and to any Can Be Master member)

**Interfaces:**
- Produces:
  - a node sends each `inbox.changed` and `inbox.removed` of its own items to the home and to every Can Be Master member it is linked with, as `node.inbox {event, item}`; each mirrors it with `id` qualified (`<node>/<id>`) and `source.node` set; a reconnect resends the node's open items. So a Standby holds every open item while the primary is silent (plan D's relay serves them to the phone, plan F pushes them).
  - `inbox.answer` on a qualified id is forwarded to its node, which decides under its own lock (first answer wins stays local to the owner, wherever the answer came from); the mirrors change when the owner's `inbox.changed` arrives
  - a client node's `inbox.list` is the home's list when linked, else its own list with the mirrors it holds; pushes follow the same source
- [ ] **Step 1: Write the failing tests:** `worker_item_appears_on_home_qualified`, `worker_item_appears_on_standby_too`, `answers_from_home_and_standby_take_one` (the same item answered through both mirrors at once), `answer_on_home_forwards_to_owner_once` (two answers at once: the owner takes the first; the second gets `answered: false`), `owner_offline_answer_fails_plainly`, `reconnect_resends_open_items`, `client_lists_home_items_when_linked_else_its_own`, `decide_holds_no_lock_while_forwarding` (a forward that blocks doesn't block `inbox.list`).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib cluster_inbox` and `inbox_ops`. Expected: FAIL for the new tests.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both and `inbox_ops_more_tests`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: inbox items from every node in one list on the home`.

### Task 7: Claude Code login on Workers

**Zone:** maintainer (`control/launcher`, `machines/`). CAP-FILEWRITE. Gated live by M4, M7 (overview).

**Files:**
- Create: `crates/notchlingd/src/cluster_logins.rs`
- Test: `crates/notchlingd/src/cluster_logins_tests.rs`
- Modify: `crates/engine/src/control/launcher.rs` (a launch may carry `env: BTreeMap<String, GrantValue>` set on the child only, never logged; `FakeLauncher` records the keys, not values), `crates/engine/src/machines/steps_secret.rs` (for a machine that is a cluster node, `claude_login` says "Sent by the cluster" and passes when the node reports a standing login), `crates/engine/src/cluster/grant_files.rs` (a standing grant: job `"login"`, no job end, the vault entry's own expiry)

**Interfaces:**
- Produces:
  - on approve and whenever the entry changes, the vault node (while primary) delivers the node's `Machine.claude_secret` (default `claude-worker`) as a standing grant to `<data>/cluster/logins/claude.json` (0600); it is in `EXCLUDED_DIRS`; under another primary the last delivered one keeps working until it expires
  - the Worker's daemon sets `CLAUDE_CODE_OAUTH_TOKEN` only in the environment of a `claude` it launches (dispatch, `delegate.run`); never in a shell job's environment
  - `Versions.logins[Claude]` true when the standing grant exists and hasn't expired
  - 14 days before the entry's expiry (`VaultEntry` expiry), the vault node raises `login_needed` "Claude Code's token for `<node>` expires on `<date>`" with Done
- [ ] **Step 1: Write the failing tests:** `standing_login_is_delivered_on_approve`, `claude_launch_gets_token_in_env_only` (FakeLauncher: key present, argv clean), `shell_job_env_has_no_token`, `login_file_is_0600_and_excluded`, `expiry_notice_14_days_before`, `machines_step_passes_for_cluster_node`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib cluster_logins` and `cargo test -p notchling-engine --lib machines::`. Expected: FAIL for the new tests.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: Claude Code's long-lived login on Workers`.

### Task 8: Codex and Antigravity logins on Workers; nothing renews

**Zone:** maintainer. CAP-PROCESS (status probes).

**Files:**
- Create: `crates/engine/src/cluster/logins.rs`, `crates/engine/src/machines/steps_node_logins.rs`
- Test: `crates/engine/src/cluster/logins_tests.rs`, `crates/engine/src/machines/tests_node_logins.rs`
- Modify: `crates/notchlingd/src/cluster_versions.rs` (fills `Versions.logins`), `crates/engine/src/cluster/versions.rs` (`compat(Dispatch(t))` refuses `"<Tool> isn't signed in on <node>"` when `logins[t] == false`), `crates/engine/src/inbox/model.rs` (`Kind::LoginNeeded`)

**Interfaces:**
- Produces:
  - `pub fn codex_signed_in(exit: i32, stdout: &str) -> bool` (from `codex login status`), `pub fn agy_signed_in(files: &dyn Fn(&Path) -> bool, home: &Path) -> bool` (its credential file exists; the path is read from the Antigravity notes in `crates/engine/src/tools/agy/`)
  - probes run on the Worker itself, at most every 10 minutes, with a 5 s timeout; never `login`, `refresh` or any command that writes a credential
  - a `login_needed` item when a probe flips to false: "Codex isn't signed in on `<node>`. Run codex login --device-auth there." with Done
  - Machines `NODE_CODEX_LOGIN`, `NODE_AGY_LOGIN`: probe the same, fix `HowTo` with the command the maintainer runs in their own terminal
- [ ] **Step 1: Write the failing tests:** `codex_status_reads_signed_in`, `agy_signed_in_by_file`, `dispatch_refuses_signed_out_tool`, `probe_flip_raises_login_needed_once`, `probes_never_refresh` (the scripted runner refuses any argv holding `login` other than `login status`, or `refresh`), `machines_login_steps_are_howto`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::logins` and `machines::tests_node_logins`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both and `cargo test -p notchling-notchlingd --lib cluster_versions`. Expected: PASS.
- [ ] **Step 5: Commit** `cluster: Codex and Antigravity login status on Workers`.

### Checkpoint 1

- [ ] `cargo test -p notchling-engine --lib cluster::`, `cargo test -p notchling-notchlingd`: PASS.
- [ ] fmt, clippy, guard lint: clean. `rg -n "NL-MARKER" crates --glob '!*tests*'` finds nothing.
- [ ] Controller reads the audit lines of one release and one refusal from `grant_broker_tests`' recorded sink.

### Task 9: The app: allowlist, logins, attaching secrets, the home's vault

**Zone:** auto (`crates/app`). No CAP.

**Files:**
- Create: `crates/app/src/cluster/secrets.rs`, `crates/app/src/cluster/secrets_view.rs`
- Test: `crates/app/src/cluster/secrets_tests.rs`
- Modify: `crates/app/src/cluster/model.rs` (a node row's logins: Claude Code, Codex, Antigravity each Signed In, Signed Out or Not Installed; a "Secrets…" row action: the machine's allowlist, empty by default, and its Any Secret switch, off by default), `crates/app/src/sheets/new_session.rs` (Secrets on a Worker: a picker of the vault's names, only those the node allows; while the vault node isn't primary the picker says "New secrets wait for `<vault>`"), `crates/app/src/vault/view.rs` (when this Mac isn't the vault node: a "Vault on `<vault>`" tab that lists that vault's entries through the relay, add and remove with the same form)

**Interfaces:**
- Consumes: `cluster.allow.set`, relayed `vault.list`/`vault.save`/`vault.remove`, `cluster.dispatch {secrets}`.
- Produces: `SecretsModel::new(names: &[String], allow: &SecretAllow) -> SecretsModel`, `fn toggle(&mut self, name: &str)`, `fn set_any(&mut self, on: bool)`, `fn request(&self) -> Value`; part names `"Node Secrets"`, `"Remote Vault"`.
- [ ] **Step 1: Write the failing tests:** `new_machine_allows_nothing_and_any_is_off`, `toggle_builds_allow_request`, `any_hides_the_list`, `picker_offers_only_allowed_names`, `picker_says_new_secrets_wait_off_primary`, `node_row_shows_login_states`, `remote_vault_tab_only_when_not_vault_node`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib cluster::secrets_tests`. Expected: FAIL.
- [ ] **Step 3: Implement** with kit tokens and `menu_panel`; a `ViewSpec` `cluster-secrets`.
- [ ] **Step 4: Run**, then render `--render cluster-secrets` (release) and look.
- [ ] **Step 5: Commit** `app: node secrets, logins and the home's vault`.

### Task 10: Simulator: grants and logins

**Zone:** maintainer. No CAP.

**Files:**
- Modify: `crates/engine/src/cluster/sim/world.rs` (a vault of marker values on the vault node, grant requests, job ends, `bump_term` moving the primary away and back), `sim/host.rs` (a fake vault and grant store per node; every byte any node logs, persists, publishes, mirrors or puts in argv goes through a recorder), `sim/invariants.rs`
- Test: `crates/engine/src/cluster/sim/sim_tests.rs` (new cases)

**Interfaces:**
- Produces `Violation::{SecretLeaked { node, path }, GrantOnWrongNode { job, node }, GrantOutlivedJob { job, node }, RefreshOnWorker { node }, AnswerTwice { item }, ReleasedOffPrimary { node, term }}`; `check` adds I3 (no node runs a refresh; the fake runner records any), I5 across nodes (an item answered once, through any mirror), I6 (a marker appears only in the vault node's vault, a `grant.deliver` to the job's node, that node's grant store and the child env record), "no grant 30 s after its job ended", and "a release happens only on the vault node while it holds the current term".
- [ ] **Step 1: Write the failing tests:** `marker_values_never_leave_their_paths` (all seeds), `a_planted_leak_is_caught` (a test flag logs one value: `SecretLeaked`), `no_grant_after_job_end`, `answer_twice_is_caught` (planted), `a_planted_release_off_primary_is_caught`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::sim`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** with `NOTCHLING_SIM_SEEDS=2000`. Expected: PASS.
- [ ] **Step 5: Commit** `engine: simulator checks secrets, grants and logins`.

### Task 11: Docs

**Zone:** maintainer. No CAP.

**Files:**
- Create: `docs/decisions/XXXX-cluster-secrets-and-logins.md` (Proposed; the number is assigned at merge, in merge order: 0021 is search and 0022 localization): the vault stays in m5's Keychain and only m5, while primary, issues grants (another primary issues none and jobs wait; copying the vault is a later decision); allowlist per machine, empty by default, with Any Secret per machine, off by default; files not env or argv for delivery; the two places a value meets an environment; Workers renew nothing; option (a) later.
- Modify: `docs/policies/privacy.md` (secret values cross to a Worker for a job, over pinned TLS, deleted at job end), `docs/local-api-ops.md`, `crates/notchlingd/AGENTS.md`, `crates/engine/AGENTS.md`, `docs/feature-map.md`, `CHANGELOG.md`
- [ ] **Step 1: Write.**
- [ ] **Step 2: Run** the full checks (plan B Task 19 Steps 2 and 3). Expected: clean, PASS.
- [ ] **Step 3: Commit** `docs: cluster secrets and Worker logins`.

### Checkpoint 2: the live check (maintainer only)

Gated by M7 (Worker logins) and plan B's live check, with m5 primary. A shell job on a Worker attached to a test entry runs `notchling secret test-entry -- sh -c 'test -n "$TEST_ENTRY" && echo set'`; the output says `set`, the mirror shows no value, and `ls <data>/cluster/grants` on the Worker is empty a minute after the job ends. An Ask me first entry asks on the phone.
