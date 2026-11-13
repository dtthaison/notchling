# Cluster v1, plan B: Nodes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Machines join one cluster over Tailscale with pinned mutual TLS; every node's daemon knows the members, their roles and versions; the maintainer sees a cluster view, starts a Claude Code, Codex or Antigravity session or a shell job on a picked Worker, and follows and answers it in Chat and on the phone.

**Architecture:** A pure, sans-IO cluster core in `engine::cluster` (ids, versions, the node wire, the node tier, a journaled cluster state and a `NodeCore` state machine that takes inputs and returns effects), checked under a deterministic multi-node simulator. A new crate `crates/cluster-net` carries node messages over TCP and TLS 1.3, both ends pinned by certificate fingerprint, only on Tailscale addresses. `notchlingd` runs the core (`cluster_node.rs`), forwards a closed list of ops between nodes, merges remote sessions into `sessions.list` with host-qualified ids and mirrors the transcript Chat opens. Control (membership, dispatch) goes through the home; data (transcript tails, replies, prompts) goes straight to the node that owns the session.

**Tech Stack:** Rust (engine, platform, local-api, notchlingd, new `cluster-net`, GPUI app with `crates/kit`), rustls 0.23 with ring, rcgen 0.14, rusqlite, `agentconsole.v1` (additive), Flutter.

**Spec:** `docs/superpowers/specs/2026-11-13-notchling-cluster-design.md`: "Scope: v1 first" (step 1 and the "Must exist first" row), "Work placement", "Sending a session to a Worker", "Node connections and joining", "Software architecture" (principles, ports, invariants), review issues 3, 5, 10, 12, 13, 14. Read the cross-plan notes and "Decisions 2026-11-14" in `2026-11-13-cluster-v1-overview.md` first; where they differ from the spec, the decisions win.

**Version sync:** this plan builds on version sync Phase 1 Tasks 1 to 4 (`origin/agent/version-sync-plans`, `2026-11-14-versions-p1-see.md`: `engine::versions`, `versions::mac::detect`, `versions::mismatch`). Its conflicts C1 to C3 are settled (overview): a tool-version mismatch at dispatch warns until version sync Phase 6; the node's Mac probe calls `versions::mac::detect`; `cluster::Tool` and `versions::Tool` both stay, with `tool_of`.

## Global Constraints

- Words: "home" in code and in these plans is the maintainer's **primary** (the laptop m5 when the cluster is made). A **Standby** is any member with **Can Be Master** on that isn't the primary (m1, and any other machine the maintainer marks). User-facing text says Primary, Standby, Worker and Can Be Master, never "home".
- Terms fence everything: every `Hello`, `Call` and `Members` carries the sender's term; a node refuses a call stamped with a term older than the highest it has seen, and a node that sees a newer term adopts it (a home that sees one steps down: plan E does the session side).

- No comments in code, doc comments included; the why goes in the crate's `AGENTS.md` (plan A ruling R2).
- Package names: `notchling-engine`, `notchling-notchlingd`, `notchling-local-api`, `notchling-platform`, `notchling-app`, new `notchling-cluster-net`; one test filter per `cargo test` run (plan A ruling R1).
- Tests never run ssh, `tailscale`, `gh` or a real `claude`, never touch Keychain or a real login. Nodes in tests are in-process (`MemTransport`, the simulator). Only `cluster-net`'s own tests and one e2e test open sockets, and only on loopback.
- `engine::cluster` is pure: no `std::time`, no randomness, no `HashMap` iteration, no file or socket. Time, randomness and storage are passed in. `BTreeMap`/`BTreeSet` only.
- Node identity: the first 16 hex characters of the SHA-256 of the node certificate's DER (64 lowercase hex), a v3 ECDSA P-256 certificate Notchling makes once per data folder and never replaces.
- Network: TCP port 47823, TLS 1.3 only, both sides pinned. Listen and accept only on Tailscale addresses (100.64.0.0/10, fd7a:115c:a1e0::/48). Loopback only when `NOTCHLING_CLUSTER_LOOPBACK=1` and the daemon runs from a cargo target folder (the `NOTCHLING_APP_TIER_PATHS` rule).
- The node wire is additive: `NODE_PROTO = 1`, unknown fields ignored, an unknown message type decodes as `Unknown` and is answered `unknown`. One JSON object per line, at most 1 MiB.
- No file turns a switch on (0007). A node with no app starts with every switch off; only the home sends `acting`, `bridge` or `install` on, from the maintainer's app-tier choice stored per node; `accounts` and `vault` never come from the cluster.
- Every join is approved by the maintainer in the inbox (Mac or phone).
- Remote session ids are `<node id>/<id>` (0012); bare ids stay local.
- An unknown or offline node is an error, never a fallback to another node.
- Files under 1,000 lines (600 warns); tests in `*_tests.rs`. Every new folder under `crates/*/src/` gets its `docs/feature-map.md` row in the task that makes it (`GUARD-FEATURE-MAP`).
- Local API and `agentconsole.v1` change additively only; protocol changes follow `.claude/skills/add-protocol-field`.
- Only crates already in `Cargo.lock`; `deny.toml` unchanged.
- User-facing text short and plain, titles in Title Case.

## Review Focus

1. A removed node that reconnects with its old certificate: the TLS handshake fails, nothing is dispatched to it, its sessions leave every list. (Task 5 `removed_member_is_refused_and_never_dispatched`, Task 9 `unpinned_peer_fails_handshake`, Task 11 `removed_node_is_dropped_and_wipes_on_contact`.)
2. A dispatch retried over a flaky link, duplicated or replayed: exactly one job starts. (Task 5 `same_key_dispatch_is_one_job`, Task 6 `duplicate_call_runs_once`, Task 7 seeds with duplicates on.)
3. A Worker whose tool version differs from the reference Mac: the dispatch goes with a warning naming both versions (version sync's call 10, conflict C1: it refuses only from version sync Phase 6, which adds Run Anyway then); a missing tool, another node protocol or schema still refuses. (Task 3 `dispatch_warns_on_a_tool_mismatch_until_phase_6`, `absent_tool_still_refuses`; Task 13 `tool_mismatch_dispatches_with_a_warning`.)
6. A call from a home whose term is older than one the receiver has seen: refused with the newer term, never run; the stale home steps down when it hears it. (Task 6 `older_term_call_is_refused`, `home_seeing_newer_term_steps_down`; Task 7 `a_planted_stale_call_is_caught`.)
4. A Worker forwarding a call it may not make (start work on another node, relay the app tier): refused and audited. (Task 4 `worker_may_not_dispatch`, `non_client_claiming_app_is_refused`; Task 12 `forward_from_worker_is_refused`.)
5. Chat on a remote session whose node goes offline: the row says Offline, the conversation stays readable from the mirror, Reply is off with the reason. (Task 16 `offline_remote_row_disables_reply`, Task 17 `remote_row_uses_mirror_transcript`.)

---

### Task 1: Decision record, cluster v1 nodes

**Zone:** maintainer (`docs/decisions/`). No CAP.

**Files:**
- Create: `docs/decisions/XXXX-cluster-v1-nodes.md` (Proposed). `XXXX` is a placeholder: the number is assigned at merge, in merge order (0019 and 0020 are on unmerged branches, 0021 is search and 0022 localization); the PR that merges it renames the file and every reference.
- Modify: `docs/decisions/README.md` (index row)

- [ ] **Step 1: Write the record** from the template in `docs/decisions/README.md`. Decision bullets, each one line:
  - Roles in v1: one primary (the home), Standbys, Workers; no Witness, no election. A member's **Can Be Master** switch (default on for Macs, off for other machines; needs disk encryption, plan D) makes it a Standby; any number may have it. The primary is the node the cluster was created on (the laptop m5) until a planned move (plan D) or a promote (plan E). Automatic failover is a setting, off by default (plan E). Terms only grow and fence every call.
  - Identity: a node certificate per data folder (v3 ECDSA P-256, made once), node id = 0012's host id rule applied to it. The phone bridge keeps its own certificate; the logging design's `host` should use the node id once one exists.
  - Transport: TCP and TLS 1.3 on 47823, bound only to Tailscale addresses, both sides pinned. No QUIC in v1 (the daemon has no async runtime).
  - Topology: every node links to the primary; a client node (a Mac with the app, marked `client` at approval) and every Can Be Master node link to every member, so Standbys hear each other and the primary without it. Control goes through the primary; data goes to the session's owner.
  - Node tier: a closed table of ops one node may ask of another, by the sender's role and the origin's tier. A client node relays its own app's tier; a Worker relays the agent tier at most.
  - Headless switches: `acting`, `bridge`, `install` follow the home's stored choice for a node with no app attached, set only by an app-tier click; `accounts` and `vault` never.
  - Joins need the maintainer's Allow in the inbox; join tokens are one-time, 10 minutes.
  - Relaying: the home and client nodes relay other nodes' sessions to the phone and the app with qualified ids on the wire.
- [ ] **Step 2:** Consequences name what it amends: 0012's "Nothing is replicated" (superseded by the cluster design), 0012's "computers never relay" and "on the wire nothing changes" (amended), 0007's tiers (a third origin, the node, through the table only). Status: Proposed, settled by the maintainer merging plan B.
- [ ] **Step 3: Commit** `docs: decision record, cluster v1 nodes (proposed, number at merge)`.

### Task 2: Platform: boot clock, boot id, Tailscale addresses, load

**Zone:** maintainer (`crates/platform`). No CAP expected (reads only).

**Files:**
- Create: `crates/platform/src/clock.rs`, `crates/platform/src/net_if.rs`, `crates/platform/src/load.rs`
- Test: `crates/platform/src/clock_tests.rs`, `crates/platform/src/net_if_tests.rs`, `crates/platform/src/load_tests.rs`
- Modify: `crates/platform/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub fn boot_ms() -> Result<u64, Error>`: a clock that counts sleep. macOS `mach_continuous_time` scaled by `mach_timebase_info`; Linux `clock_gettime(CLOCK_BOOTTIME)`; Windows `Error::Unsupported`.
  - `pub fn boot_id() -> Result<String, Error>`: changes on every boot. Linux `/proc/sys/kernel/random/boot_id` trimmed; macOS `sysctl kern.boottime` as `<sec>.<usec>`.
  - `pub fn is_tailscale(ip: &IpAddr) -> bool` and `pub fn tailscale_addrs() -> Vec<IpAddr>` (getifaddrs on unix, empty elsewhere).
  - `pub struct Load { pub cpu_pct: u8, pub mem_used_mb: u64, pub mem_total_mb: u64 }`, `pub struct LoadSampler`, `fn new() -> LoadSampler`, `fn sample(&mut self) -> Result<Load, Error>` (CPU is the busy share since the previous sample; the first sample reports 0).
  - pure parsers `pub fn parse_proc_stat(text: &str) -> Option<(u64, u64)>` (busy, total) and `pub fn parse_meminfo(text: &str) -> Option<(u64, u64)>` (used MB, total MB).

- [ ] **Step 1: Write the failing tests:**
  - `is_tailscale_ranges`: 100.64.0.1 and 100.127.255.254 yes; 100.128.0.1, 10.0.0.1, 127.0.0.1 no; fd7a:115c:a1e0::1 yes; ::1 and fe80::1 no.
  - `boot_ms_moves_forward` (two reads, the second not smaller; `#[cfg(unix)]`).
  - `boot_id_is_stable_within_a_run` (`#[cfg(unix)]`).
  - `parses_proc_stat_cpu_delta`: two fixed `cpu  …` lines whose busy delta is a quarter of the total delta give 25 through `LoadSampler`'s pure step.
  - `parses_meminfo`: `MemTotal: 8000000 kB`, `MemAvailable: 2000000 kB` give used 5859, total 7812.
- [ ] **Step 2: Run** `cargo test -p notchling-platform clock` (then `net_if`, `load`). Expected: FAIL, modules missing.
- [ ] **Step 3: Implement.** macOS load: `host_statistics64` for CPU ticks and `sysctl hw.memsize` with `vm_statistics64` for memory; keep the AppKit-free path.
- [ ] **Step 4: Run** the three filters. Expected: PASS.
- [ ] **Step 5: Commit** `platform: boot clock, boot id, Tailscale addresses and load`.

### Task 3: Cluster ids and versions

**Zone:** maintainer (`crates/engine`). No CAP.

**Files:**
- Create: `crates/engine/src/cluster/mod.rs`, `crates/engine/src/cluster/id.rs`, `crates/engine/src/cluster/versions.rs`
- Uses (version sync Phase 1 Tasks 1 and 4, `2026-11-14-versions-p1-see.md`): `crates/engine/src/versions/` (`Tool`, `Version` with the strict pattern, `cli_output`, `mismatch`, `Mismatch::text`). If those tasks haven't merged when this one starts, build them first, on this branch, exactly as that plan writes them (its call V3: the strict pattern wins); version sync then only extends them.
- Test: `crates/engine/src/cluster/id_tests.rs`, `crates/engine/src/cluster/versions_tests.rs`
- Modify: `crates/engine/src/lib.rs` (`pub mod cluster;`), `docs/feature-map.md` (a row for `crates/engine/src/cluster/`)

**Interfaces:**
- Produces:
  - `pub struct NodeId(String)` (serde transparent, `Ord`): `fn from_fingerprint(fp: &str) -> Option<NodeId>` (64 lowercase hex in, first 16 out), `fn as_str(&self) -> &str`, `Display`.
  - `pub fn fingerprint(der: &[u8]) -> String` (SHA-256, 64 lowercase hex).
  - `pub struct Qualified { pub node: NodeId, pub id: String }`, `fn parse(s: &str) -> Option<Qualified>` (`^[0-9a-f]{16}/.+$`, split at the first `/`), `pub fn qualify(node: &NodeId, id: &str) -> String`.
  - `pub enum Role { Home, Standby, Worker }` (derived, never stored: Task 5's `role_of`) and `pub enum Tool { Claude, Codex, Agy, Shell }` (what a job runs), serde `snake_case`.
  - `pub fn tool_of(t: Tool) -> Option<versions::Tool>` (conflict C3: both enums stay; `Shell` has none; Claude, Codex and Agy map one to one).
  - `cluster::versions::Versions { pub notchling: String, pub proto: u32, pub features: Vec<String>, pub schema: u32, pub os: String, pub arch: String, pub tools: BTreeMap<versions::Tool, versions::Version>, pub logins: BTreeMap<versions::Tool, bool> }` (`logins` filled by plan C; empty here; `tools` from `versions::mac::detect`, Task 10).
  - `pub enum Purpose { Link, Dispatch(Tool), Promote }`, `pub enum Compat { Ok, Warn(String), Refuse(String) }`.
  - `pub fn compat(reference: &Versions, node: &Versions, node_name: &str, purpose: Purpose) -> Compat`.
  - consts `NODE_PROTO: u32 = 1`, `CLUSTER_SCHEMA: u32 = 1`, `LINUX_CLAUDE_PROVEN: bool = false` (flipped only after the maintainer's proof M4 in the overview).
- Rules of `compat`, in order (version-sync design §17.5 is the source for the tool rule):
  - any purpose: `proto` differs → Refuse `"<node> speaks node protocol <n>, this Mac <m>"`.
  - `Link`: else Ok.
  - `Dispatch(t)`: feature `"dispatch"` missing → Refuse; `schema` differs → Refuse; tool not installed → Refuse `"<Tool> isn't installed on <node>"`; `os == "windows"` and `t == Claude` → Refuse `"Claude Code doesn't run on Windows nodes in v1"`; `os == "linux"`, `t == Claude`, `!LINUX_CLAUDE_PROVEN` → Refuse `"Claude Code on Linux isn't proven yet"`; tool version differs from the reference's → **Warn** with `versions::mismatch`'s text `"<node> runs <Tool> <v1>, the Mac <v2>"` (conflict C1: warn, never refuse, until version sync Phase 6 Task 5 makes it a refusal with Run Anyway); `notchling` differs → Warn `"<node> runs Notchling <v1>, the Mac <v2>"`; two warnings join with `"; "`; else Ok. `Shell` skips the tool rules.
  - `Promote`: `notchling`, `schema` and Claude Code's version must equal the reference's, else Refuse naming the first that differs.

- [ ] **Step 1: Write the failing tests:**
  - `node_id_is_first_16_of_fingerprint`, `rejects_uppercase_or_short_fingerprint`
  - `qualified_splits_at_first_slash` (`0123456789abcdef/codex:a/b` → node `0123456789abcdef`, id `codex:a/b`), `bare_uuid_is_not_qualified`
  - `tool_of_maps_each_tool_and_shell_has_none`
  - `link_refuses_other_proto`, `dispatch_refuses_missing_tool`, `absent_tool_still_refuses`, `dispatch_warns_on_a_tool_mismatch_until_phase_6` (asserts `Warn` with the exact text `"m1 runs Claude Code 2.1.323, the Mac 2.1.324"`; the same name version sync Phase 1 Task 10 planned, so that task shrinks to `delegate --machine`'s stderr line and no test is weakened), `dispatch_warns_on_notchling_mismatch`, `dispatch_refuses_linux_claude_until_proven`, `promote_needs_same_build`
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::`. Expected: FAIL.
- [ ] **Step 3: Implement** the types and `compat`. `Tool`'s display names: Claude Code, Codex, Antigravity, Shell.
- [ ] **Step 4: Run** `cargo test -p notchling-engine --lib cluster::` and `cargo test -p notchling-engine --lib versions::`, then `PYTHONPATH=tools/guard python3 -m guard featuremap`. Expected: PASS, clean.
- [ ] **Step 5: Commit** `engine: cluster ids, versions and the compatibility check`.

### Task 4: The node wire and the node tier

**Zone:** maintainer. No CAP.

**Files:**
- Create: `crates/engine/src/cluster/wire.rs`, `crates/engine/src/cluster/policy.rs`
- Test: `crates/engine/src/cluster/wire_tests.rs`, `crates/engine/src/cluster/policy_tests.rs`

**Interfaces:**
- Consumes: `NodeId`, `Versions`, `Role` (Task 3).
- Produces:
  - `pub enum Msg` (serde `tag = "type"`, `snake_case`):
    - `Hello { node: NodeId, cluster: Option<String>, versions: Versions, boot_id: String, term: Option<Term> }` (the highest term the sender has seen)
    - `HelloOk { node: NodeId, cluster: String, term: Term, home: NodeId }`
    - `Refused { reason: String, term: Option<Term>, home: Option<NodeId> }` (`reason: "stale_term"` carries the newer term and its home)
    - `Join { secret: String, name: String, fingerprint: String, addrs: Vec<String>, port: u16, versions: Versions }`
    - `JoinPending { item: String }`, `JoinOk { cluster: String, term: Term, home: NodeId, members: Vec<Member> }`
    - `Members { term: Term, home: NodeId, members: Vec<Member> }`
    - `Call { id: u64, op: String, args: Value, origin: Origin, term: Term }` (the sender's current term), `Reply { id: u64, ok: Option<Value>, err: Option<WireError> }`
    - `Event { topic: String, name: String, data: Value }`
    - `Ping { boot_ms: u64 }`, `Pong { echo: u64 }`
    - `#[serde(other)] Unknown`
  - `pub struct Origin { pub node: NodeId, pub tier: OriginTier }`, `pub enum OriginTier { App, Agent, Node }`
  - `pub struct WireError { pub code: String, pub message: String }`
  - `pub const MAX_LINE: usize = 1 << 20`; `pub fn encode(msg: &Msg) -> Result<Vec<u8>, String>` (one line ending `\n`, over `MAX_LINE` is an error); `pub fn decode(line: &[u8]) -> Result<Msg, String>`
  - `pub enum Sent { Home, Client, Member }` (the sender's standing as the receiver sees it)
  - `pub struct Rule { pub op: &'static str, pub from: &'static [Sent], pub tier: OriginTier, pub id_field: Option<&'static str> }`
  - `pub const RULES: &[Rule]` and `pub fn check(op: &str, sender: &SenderFacts, origin: &Origin) -> Result<&'static Rule, Denied>`; `SenderFacts { id: NodeId, role: Role, client: bool, state: MemberState, is_home: bool }` (`role` from Task 5's `role_of`); `enum Denied { NotAMember, Removed, NotAllowed, TierTooHigh }` (the term check is the core's, Task 6, before `check`)
  - `pub fn cluster_may_set(switch: &str) -> bool` (true for `acting`, `bridge`, `install`)
- `RULES` for plan B (later plans append to this table; keep it one table):
  - from `Home`, origin `Node` or `App`: `session.new`, `codex.send`, `agy.open`, `agy.send`, `shell.run`, `shell.stop`, `session.stop`, `codex.stop`, `agy.cancel`, `node.switch.set`, `node.put`, `node.put.end`, `workspace.prepare`, `workspace.return`
  - from `Home`, origin `Agent`: `delegate.run`
  - from `Home`, origin `App` (relaying its own app or a phone on its bridge): everything in the `Client` list below
  - from `Client`, origin `App` (each with its `id_field`): `session.reply`, `session.answer`, `session.permission`, `session.interrupt`, `session.stop`, `codex.send`, `codex.stop`, `agy.send`, `agy.cancel`, `agy.permission`, `agy.conversation`, `prompts.list`, `node.tail`, `shell.stop`; and to the home only: `cluster.dispatch`, `cluster.member.set`, `cluster.remove`
  - from any `Member`, origin `Node`: `node.sessions`, `node.versions`, `node.load`; origin `Agent` or `App`, to the home only: `cluster.join.token` (every join still needs Allow)
  - from any `Member`, origin `Agent`, to the home only: `cluster.dispatch` with `tool` limited to the delegate form (Task 13 enforces it)

- [ ] **Step 1: Write the failing tests:**
  - `encodes_one_line_and_round_trips`, `unknown_type_decodes_as_unknown`, `line_over_1mib_is_refused`, `bad_json_is_an_error`, `call_and_hello_carry_term`
  - `home_may_start_sessions`, `worker_may_not_dispatch` (a Worker's `session.new` is `NotAllowed`), `client_app_origin_may_reply`, `non_client_claiming_app_is_refused` (`TierTooHigh`), `removed_member_is_refused`, `unknown_op_is_not_allowed`
  - `cluster_never_sets_vault_or_accounts`
  - `every_rule_names_a_known_op` (each op in `RULES` is in a list the test holds of local-api op names plus the node-only ones; keeps typos out)
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::wire` and `cluster::policy`. Expected: FAIL.
- [ ] **Step 3: Implement.** `Member` is defined in Task 5; until then declare it in `members.rs` with the fields Task 5 lists, so this task compiles.
- [ ] **Step 4: Run** both. Expected: PASS.
- [ ] **Step 5: Commit** `engine: node wire and the node tier table`.

### Task 5: Cluster state: members, joins, jobs

**Zone:** maintainer. No CAP.

**Files:**
- Create: `crates/engine/src/cluster/members.rs`, `crates/engine/src/cluster/jobs.rs`, `crates/engine/src/cluster/join.rs`, `crates/engine/src/cluster/state.rs`
- Test: `crates/engine/src/cluster/state_tests.rs`, `crates/engine/src/cluster/jobs_tests.rs`, `crates/engine/src/cluster/join_tests.rs`

**Interfaces:**
- Produces:
  - `pub struct Member { pub id: NodeId, pub name: String, pub fingerprint: String, pub addrs: Vec<String>, pub port: u16, pub client: bool, pub can_be_master: bool, pub reference: bool, pub accepts_delegation: bool, pub state: MemberState, pub versions: Option<Versions>, pub desired: Desired, pub joined_ms: i64 }`
  - `pub fn role_of(state: &ClusterState, id: &NodeId) -> Role`: `Home` when `id == state.home`, `Standby` when the member has `can_be_master`, else `Worker`. The role is never stored, so a promote or a move changes it by changing `home` only.
  - `pub fn can_be_master_default(os: &str) -> bool` (true for `macos`, false otherwise: the maintainer's default, on for the two Macs, off for workers; plan D gates it on disk encryption)
  - `pub enum MemberState { Pending, Active, Removed }`, `pub struct Desired { pub acting: bool, pub bridge: bool, pub install: bool }`
  - `pub struct JobSpec { pub node: NodeId, pub tool: Tool, pub cwd: String, pub args: Value, pub secrets: Vec<String> }` (`secrets` used by plan C)
  - `pub struct Job { pub id: String, pub key: String, pub spec: JobSpec, pub state: JobState, pub term: Term, pub created_ms: i64, pub session: Option<String>, pub detail: Option<String> }`, `pub enum JobState { Dispatching, Running, Done, Failed, Interrupted, Lost }`; job id `"j-"` + first 12 hex of SHA-256 of the key
  - `pub struct Term { pub n: u64, pub node: NodeId }` (`Ord` by `n`, then `node`; a new cluster starts at `Term { n: 1, node: <home> }`; plan E relies on the node half to keep two promotes apart)
  - `pub struct ClusterState { pub id: String, pub name: String, pub term: Term, pub home: NodeId, pub vault: NodeId, pub members: BTreeMap<NodeId, Member>, pub jobs: BTreeMap<String, Job>, pub seq: u64 }` (`vault` is the node the cluster was created on, whose Keychain holds the vault; it never changes in v1: plan C)
  - `pub enum Command { Create { id: String, name: String, home: Member }, ProposeJoin { member: Member, item: String }, Approve { node: NodeId, client: bool, can_be_master: bool }, Deny { node: NodeId }, Remove { node: NodeId }, SetMember { node: NodeId, can_be_master: Option<bool>, client: Option<bool>, reference: Option<bool>, accepts_delegation: Option<bool>, desired: Option<Desired> }, Versions { node: NodeId, versions: Versions }, Dispatch { key: String, spec: JobSpec }, JobState { id: String, state: JobState, session: Option<String>, detail: Option<String> } }`
  - `pub struct JournalEntry { pub seq: u64, pub at_ms: i64, pub term: Term, pub command: Command }`
  - `pub fn apply(state: &mut ClusterState, cmd: Command, now_ms: i64) -> Result<(JournalEntry, Vec<Change>), CommandError>`; `pub enum Change { Member(Member), Job(Job), Cluster { term: Term, home: NodeId } }`; `pub enum CommandError { NotActive(NodeId), Unknown(NodeId), Removed(NodeId), BadTransition, Exists }`
  - `pub struct JoinToken { pub cluster: String, pub home: NodeId, pub fingerprint: String, pub addrs: Vec<String>, pub port: u16, pub secret: String }`, `pub fn encode_token(t: &JoinToken) -> String` (`"nlj1."` + base64url JSON, no padding), `pub fn decode_token(s: &str) -> Result<JoinToken, String>`
  - `pub struct Tokens` with `fn issue(&mut self, secret: String, now_ms: i64)` and `fn take(&mut self, secret: &str, now_ms: i64) -> bool` (one-time, `TOKEN_TTL_MS = 600_000`)
- Rules: a `Dispatch` with a key already in `jobs` returns no change and no journal entry beyond the read; dispatch to a member that isn't Active is `NotActive`; `JobState` moves only forward (Dispatching → Running → Done/Failed/Interrupted/Lost; Dispatching → Failed); `Remove` marks Removed and keeps the row; the home can't be removed; `Create` sets `vault` = `home` and gives the home `can_be_master: true`.

- [ ] **Step 1: Write the failing tests:**
  - `create_sets_home_and_term_one`, `create_sets_vault_to_home`, `terms_order_by_n_then_node`, `join_is_pending_until_approved`, `deny_drops_the_pending_member`, `home_cannot_be_removed`
  - `role_follows_home_and_can_be_master` (home is Home whatever its switch; a member with the switch is Standby; without, Worker; any number may be Standby), `can_be_master_defaults_on_for_macs_only`
  - `removed_member_is_refused_and_never_dispatched`
  - `same_key_dispatch_is_one_job`, `job_ids_come_from_keys`, `job_state_moves_forward_only`
  - `token_round_trips`, `token_is_one_time_and_expires` (taken once at 0; again false; a fresh one at 600_001 false)
  - `seq_grows_by_one_per_entry`
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::state` (and `cluster::jobs`, `cluster::join`). Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `engine: journaled cluster state, joins and jobs`.

### Task 6: `NodeCore`, the sans-IO node

**Zone:** maintainer. No CAP.

**Files:**
- Create: `crates/engine/src/cluster/core/mod.rs`, `core/link.rs` (hello, handshake, liveness, dialing), `core/forward.rs` (calls and replies), `core/control.rs` (home-side commands, membership pushes)
- Test: `crates/engine/src/cluster/core/core_tests.rs`
- Modify: `docs/feature-map.md` (the `cluster/core/` folder)

**Interfaces:**
- Consumes: Tasks 3 to 5.
- Produces:
  - `pub enum Input { Tick { boot_ms: u64, wall_ms: i64 }, Linked { peer: NodeId }, Unlinked { peer: NodeId }, Msg { from: NodeId, msg: Msg }, Local { id: u64, cmd: LocalCmd }, LocalDone { call: u64, result: Result<Value, WireError> } }`
  - `pub enum LocalCmd { Create { name: String, me: Member }, IssueToken { secret: String }, Approve { node: NodeId, client: bool, can_be_master: bool }, Deny { node: NodeId }, Remove { node: NodeId }, SetMember { .. as Command::SetMember }, Dispatch { key: String, spec: JobSpec }, Forward { node: NodeId, op: String, args: Value, origin: Origin }, JoinWith { token: JoinToken, me: Member } }`
  - `pub enum Effect { Send { to: NodeId, msg: Msg }, Dial { to: NodeId, addrs: Vec<String>, port: u16, fingerprint: String }, Close { peer: NodeId }, Pins { fingerprints: Vec<String> }, Persist { entry: JournalEntry, changes: Vec<Change> }, Publish { topic: String, name: String, data: Value }, RunLocal { call: u64, op: String, args: Value, origin: Origin }, Answer { local: u64, result: Result<Value, WireError> }, Raise { item: crate::inbox::Item }, SteppedDown { term: Term, home: NodeId }, Wipe }`
  - `pub struct NodeCore`: `fn new(me: NodeId, versions: Versions, state: Option<ClusterState>, client: bool) -> NodeCore`, `fn step(&mut self, input: Input) -> Vec<Effect>`, `fn state(&self) -> Option<&ClusterState>`, `fn online(&self) -> BTreeSet<NodeId>`, `fn highest_term(&self) -> Option<&Term>`, `fn status(&self) -> Value` (the `cluster.status` reply body of Task 10)
  - consts `PING_MS = 5_000`, `OFFLINE_MS = 20_000`, `CALL_MS = 30_000`, `DIAL_BACKOFF_MS = (1_000, 60_000)`, `DEDUPE_MS = 300_000`
- Behavior:
  - On `Linked`, send `Hello`. On `Hello`, `compat(Link)`; Refuse → `Refused` + `Close`; a sender that isn't an Active member gets `Refused { reason: "removed" }` if Removed, else `Refused { reason: "not a member" }`. A Removed node that receives `Refused { "removed" }` from its pinned home emits `Wipe`.
  - Terms: the core keeps the highest term it has seen (journaled with `Persist`, so a restart never forgets it). An incoming `Call` whose `term` is older than that is answered `WireError { code: "stale_term" }` with the newer term and home in its message, never run, and a Publish `cluster.refused`; a `Hello` with an older term gets `Refused { reason: "stale_term", term, home }`. Any message carrying a newer term is adopted: the state's `term` and `home` change, and a node that was the home emits `SteppedDown { term, home }` (plan E stops its moving sessions and keeps their side copies) and stops acting as home at once (no `Dispatch`, no `Members` push).
  - Ping every `PING_MS`; a peer silent for `OFFLINE_MS` goes offline (Publish `cluster.changed`).
  - A node not linked to the home dials it with backoff; a client node and every Can Be Master node dial every Active member (so Standbys hear the home and each other, and an old home that comes back hears a newer term from any member); Workers only answer.
  - `Pins` lists the fingerprints of every Active member (and the joiner during a join) whenever membership changes.
  - The home pushes `Members` to every linked node after a membership change.
  - `Forward` sends a `Call` with a fresh id and the current term; a `Reply` answers the local id; no reply in `CALL_MS` answers `timeout`. An incoming `Call` passes the term check, then `policy::check`, or is answered with a `WireError { code: "forbidden" }` and a Publish `cluster.refused` (the daemon audits it). A repeated `(from, id)` within `DEDUPE_MS` is answered from the first result and never run twice.
  - `Dispatch` on the home: `apply`; a new job to an online node sends the tool's op (`session.new`, `codex.send` with `draft_folder`, `agy.open`, `shell.run`); an offline node is an error `"<node> is offline"`; `compat(Dispatch)` Refuse is an error, Warn goes with the job and its reply carries `warning`; the reply moves the job to Running with its session id or to Failed. On a non-home node `Dispatch` becomes a `Forward` of `cluster.dispatch` to the home.

- [ ] **Step 1: Write the failing tests** (drive two or three cores by hand, routing `Send` effects):
  - `hello_with_other_proto_is_refused`, `removed_node_gets_removed_and_wipes`
  - `peer_goes_offline_after_20s_silence`, `node_dials_home_with_backoff`, `can_be_master_node_dials_every_member`, `worker_only_answers`
  - `call_reply_correlates_and_times_out`, `incoming_call_from_worker_to_dispatch_is_refused`, `duplicate_call_runs_once`
  - `older_term_call_is_refused` (a Worker that saw term 3 refuses a term 2 call with `stale_term` naming term 3's home, and runs nothing), `newer_term_is_adopted`, `home_seeing_newer_term_steps_down` (emits `SteppedDown`, then refuses to dispatch), `highest_term_survives_restart`
  - `home_pushes_members_on_change`, `pins_follow_membership`
  - `dispatch_to_offline_node_is_refused`, `dispatch_from_client_goes_through_home`, `tool_mismatch_dispatches_with_a_warning`
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::core`. Expected: FAIL.
- [ ] **Step 3: Implement.** Keep each file under 600 lines.
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `engine: NodeCore, the cluster node as a state machine`.

### Task 7: The deterministic simulator

**Zone:** maintainer. No CAP.

**Files:**
- Create: `crates/engine/src/cluster/sim/mod.rs`, `sim/net.rs`, `sim/world.rs`, `sim/host.rs`, `sim/invariants.rs`, `sim/scenario.rs`
- Test: `crates/engine/src/cluster/sim/sim_tests.rs`
- Modify: `crates/engine/src/cluster/mod.rs` (`#[cfg(test)] pub mod sim;`), `docs/feature-map.md`

**Interfaces:**
- Consumes: `NodeCore`, `Input`, `Effect` (Task 6).
- Produces (later plans add scenario ops and invariants here):
  - `pub struct Rng(u64)` (SplitMix64): `fn new(seed: u64)`, `fn below(&mut self, n: u64) -> u64`, `fn chance(&mut self, pct: u8) -> bool`
  - `pub struct World`: `fn new(seed: u64) -> World`, `fn add_node(&mut self, name: &str, client: bool) -> NodeId`, `fn create(&mut self, home: NodeId)`, `fn join(&mut self, node: NodeId, can_be_master: bool)`, `fn partition(&mut self, a: NodeId, b: NodeId)`, `fn heal(&mut self)`, `fn crash(&mut self, n: NodeId)`, `fn restart(&mut self, n: NodeId)`, `fn suspend(&mut self, n: NodeId)`, `fn resume(&mut self, n: NodeId)`, `fn local(&mut self, n: NodeId, cmd: LocalCmd) -> u64`, `fn run_for(&mut self, ms: u64)`, `fn trace(&self) -> &[String]`, `fn now(&self) -> u64`; test-only `fn bump_term(&mut self, to: NodeId)` (a stand-in for plan E's promote, so the term rules are checked before E exists)
  - Each node has its own boot clock: it keeps counting while suspended (the node processes nothing), restarts at 0 with a new boot id after a reboot, and drifts by at most 1 ms per 10 s (seeded).
  - `SimNet`: delay 1 to 50 ms, `drop_pct`, `dup_pct`, partitions; all seeded; delivery order deterministic for a seed.
  - `SimHost` runs `RunLocal`: records every started job as `(node, job, sim_ms)` and answers with a fake session id; `Persist` goes to a per-node in-memory journal that survives `crash`/`restart`.
  - `pub enum Violation { TwoHomesInTerm { term: Term, a: NodeId, b: NodeId }, TermWentDown { node: NodeId }, StaleTermAccepted { node: NodeId, call: String }, JobStartedTwice { job: String }, JobOnTwoNodes { job: String }, DispatchToRemoved { job: String, node: NodeId }, KeyTwoJobs { key: String } }`
  - `pub fn check(world: &World) -> Result<(), Violation>` (I1 as far as plan B can see: one home per term, terms never go down per node, no node runs a call stamped with a term older than one it had seen; I2: a job starts on one node, once; plus membership: nothing dispatched after a removal starts; a key maps to one job)
  - `pub struct Failure { pub seed: u64, pub violation: Violation, pub trace_tail: Vec<String> }`, `pub fn run_seed(seed: u64, steps: usize) -> Result<(), Failure>`: four nodes (two Macs with Can Be Master, one of them the home and a client; two Workers), weighted random ops (dispatch with a reused key 20% of the time, partition, heal, crash, restart, suspend, resume, remove a Worker, `bump_term` to the other Mac 5% of the time, advance time), `check` after every step.

- [ ] **Step 1: Write the failing tests:**
  - `same_seed_same_trace`
  - `invariants_hold_over_seeds`: seeds `0..N`, `N` from `NOTCHLING_SIM_SEEDS` (default 200), 300 steps each, under 20 s in debug; on failure the message holds the seed and the last 40 trace lines, and `NOTCHLING_SIM_SEED=<s>` replays one seed.
  - `a_planted_double_start_is_caught`: a test-only `SimHost` flag starts every job twice; `run_seed` returns `JobStartedTwice` (the checker isn't vacuous).
  - `a_planted_stale_call_is_caught`: a test-only core flag skips the term check on Workers; after a `bump_term` and a heal, `run_seed` returns `StaleTermAccepted`.
  - `suspended_node_clock_keeps_counting`, `rebooted_node_gets_new_boot_id`
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::sim`. Expected: FAIL.
- [ ] **Step 3: Implement.** No `HashMap`, no system time; the trace is plain text with sim time, node name and the effect.
- [ ] **Step 4: Run**, then `NOTCHLING_SIM_SEEDS=2000 cargo test -p notchling-engine --lib cluster::sim -- invariants_hold_over_seeds`. Expected: PASS.
- [ ] **Step 5: Commit** `engine: deterministic cluster simulator with invariants`.

### Task 8: `cluster.db`, and every table classed as node or cluster

**Zone:** maintainer. CAP-FILEWRITE (a new database file).

**Files:**
- Create: `crates/engine/src/cluster/persist.rs`, `crates/engine/src/store_class.rs`
- Test: `crates/engine/src/cluster/persist_tests.rs`, `crates/engine/src/store_class_tests.rs`
- Modify: `crates/engine/src/store.rs` (extract `pub fn open_db(path: &Path, migrate: &dyn Fn(&Connection) -> Result<(), String>) -> Result<Connection, String>` from `open`, same flags: 0600, WAL, busy timeout, symlink refused, retry), `crates/engine/src/lib.rs`

**Interfaces:**
- Consumes: `ClusterState`, `JournalEntry`, `Change` (Task 5).
- Produces:
  - `pub const CLUSTER_DIR: &str = "cluster"`, `pub const CLUSTER_DB: &str = "cluster.db"`; the folder is 0700
  - `pub fn load(cluster_dir: &Path) -> Result<Option<ClusterState>, String>` (no file → `None`, creating nothing)
  - `pub fn save(cluster_dir: &Path, entry: &JournalEntry, changes: &[Change]) -> Result<(), String>` (one IMMEDIATE transaction: the journal row and each changed row)
  - `pub fn wipe(cluster_dir: &Path) -> Result<(), String>` (removes `cluster.db` and its `-wal`/`-shm`, never follows a link)
  - schema `user_version = CLUSTER_SCHEMA`: `meta(key PRIMARY KEY, value)`, `state(key PRIMARY KEY, value)` (id, name, term, home, vault, the highest term seen), `members(id PRIMARY KEY, value)`, `jobs(id PRIMARY KEY, key UNIQUE, value)`, `journal(seq INTEGER PRIMARY KEY, at_ms INTEGER, term INTEGER, command TEXT)`
  - a `user_version` above `CLUSTER_SCHEMA` → `Err("cluster.db is from a newer Notchling")`, never written
  - `store_class::Class { Node, Cluster }`, `fn table_class(name: &str) -> Option<Class>`, `fn doc_class(name: &str) -> Option<Class>`: in `notchling.db` `meta`, `settings`, `drafts`, `docs` are Node; `inbox` is Cluster (it moves into `cluster.db` in plan D); every doc constant listed in the overview's "notchling.db today" is Node

- [ ] **Step 1: Write the failing tests:**
  - `cluster_db_round_trips_state`, `missing_cluster_db_loads_none_and_creates_nothing`, `newer_schema_is_refused_untouched`, `journal_seq_only_grows`, `wipe_removes_the_database_only`
  - `every_table_in_notchling_db_has_a_class` (open a temp `notchling.db`, read `sqlite_master`, each table has a class; a table added later without a class fails here)
  - `every_doc_constant_has_a_class` (the test lists the doc constants: `LIMITS_DOC`, `CACHE_DOC`, `CODEX_DOC`, `ANTIGRAVITY_DOC`, `IDS_DOC`, second opinions' `DOC`, `LIST_DOC`, `USAGE_DOC`, `OFFERS_DOC`, `REGISTRY_DOC`, `INDEX_DOC`)
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::persist` and `store_class`. Expected: FAIL.
- [ ] **Step 3: Implement.** The existing `store` tests must pass unchanged after the extraction.
- [ ] **Step 4: Run** both filters and `cargo test -p notchling-engine --lib store`. Expected: PASS.
- [ ] **Step 5: Commit** `engine: cluster.db, and notchling.db's tables classed as node or cluster`.

### Task 9: `cluster-net`: identity, pinning, links

**Zone:** maintainer (new crate, `Cargo.toml`). CAP-NETWORK, CAP-FILEWRITE.

**Files:**
- Create: `crates/cluster-net/Cargo.toml` (deps: `rustls` 0.23.46 ring/std, `rustls-pki-types`, `ring`, `rcgen` 0.14 ring/pem/crypto, `sha2`, `libc` on unix), `src/lib.rs`, `src/identity.rs`, `src/pin.rs`, `src/frame.rs`, `src/addr.rs`, `src/link.rs`, `src/listen.rs`, `src/mem.rs`, `crates/cluster-net/AGENTS.md`
- Test: `src/identity_tests.rs`, `src/pin_tests.rs`, `src/frame_tests.rs`, `src/link_tests.rs`
- Modify: `docs/feature-map.md` (the crate's row), `crates/AGENTS.md` (crates table)

**Interfaces:**
- Produces:
  - `pub struct Identity { pub cert_der: Vec<u8>, pub fingerprint: String, key_der: Vec<u8> }` (`Debug` prints the fingerprint only); `pub fn load_or_make(dir: &Path) -> Result<Identity, Error>`: `node.pem` and `node.key` (0600, folder 0700), made once (ECDSA P-256, CN `notchling-node`, 20 years) and never replaced; an existing file that doesn't parse is an error, never overwritten; a link in place is refused
  - `pub struct Pins` (`Arc<RwLock<BTreeSet<String>>>`): `fn set(&self, fps: impl IntoIterator<Item = String>)`, `fn contains(&self, fp: &str) -> bool`
  - `pub fn server_config(id: &Identity, pins: Pins) -> Result<Arc<rustls::ServerConfig>, Error>` (TLS 1.3 only; client certificate required; accepted only if its fingerprint is pinned; handshake signatures checked with ring's verifiers)
  - `pub fn client_config(id: &Identity, expect: &str) -> Result<Arc<rustls::ClientConfig>, Error>` (the server's fingerprint must equal `expect`)
  - `pub fn allowed(ip: &IpAddr, loopback_ok: bool) -> bool` (Tailscale ranges via `notchling_platform::net_if::is_tailscale`, loopback only if `loopback_ok`)
  - `pub trait LinkEvents: Send + Sync { fn line(&self, peer_fp: &str, line: Vec<u8>); fn closed(&self, peer_fp: &str); }`
  - `pub trait Conn: Send + Sync { fn send(&self, line: Vec<u8>) -> Result<(), Error>; fn peer_fingerprint(&self) -> String; fn close(&self); }`
  - `pub trait Transport: Send + Sync { fn dial(&self, addrs: &[String], port: u16, expect: &str, events: Arc<dyn LinkEvents>) -> Result<Arc<dyn Conn>, Error>; fn listen(&self, port: u16, accept: Arc<dyn Fn(Arc<dyn Conn>) + Send + Sync>, events: Arc<dyn LinkEvents>) -> Result<Listener, Error>; }`
  - `pub struct TcpTls { identity: Identity, pins: Pins, loopback_ok: bool }` (listens on every `tailscale_addrs()` address, or 127.0.0.1 when `loopback_ok`; connect timeout 5 s; a reader thread per link; a writer queue of 256 lines, full → close)
  - `pub struct MemTransport` (in-process: a shared registry of listeners by `(addr, port)`; fingerprints and pins checked the same way; `fn new_net() -> Arc<MemNet>`, `fn on(net, identity_fp, pins) -> MemTransport`)
  - `frame::LineReader` with the 1 MiB cap; a longer line closes the link

- [ ] **Step 1: Write the failing tests:**
  - `identity_is_made_once_and_kept` (same fingerprint on the second load; files 0600, folder 0700), `unreadable_identity_is_an_error_not_replaced`, `link_in_place_is_refused`
  - `pinned_pair_connects_on_loopback` (two identities, both pinned, a line each way), `unpinned_peer_fails_handshake` (the server pins someone else: the handshake fails on both ends), `wrong_server_fingerprint_fails`, `tls12_is_refused`
  - `line_over_cap_closes_link`, `tailscale_only_unless_loopback_ok`
  - `mem_transport_delivers_in_order_and_checks_pins`
- [ ] **Step 2: Run** `cargo test -p notchling-cluster-net`. Expected: FAIL.
- [ ] **Step 3: Implement.** Write `crates/cluster-net/AGENTS.md` notes for: the never-replaced identity, pinning in place of chain validation, loopback only in dev builds, the writer queue.
- [ ] **Step 4: Run** `cargo test -p notchling-cluster-net`, `cargo deny check`. Expected: PASS, clean.
- [ ] **Step 5: Commit** `cluster-net: node identity, pinned TLS links, in-memory transport`.

### Checkpoint 1: the core and the transport

- [ ] `cargo test -p notchling-engine --lib cluster::`, `cargo test -p notchling-cluster-net`, `cargo test -p notchling-platform`: PASS.
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `PYTHONPATH=tools/guard python3 -m guard lint` and `featuremap`: clean.
- [ ] Controller reads `RULES` (Task 4) against the overview's node tier list, and one failing-seed message from `a_planted_double_start_is_caught`, before the daemon tasks start.

### Task 10: The node driver, `cluster.status` and the versions probe

**Zone:** maintainer (`notchlingd`, `local-api`). CAP-NETWORK (listener), CAP-FILEWRITE (identity, database). No CAP-PROCESS of its own: tool versions come from version sync's `versions::mac::detect` (files only) and its daemon's agy probe (conflict C2).

**Files:**
- Create: `crates/notchlingd/src/cluster_node.rs`, `crates/notchlingd/src/cluster_ops.rs`, `crates/notchlingd/src/cluster_versions.rs`
- Test: `crates/notchlingd/src/cluster_node_tests.rs`, `crates/notchlingd/src/cluster_ops_tests.rs`, `crates/notchlingd/src/cluster_versions_tests.rs`
- Modify: `crates/local-api/src/wire.rs` (`ops::CLUSTER_STATUS = "cluster.status"`; `topics::CLUSTER = "cluster"`, `topics::CLUSTER_CHANGED = "cluster.changed"`, `topics::CLUSTER_REFUSED = "cluster.refused"`; `ClientKind::Node`), `crates/local-api/src/server.rs` and `conn.rs` (count `Node` clients nowhere: they never connect to the socket; only exhaustive matches change), `crates/notchlingd/src/layout.rs` (`fn cluster_dir(&self) -> PathBuf`), `crates/notchlingd/src/daemon_wiring.rs` (build and close the node; share `Ops` as `Arc<Ops>` with the driver), `crates/notchlingd/src/daemon.rs`, `crates/notchlingd/src/lib.rs`, `crates/notchlingd/Cargo.toml` (dep `notchling-cluster-net`)

**Interfaces:**
- Consumes: `NodeCore` (Task 6), `persist` (Task 8), `Transport`, `Identity` (Task 9), `boot_ms`, `tailscale_addrs`, `LoadSampler` (Task 2).
- Produces:
  - `pub struct ClusterSetup { pub cluster_dir: PathBuf, pub fixture: bool, pub loopback_ok: bool, pub port: u16, pub versions: Versions }`
  - `pub trait LocalCalls: Send + Sync { fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError>; }`; the daemon's one runs the op through `Ops::call` with `Caller { tier: <App for an App origin the policy allowed, else Agent>, client: ClientKind::Node, peer: None, pid: None }` and audits `cluster:<op>` with the origin node in `peer`
  - `pub struct ClusterNode`: `fn start(setup: ClusterSetup, room: Arc<dyn Room>, local: Arc<dyn LocalCalls>, transport: Arc<dyn Transport>, audit: AuditLog) -> Arc<ClusterNode>`, `fn local(&self, cmd: LocalCmd) -> Result<Value, ApiError>` (waits up to `CALL_MS`, never on the UI's thread), `fn status(&self) -> Value`, `fn close(&self)`
  - effects: `Persist` → `persist::save` (a save error is logged and the op fails; the in-memory core is not advanced past a failed save); `Publish` → `room.publish`; `Pins` → `Pins::set`; `RunLocal` → `LocalCalls` on a worker thread, then `Input::LocalDone`; `Wipe` → `persist::wipe` and the mirror folder (Task 12)
  - a tick thread every second feeds `Input::Tick` from `boot_ms()` and the wall clock; under a fixture root nothing listens or dials
  - `cluster.status {}` (`OpSpec::AGENT`): `{"cluster": {"id", "name", "term", "home", "vault"} | null, "me": "<node id>", "tailscale": bool, "members": [{"id", "name", "role", "canBeMaster", "online", "client", "reference", "state", "versions", "load", "desired", "acceptsDelegation"}]}` (`role` from `role_of`: `home`, `standby`, `worker`)
  - topic `cluster` (agent tier): `cluster.changed` with the same body after any change
  - `cluster_versions::probe(roots: &versions::mac::Roots, cache: &mut IdentityCache, agy: Option<versions::Version>) -> Versions` (conflict C2): the tool versions are `versions::mac::detect(roots, tool, cache)` for Claude Code, Codex and Antigravity (version sync Phase 1 Task 3; it reads files only, on any OS, since the layouts are the same under a home); agy's version is the one version sync's daemon already asks for (its Phase 1 Task 5: at most every 10 minutes, and on a node that isn't the Mac not at all until version sync's agy knob is verified, its call V2), passed in, else unknown; this module keeps only the node-level fields: `notchling` (the daemon's own version), `proto`, `features`, `schema`, `os`, `arch`. Needs version sync Phase 1 Tasks 2 and 3; if they haven't merged, build them first from that plan as written.
- [ ] **Step 1: Write the failing tests** (two `ClusterNode`s on one `MemTransport` net, temp data folders, a recording `Room`):
  - `two_nodes_link_and_see_each_other` (after Create and an approved join done through `local`, both statuses list both, online)
  - `status_is_agent_tier`, `status_without_cluster_is_null`
  - `fixture_root_binds_nothing`
  - `failed_save_fails_the_op_and_keeps_state`
  - `probe_uses_detect_and_adds_node_fields` (temp home with a fake native link and package files: the tools equal what `detect` returns; nothing is started, as in version sync's `detect_starts_nothing`), `unknown_agy_version_stays_unknown`
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib cluster_node` (then `cluster_ops`, `cluster_versions`). Expected: FAIL.
- [ ] **Step 3: Implement.** No lock held across a send, a save or a local call (crates/AGENTS.md, "Locks held across blocking work").
- [ ] **Step 4: Run** the three filters and `cargo test -p notchling-local-api`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: the cluster node driver and cluster.status`.

### Task 11: Create, join, approve, remove; headless switches; the command line

**Zone:** maintainer. CAP-NETWORK, CAP-FILEWRITE.

**Files:**
- Create: `crates/notchlingd/src/cluster_join.rs`, `crates/notchlingd/src/cluster_switches.rs`, `crates/notchlingd/src/cluster_cli.rs`
- Test: `crates/notchlingd/src/cluster_join_tests.rs`, `crates/notchlingd/src/cluster_switches_tests.rs`, `crates/notchlingd/src/cluster_cli_tests.rs`
- Modify: `crates/local-api/src/wire.rs` (`CLUSTER_CREATE`, `CLUSTER_JOIN_TOKEN`, `CLUSTER_JOIN`, `CLUSTER_MEMBER_SET`, `CLUSTER_REMOVE`; node-only `NODE_SWITCH_SET = "node.switch.set"`), `crates/notchlingd/src/cluster_ops.rs`, `crates/notchlingd/src/cli.rs` (`notchling cluster status [--json] | token | join --token-stdin`), `crates/engine/src/inbox/model.rs` (`Kind::NodeJoin`), `crates/notchlingd/src/switches.rs` (a `pub(crate)` setter the node driver may call, audited `cluster_set`)

**Interfaces:**
- Consumes: Tasks 5, 6, 10; `InboxOps::raise`, `Responder` (plan A).
- Produces (all additive; shapes go in `docs/local-api-ops.md` in Task 19):
  - `cluster.create {name}` → `{cluster}`: `OpSpec::APP`; this node becomes the home (the primary) and the vault node, term 1, `client: true`, `reference: true`, `can_be_master: true`.
  - `cluster.join.token {}` → `{token}`: `OpSpec::AGENT` (every join still needs Allow); on a non-home member it is forwarded to the home.
  - `cluster.join {token}` → `{state: "pending" | "joined"}`: `OpSpec::AGENT`, refused `"this node is already in a cluster"` when it has one.
  - `cluster.member.set {node, canBeMaster?, client?, reference?, acceptsDelegation?, desired?}`: `OpSpec::APP`, forwarded to the home from a client node. (Plan D's readiness check gates `canBeMaster: true` on disk encryption and the same home path; until D lands the flag is stored and nothing uses it.)
  - `cluster.remove {node}`: `OpSpec::APP`, forwarded to the home.
  - Join on the home: `Tokens::take`, `ProposeJoin`, then an inbox item `node-join:<node id>`, kind `node_join`, title `Add <name> to the cluster`, detail `<os> <arch> · <first address> · <node id>` and, for a Mac, `· Can Be Master`, actions `[Allow "Add" primary, Deny "Deny"]`, expires in 10 minutes; its responder applies `Approve { client: false, can_be_master: can_be_master_default(os) }` or `Deny` and answers `true`; the joiner gets `JoinOk` or `Refused`. The maintainer changes either flag later in the cluster view.
  - Headless switches: when a node with no app attached gets `Members` or `HelloOk`, it sets `acting`, `bridge` and `install` to its `desired` (only those three: `policy::cluster_may_set`); a node with an app attached ignores them (the app owns its switches, 0011's note of 2026-11-11). `node.switch.set {switch, on}` from the home does the same at once. Every change is audited `cluster_set:<switch>:<on|off>`.
  - Removal: the home applies `Remove`, pushes `Members`; every node drops the pin; the removed node's next hello gets `Refused { "removed" }` and it wipes (Task 6).
  - `notchling cluster join --token-stdin` reads the token from stdin only; a token in argv is refused `"pass the token on stdin"`.

- [ ] **Step 1: Write the failing tests:**
  - `join_with_token_raises_inbox_item_and_allow_adds_worker` (a Linux joiner: Worker, Can Be Master off), `allowed_mac_joins_with_can_be_master_on`, `deny_refuses_the_joiner`, `token_is_one_time`, `join_refused_when_already_in_a_cluster`
  - `join_token_from_client_node_is_minted_by_home`
  - `removed_node_is_dropped_and_wipes_on_contact`
  - `home_desired_turns_acting_on_headless_node`, `desired_never_touches_vault_or_accounts`, `node_with_app_ignores_home_desired`, `switch_changes_are_audited`
  - `cli_join_reads_token_from_stdin_only`
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib cluster_join` (then `cluster_switches`, `cluster_cli`). Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the filters and `cargo test -p notchling-engine --lib inbox::`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: create, join with approval, remove, headless switches`.

### Task 12: Forwarding, remote sessions and the transcript mirror

**Zone:** maintainer. CAP-FILEWRITE (mirror files).

**Files:**
- Create: `crates/notchlingd/src/cluster_forward.rs`, `crates/notchlingd/src/cluster_sessions.rs`, `crates/notchlingd/src/cluster_mirror.rs`
- Test: `crates/notchlingd/src/cluster_forward_tests.rs`, `crates/notchlingd/src/cluster_sessions_tests.rs`, `crates/notchlingd/src/cluster_mirror_tests.rs`
- Modify: `crates/notchlingd/src/daemon_wiring.rs` (`Ops::call` asks `cluster_forward::route` first), `crates/notchlingd/src/daemon.rs` (`sessions.list` and `sessions.changed` carry remote rows), the daemon's `Room` (`can_answer` also true while a cluster client follows this node's `prompts`), `crates/engine/src/sessions/agent.rs` (`pub node: Option<NodeRef>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`; `NodeRef { id, name, online }`), `crates/local-api/src/wire.rs` (`CLUSTER_FOLLOW = "cluster.follow"`, node-only `NODE_TAIL = "node.tail"`, `NODE_SESSIONS = "node.sessions"`)

**Interfaces:**
- Consumes: `policy::RULES` `id_field`s (Task 4), `ClusterNode::local(Forward)` (Task 10).
- Produces:
  - `pub fn route(op: &str, args: &Value, me: &NodeId) -> Option<(NodeId, Value)>`: for an op with a rule whose `id_field` holds a qualified id of another node, the owner and the args with the bare id; else `None` (bare ids and this node's own stay local).
  - Remote rows: each linked member's `node.sessions` list (asked on link and on its `sessions.changed` event), each row with `id` qualified, `host` = node id, `node` set, `transcriptPath` = the mirror file when one exists, else null. A node offline keeps its last rows with `node.online = false`.
  - `node.tail {id, from}` (owner side, client-only rule) → `{data: <base64>, next: u64, size: u64, reset: bool}`: `id` must be a session in this node's list with a transcript path (the path never comes from the caller), at most 512 KB per call; `reset` when the file is shorter than `from` or replaced.
  - `cluster.follow {id}` (`OpSpec::APP`): starts or renews a 10-minute follow; the daemon pulls `node.tail` on the owner's `sessions.changed` and every 2 s while followed, appending to `<data>/cluster/mirror/<node id>/<file name>` (0600, folder 0700); a `reset` rewrites the file.

- [ ] **Step 1: Write the failing tests:**
  - `qualified_reply_is_forwarded_to_owner`, `bare_id_stays_local`, `own_qualified_id_stays_local`
  - `forward_from_worker_is_refused` (a Worker sends `session.reply` to a client: refused, a `cluster.refused` event, an audit line)
  - `remote_rows_are_qualified_with_node_field`, `offline_node_rows_say_offline`
  - `tail_serves_only_known_transcripts`, `tail_is_capped_at_512k`
  - `mirror_appends_and_resets_on_truncation`, `follow_expires_after_ten_minutes`
  - `headless_node_waits_for_cluster_follower_on_permission` (`hook.request` on a node with no app waits when a client follows its `prompts`, refuses at once when none does)
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib cluster_forward` (then `cluster_sessions`, `cluster_mirror`). Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the filters and `cargo test -p notchling-engine --lib sessions::`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: forward ops by qualified id, remote sessions, transcript mirror`.

### Task 13: Dispatch sessions and shell jobs; `delegate --machine`; the e2e test

**Zone:** maintainer. CAP-PROCESS (shell jobs), CAP-FILEWRITE (job logs).

**Files:**
- Create: `crates/engine/src/cluster/shell_job.rs`, `crates/notchlingd/src/cluster_dispatch.rs`, `crates/notchlingd/src/shell_jobs.rs`, `crates/notchlingd/tests/cluster_e2e.rs`
- Test: `crates/engine/src/cluster/shell_job_tests.rs`, `crates/notchlingd/src/cluster_dispatch_tests.rs`, `crates/notchlingd/src/shell_jobs_tests.rs`
- Modify: `crates/engine/src/delegate/options.rs` (`--machine <node>`), `crates/notchlingd/src/delegate_cli.rs`, `crates/local-api/src/wire.rs` (`CLUSTER_DISPATCH = "cluster.dispatch"`, `SHELL_RUN = "shell.run"`, `SHELL_STOP = "shell.stop"`, node-only `DELEGATE_RUN = "delegate.run"`), `crates/notchlingd/src/layout.rs` (`--cluster-port` for tests, default 47823)

**Interfaces:**
- Consumes: `LocalCmd::Dispatch` (Task 6), `compat` (Task 3).
- Produces:
  - `cluster.dispatch {node, tool, cwd, model?, text?, command?, secrets?, key}` → `{job, session?, warning?}`: `OpSpec::APP`; `warning` carries `compat`'s Warn text (a tool or Notchling version mismatch). No `runAnyway` here: nothing it would override refuses until version sync Phase 6 Task 5, which adds it with the refusal (conflict C1). The agent-tier form (from `notchling delegate --machine`) carries `{node, delegate: [args], key}`, only to a member with `acceptsDelegation`; its warning is printed to stderr before the job's output (version sync Phase 1 Task 10's half that remains).
  - On the Worker: `shell.run {job, cwd, command}` (from the home) runs `/bin/sh -c <command>` in `cwd` in its own process group, environment scrubbed of `NOTCHLING_*` and `CLAUDE_CODE_*`, output to `<data>/cluster/jobs/<job>.log` (0600), and its exit as a `JobState` event; `shell.stop {job}` sends SIGTERM to the group, SIGKILL after 2 s. The row it adds to `sessions.list` has `id` `shell:<job>`, `tool` `shell`, `transcriptPath` the log.
  - `delegate.run {args}` on the Worker runs the engine's delegate there and returns its outcome JSON.
  - engine: `pub fn shell_plan(job: &str, cwd: &str, command: &str, data_dir: &Path) -> Result<ShellPlan, String>` (`cwd` absolute and existing; `ShellPlan { argv, env_remove: Vec<String>, log: PathBuf }`).
  - `notchling delegate --machine <node> …`: unknown node → exit 2 `"No node named <node>"`; offline → `"<node> is offline"`; otherwise one `cluster.dispatch` and the Worker's outcome printed as the local delegate prints it.

- [ ] **Step 1: Write the failing tests:**
  - `dispatch_on_home_starts_session_on_worker` (a fake `LocalCalls` on the Worker records `session.new {folder}`)
  - `dispatch_from_client_goes_through_home`, `dispatch_to_offline_node_is_an_error`, `dispatch_refused_by_versions_says_why` (a missing tool), `tool_mismatch_dispatches_with_a_warning` (the reply's `warning` names both versions), `retry_with_same_key_returns_same_job`, `agent_dispatch_needs_accepts_delegation`, `dispatch_from_stepped_down_home_is_refused` (after a newer term, the old home's `cluster.dispatch` answers `"<name> is primary now"`)
  - `shell_plan_scrubs_env_and_needs_absolute_cwd`
  - `shell_job_runs_and_logs` (`#[cfg(unix)]`, `printf hi` in a temp folder: the log holds `hi`, the job ends Done), `shell_stop_kills_the_group`
  - `delegate_machine_unknown_is_error`, `delegate_machine_prints_the_warning_and_runs`
  - e2e `two_daemons_join_and_run_a_shell_job`: two `notchling daemon` processes over copied fixture roots, `NOTCHLING_CLUSTER_LOOPBACK=1`, free ports, a test-built app-tier path list (`NOTCHLING_APP_TIER_PATHS` to the test client), create, token, join, Allow through `inbox.answer`, dispatch `printf ok`; the log ends with `ok`; both daemons stopped by a guard.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib cluster_dispatch` (then `shell_jobs`; `cargo test -p notchling-engine --lib cluster::shell_job`; `cargo test -p notchling-notchlingd --test cluster_e2e`). Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** all four. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: dispatch to a picked node, shell jobs, delegate --machine`.

### Task 14: Code for a Worker session, by git

**Zone:** maintainer. CAP-PROCESS (git), CAP-FILEWRITE.

**Files:**
- Create: `crates/engine/src/cluster/blob.rs`, `crates/engine/src/cluster/workspace.rs`, `crates/notchlingd/src/cluster_blobs.rs`, `crates/notchlingd/src/cluster_workspace.rs`
- Test: `crates/engine/src/cluster/blob_tests.rs`, `crates/engine/src/cluster/workspace_tests.rs`, `crates/notchlingd/src/cluster_workspace_tests.rs` (`#[cfg(unix)]`, real git in temp folders)
- Modify: `crates/local-api/src/wire.rs` (`CLUSTER_RETURN = "cluster.return"`; node-only `NODE_PUT`, `NODE_PUT_END`, `WORKSPACE_PREPARE`, `WORKSPACE_RETURN`)

**Interfaces:**
- Produces:
  - blob: `pub const CHUNK: usize = 256 * 1024`, `pub const MAX_BLOB: u64 = 256 * 1024 * 1024`; `pub fn chunks(len: u64) -> Vec<(u64, usize)>`; `pub struct Incoming` (`fn put(&mut self, offset: u64, data: &[u8]) -> Result<(), String>` in order only, `fn finish(self, sha256: &str) -> Result<(), String>`). Plan D reuses it for snapshots.
  - `node.put {transfer, offset, data}` / `node.put.end {transfer, sha256}` write `<data>/cluster/incoming/<transfer>.part` (0600) and rename to `<transfer>` when the hash matches; parts older than an hour are deleted.
  - `pub struct RepoFacts { pub root: String, pub origin: Option<String>, pub branch: String, pub head: String, pub upstream: Option<String> }`
  - `pub fn plan_send(facts: &RepoFacts) -> SendPlan` (sender side: `git bundle create <file> <upstream>..HEAD` when HEAD is ahead of its upstream, nothing otherwise; full refnames, `--no-tags`)
  - `pub fn plan_prepare(facts: &RepoFacts, job: &str, work_root: &str, bundle: Option<&str>) -> Result<Vec<GitCall>, String>` (Worker side: clone from `origin` into `<work_root>/<repo name>-<first 8 of sha256(origin)>` if missing; `fetch origin +refs/heads/<branch>:refs/remotes/origin/<branch>`; `fetch <bundle> +HEAD:refs/notchling/in/<job>` when given; `worktree add -B notchling/<job> <work_root>/jobs/<job> <head>`); refuses a repo with no origin `"This repository has no remote; send its folder to <node> another way"`
  - `pub fn plan_return(job: &str) -> GitCall` (Worker bundles `refs/heads/notchling/<job>` against the job's start commit) and `pub fn plan_fetch_back(bundle: &str, node_name: &str, job: &str) -> Vec<GitCall>` (sender: fetch into `refs/heads/notchling/<node>/<job>`, never force, never merge; an existing branch that isn't an ancestor stops with `"notchling/<node>/<job> already has other commits"`)
  - every `GitCall` runs with `GIT_TERMINAL_PROMPT=0`, `-c core.hooksPath=/dev/null`, `-c core.fsmonitor=false`, every inherited `GIT_*` removed (as `engine::git::read` does)
  - `cluster.dispatch` gains `code: "send" | "folder"`; `send` runs the plan before the tool starts and uses the worktree as `cwd`; `cluster.return {job}` (`OpSpec::APP`) brings the branch back

- [ ] **Step 1: Write the failing tests:**
  - `blob_must_arrive_in_order`, `blob_hash_mismatch_is_rejected`, `blob_over_cap_is_refused`
  - `send_plan_bundles_only_unpushed_commits`, `prepare_clones_once_and_adds_a_worktree`, `repo_without_origin_is_refused`
  - real git: `worker_gets_unpushed_commit`, `hooks_never_run` (a trap `post-checkout` hook writes a file; it must not exist), `return_creates_branch_and_never_merges`, `fetch_back_refuses_a_diverged_branch`
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::blob` (then `cluster::workspace`, `cargo test -p notchling-notchlingd --lib cluster_workspace`). Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** all three. Expected: PASS.
- [ ] **Step 5: Commit** `cluster: send a repository to a Worker by git and bring work back as a branch`.

### Task 15: Machines: add a machine as a node

**Zone:** maintainer for `crates/engine/src/machines/`; auto for `crates/app/src/machines/`. No new CAP (the existing `Runner`).

**Files:**
- Create: `crates/engine/src/machines/steps_node.rs`
- Test: `crates/engine/src/machines/tests_node_steps.rs`, `crates/app/src/machines/model_tests_node.rs`
- Modify: `crates/engine/src/machines/step.rs` (`StepId::NODE_TAILSCALE`, `NODE_BINARY`, `NODE_SERVICE`, `NODE_JOINED`), `crates/engine/src/machines/catalog.rs` (node steps after `REACHABLE` when `m.node.is_some()`), `crates/engine/src/machines/registry.rs` (`pub node: Option<NodeSettings>` with serde default; `NodeSettings {}` for now: Can Be Master is not stored here but in the cluster, set at approval by OS and changed in the cluster view, Task 16), `crates/engine/src/machines/mod.rs`, `crates/engine/src/machines/step.rs` (`StepContext` gains `tokens: Arc<dyn ClusterTokens>`; `trait ClusterTokens { fn join_token(&self) -> Result<String, String>; fn cluster_id(&self) -> Option<String>; fn can_be_master(&self, machine: &str) -> Option<bool>; }`, with a refusing default), `crates/app/src/machines/model.rs` and its view (a "Cluster Node" toggle on a machine, and a read-only "Can Be Master" line from `cluster.status`; plan D Task 12 adds its readiness steps there)

**Interfaces:**
- Produces the four steps:
  - `NODE_TAILSCALE` probe `tailscale status --json`: Pass when `BackendState` is `Running` and `Self.TailscaleIPs` has a 100.64/10 address; else Fail with HowTo `"Install Tailscale on <name> and run sudo tailscale up there, in your own terminal."`
  - `NODE_BINARY` probe `~/.local/bin/notchling --version`: Pass when it equals the Mac's build; Fix copies `<data>/cluster/dist/notchling-<os>-<arch>` with `scp -O` to `~/.local/bin/notchling.new`, checks its SHA-256 on the machine (`shasum -a 256` or `sha256sum`), then `mv` and `chmod 755`; a missing dist file fails `"Put a notchling build for <os>-<arch> in <path> first."`
  - `NODE_SERVICE` probe `systemctl --user is-active dev.notchling.daemon` or `launchctl print gui/<uid>/dev.notchling.daemon`; Fix runs `~/.local/bin/notchling service install` then its printed load command; on Linux `loginctl show-user <user> -p Linger` must say yes, and enabling linger is an admin Fix (`needs_admin`)
  - `NODE_JOINED` probe `~/.local/bin/notchling cluster status --json`: Pass when its cluster id equals `tokens.cluster_id()`; Fix gets `tokens.join_token()` and runs `~/.local/bin/notchling cluster join --token-stdin` with the token as `Call::stdin` only, the token added to the step's masks

- [ ] **Step 1: Write the failing tests** (`ScriptedRunner`):
  - `tailscale_running_passes`, `tailscale_stopped_fails_with_howto`
  - `binary_fix_copies_and_verifies_hash`, `binary_fix_without_dist_file_says_where`
  - `service_fix_installs_and_loads_linux`, `service_fix_installs_and_loads_macos`, `linger_off_needs_admin_fix`
  - `join_fix_sends_token_on_stdin_only` (no recorded argv holds the token; output masked)
  - `node_steps_absent_without_node_settings`
  - app: `node_toggle_sets_settings`, `can_be_master_line_reads_cluster_status`
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib machines::tests_node_steps` and `cargo test -p notchling-app --lib machines::model_tests_node`. Expected: FAIL.
- [ ] **Step 3: Implement.** The `ClusterTokens` the Machines window gets asks the local daemon `cluster.join.token` and `cluster.status` (agent tier is enough: Task 11).
- [ ] **Step 4: Run** both and `cargo test -p notchling-engine --lib machines::`. Expected: PASS.
- [ ] **Step 5: Commit** `machines: add a machine as a cluster node`.

### Checkpoint 2: the daemon side

- [ ] `cargo test --workspace --locked`: PASS (known flaky tests from earlier reports excepted, named in the ledger).
- [ ] `cargo test -p notchling-notchlingd --test cluster_e2e`: PASS.
- [ ] fmt, clippy, guard lint and featuremap: clean.
- [ ] Controller checks the audit lines of a refused forward and a headless switch change in the e2e's audit file.

### Task 16: The cluster view and node labels

**Zone:** auto (`crates/app`, `crates/kit`), except the feature-map row (protected). No CAP.

**Files:**
- Create: `crates/app/src/cluster/mod.rs`, `model.rs`, `wire.rs`, `feed.rs`, `view.rs`, `renders.rs`
- Test: `crates/app/src/cluster/model_tests.rs`, `crates/app/src/chat/sidebar_node_tests.rs`
- Modify: `crates/app/src/chat/view/sidebar.rs` (a footer button `"cluster"`, `Icon::Server`, "Cluster", "Machines in the cluster"), `crates/app/src/chat/sidebar.rs` (`SidebarModel` rows: node name and offline), `crates/app/src/lib.rs`, `crates/app/src/mac/views.rs`, `crates/kit/src/icon.rs` (Lucide `server`, if missing), `docs/feature-map.md`

**Interfaces:**
- Consumes: `cluster.status`, topic `cluster`, `cluster.create`, `cluster.member.set`, `cluster.remove`, `cluster.join.token` (Tasks 10, 11).
- Produces:
  - `ClusterModel::from_status(status: &Value, me: &str) -> ClusterModel`, `fn rows(&self) -> Vec<NodeRow>`, `fn can_create(&self) -> bool`
  - `NodeRow { id, name, role: &'static str ("Primary", "Standby", "Worker"), can_be_master: bool, online: bool, os: String, cpu: Option<u8>, mem: Option<(u64, u64)>, tools: Vec<(String, Option<String>)>, warnings: Vec<String>, desired: (bool, bool, bool), me: bool }`; `warnings` come from `compat` against the reference node
  - the view: a sheet from the footer button, one row per node (name, role, online dot, OS, CPU and memory, tool versions, warnings), row menu Can Be Master (a check), Acting On/Off, Remove (confirm); Create Cluster when there is none; Copy Join Token. Plans D and E add Move to <name> Now and Take Back to this menu.
  - Sidebar: a remote row's meta line shows the node's name; an offline node's rows are dimmed and its reply box says `"<node> is offline"`
  - parts `"Cluster Button"`, `"Cluster View"`, `"Node Row"`

- [ ] **Step 1: Write the failing tests:**
  - `status_maps_roles_and_online` (labels Primary, Standby, Worker; never "Home"), `can_be_master_check_sends_member_set`, `compat_warning_shows_on_row`, `no_cluster_offers_create`, `this_mac_is_marked`
  - `remote_row_label_is_node_name`, `offline_remote_row_disables_reply`
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib cluster::` and `chat::sidebar_node_tests`. Expected: FAIL.
- [ ] **Step 3: Implement** the model, the feed (calls off the UI thread, like `inbox/feed.rs`), the view with kit tokens and menus (`menu_panel`), and a `ViewSpec` `cluster` in `renders.rs` over a fixture status.
- [ ] **Step 4: Run** the tests, then `NOTCHLING_RENDER_FORCE_FRAMES=1 notchling-app --render cluster --out /tmp/cluster.png` (release build) and look at it.
- [ ] **Step 5: Commit** `app: cluster view and node labels in the Sidebar`.

### Task 17: Run on, remote sessions in Chat, shell job rows

**Zone:** auto. No CAP.

**Files:**
- Create: `crates/app/src/chat/view/tool/shell_job.rs` (a `ChatTool` adapter, decision 0015), `crates/app/src/chat/remote.rs`
- Test: `crates/app/src/sheets/new_session_run_on_tests.rs`, `crates/app/src/chat/remote_tests.rs`
- Modify: `crates/app/src/sheets/new_session.rs` and `crates/app/src/sheets/view.rs` (a Run on column; Code: Send This Repository or Use a Folder There; a Command field for a shell job), `crates/app/src/chat/loader.rs` (a remote row's transcript is its `transcriptPath`), `crates/app/src/chat/view/load.rs` (sends `cluster.follow` every 60 s while a remote row is selected), `crates/app/src/chat/view/tool/mod.rs` (register the adapter)

**Interfaces:**
- Consumes: `cluster.status`, `cluster.dispatch`, `cluster.follow`, qualified ids (Tasks 10 to 14).
- Produces:
  - `RunOn { nodes: Vec<RunOnRow> }` with `RunOnRow { id, name, online, reason: Option<String>, warning: Option<String> }`; online nodes first, this Mac first of all; a refused node shows its reason and can't be picked; a node with a version mismatch can be picked and shows the warning under its name (no Run Anyway until version sync Phase 6)
  - `NewSession::start` sends `cluster.dispatch` with a fresh key (`ns-<uuid>`) when the node isn't this Mac, else today's request
  - `chat::remote::follow_due(selected: Option<&Session>, last_ms: i64, now_ms: i64) -> bool`
  - the shell job adapter: the log as one growing code block, a Stop button (`shell.stop`), no reply box

- [ ] **Step 1: Write the failing tests:**
  - `run_on_lists_online_nodes_first_with_reasons`, `version_mismatch_is_pickable_with_warning`, `start_on_worker_sends_dispatch_with_key`, `send_repository_sets_code_send`
  - `remote_row_uses_mirror_transcript`, `follow_is_due_every_minute_while_selected`, `shell_job_row_shows_log_tail`
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib sheets::new_session_run_on_tests` and `chat::remote_tests`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run**, then render New Session with `--render new-session` over a fixture with two nodes and look at it.
- [ ] **Step 5: Commit** `app: Run on, remote sessions in Chat, shell job rows`.

### Task 18: The phone: node labels and remote sessions through the home

**Zone:** maintainer for `protocol/`, `crates/engine/src/bridge/`, `crates/notchlingd/`; auto for `apps/flutter/lib/features/` (not pairing) and new Flutter tests. No CAP.

**Files:** follow `.claude/skills/add-protocol-field/SKILL.md`:
- Modify: `protocol/agentconsole.v1.schema.json` (`Session.node`: optional `{id, name, online}`, described "Additive: the node a session runs on in a cluster; an older phone ignores it"), new fixtures under `protocol/fixtures/`, regenerate (`python3 tools/protocol/gen.py`)
- Modify: `crates/engine/src/bridge/live.rs` and `crates/notchlingd/src/bridge_backend.rs` (the phone's list is the daemon's merged list; `send_message`, `respond` and `chat_messages` on a qualified id go through the same forwarding and mirror as the app)
- Modify: the Flutter session row in `apps/flutter/lib/features/sessions/` (a node label when `node` is set and not this computer)
- Test: `crates/engine/src/bridge/router_cluster_tests.rs` (new file), `apps/flutter/test/node_label_test.dart`

- [ ] **Step 1: Write the failing tests:** `phone_lists_remote_sessions_with_node`, `phone_reply_to_remote_session_is_forwarded`, `old_phone_fixture_without_node_still_parses`; Flutter `shows_node_label_for_remote_session`, `no_label_without_node`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib bridge::router_cluster_tests`, `python3 tools/protocol/gen.py --check`, `~/flutter/bin/flutter test test/node_label_test.dart` (in `apps/flutter`). Expected: FAIL.
- [ ] **Step 3: Implement** schema first, regenerate, then the backend and the Flutter row.
- [ ] **Step 4: Run** the three and `~/flutter/bin/dart analyze`. Expected: PASS.
- [ ] **Step 5: Commit** `phone: sessions on other nodes, with their node (additive)`.

### Task 19: Docs and checks

**Zone:** maintainer (AGENTS files, `ARCHITECTURE.md`, `docs/policies/`, `docs/feature-map.md`). No CAP.

**Files:**
- Modify: `crates/AGENTS.md` (a "Cluster" bullet in Architecture in one pass: the core, the driver, the node tier, the mirror; "Things that bit before" stays as is), `crates/engine/AGENTS.md`, `crates/notchlingd/AGENTS.md`, `crates/app/AGENTS.md` (the why of each new file in their format), `ARCHITECTURE.md` (the Remote computers row points at the nodes decision, by its number once merged), `docs/local-api-ops.md` (every new op and topic with its shape), `docs/policies/privacy.md` (what crosses machines: session lists, transcript tails, commands and folders of dispatched work, git bundles; only over Tailscale and pinned TLS; nothing to a third party), `docs/feature-map.md` (a Cluster row), `CHANGELOG.md` (`[Unreleased]` → Added)

- [ ] **Step 1: Write the docs.**
- [ ] **Step 2: Run** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, `cargo deny check`. Expected: clean, PASS.
- [ ] **Step 3: Run** `PYTHONPATH=tools/guard python3 -m guard lint`, `featuremap`, `rules`, `claudemd`, and `python3 tools/strip_comments.py --check`. Expected: 0 new.
- [ ] **Step 4:** No install (plan A ruling R31): the final report gives the maintainer the live check below.
- [ ] **Step 5: Commit** `docs: cluster nodes`.

### Checkpoint 3: the live check (maintainer only)

Gated by the overview's M1 (Tailscale), M2 (Linux binary), M3 (the node user). The maintainer installs the build, creates the cluster on m5 (the laptop, primary), allows m1's join (it shows as a Standby with Can Be Master on; nothing is sent to it before plan D), adds a VPS as a node in Machines, allows the join (a Worker), then: a shell job `uname -a` on it, a Codex session on it, both seen in Chat and on the phone. Claude Code on a Linux node waits for M4.
