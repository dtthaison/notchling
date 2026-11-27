# Provider Contract: Implementation Plans, Overview

> **For agentic workers:** this file orders the seven phase plans and holds the calls they refer to. Each phase plan is executed with superpowers:subagent-driven-development (recommended) or superpowers:executing-plans.

**Goal:** Build the provider capability contract the maintainer approved on 2026-11-14: 39 capabilities declared for Claude Code, Codex and Antigravity, routed through one registry, with two shrink-only files that make every missing capability visible, subagents of every tool in one shape, one finished signal, delegation from and to every tool, and the held queue for every tool.

**Spec:** `docs/superpowers/specs/2026-11-13-provider-contract-design.md` (commit b34fabf), with its "Calls settled after review" and section 14 "Decided by the user". **Code base:** `origin/merge/settings-batch` (cc28da7), being merged into `agent-run-oss`. Every phase branches from the integration branch once the phase before it has merged; nothing here touches `main` or merges anything.

## The plans

| Phase | File | Branch | Tasks |
| --- | --- | --- | --- |
| 0. Contract and matrix | `2026-11-14-providers-p0-contract.md` | `agent/providers-p0-contract` | 8 |
| 1. Subagents in one shape | `2026-11-14-providers-p1-subagents.md` | `agent/providers-p1-subagents` (1.11 and 1.12 may go as `agent/providers-p1-antigravity-name`) | 13 |
| 2. Signals | `2026-11-14-providers-p2-signals.md` | `agent/providers-p2-signals` (2.9 may go as `agent/providers-p2-phone`) | 12 |
| 3. Delegate everywhere | `2026-11-14-providers-p3-delegate.md` | `agent/providers-p3-delegate` | 8 |
| 4. Held queue for every tool | `2026-11-14-providers-p4-held-queue.md` | `agent/providers-p4-held-queue` | 6 |
| 5. The other ports | `2026-11-14-providers-p5-ports.md` | `agent/providers-p5a-sessions` … `-p5i-setup`, one per item | 9 items |
| 6. Clean-up | `2026-11-14-providers-p6-cleanup.md` | `agent/providers-p6-cleanup` | 4 |

60 tasks in all. Decision numbers are placeholders, assigned at merge (call C5).

## Order and lanes

```
P0 ──► P1 ──┬──► P2 ──┬──► P4 ──► P5b ──► P5c
            │         └──► P3 (its skill line about finish signals waits for P2)
            └──► P3 (record and caller work can start after P1)
P0 ──► P5e, P5f, P5h (any time after P0; one pull request each)
P1 ──► P5a ──► P5d, P5g          P5i after decision 0019
P5 (5a, 5b at least) ──► P6
```

- **Strictly in order:** P0 first (every later task edits its files). P1 before P2 (signals read the registry's subagents). P2 before P4 (the held queue releases on `turn.finished`). P5a and P5b before P6 (they move every caller of the alias ops).
- **In parallel:** P3 beside P2 once P1 has merged; P5e, P5f and P5h beside anything after P0; inside a phase, the lanes each plan names (for example P1's Codex readers, child index, Antigravity port and Search run as four agents with their own `CARGO_TARGET_DIR`).
- Each phase is one pull request into the integration branch with the app working after it, except P5 (one per item) and the optional splits named above.

## The matrix at phase 0, and where each cell is wired

N Native, F Fallback (flow), NA NotApplicable, GAP (plan). After the slash, the phase whose plan wires the cell or closes the gap. `design` gaps leave `gaps.json` in the pull requests that land decision 0019 or the version-sync plans, never into `unwired.json` (call C1).

| Capability | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| `session.list` | N / P2 | N / P2 (child bug fixed P1) | N / P2 |
| `session.status` | N / P5a | N / P5a | N / P5a |
| `session.new` | N / P5a | N / P5a | N / P5a |
| `session.stop` | N / P5a | NA | N / P5a |
| `session.remove` | N / P5a | N / P5a | GAP / P5d (F hide-row) |
| `session.move` | N / P5a | GAP / P5g, if wanted | NA |
| `folder.trust` | N / P5a | NA | N / P5a |
| `conversation.read` | N / P5a | N / P5a | N / P5a |
| `conversation.follow` | N / P5a | N / P5a | N / P5a |
| `conversation.context` | N / P5a | N / P5a | N / P5a |
| `subagent.list` | N / P1 | GAP / P1 | GAP / P1 |
| `subagent.status` | N / P1 | GAP / P1 | GAP / P1 |
| `subagent.glance` | N / P1 | GAP, meanwhile delegate-jobs / P1 | GAP / P1 |
| `subagent.open` | N / P1 | GAP / P1 | GAP / P1 |
| `subagent.stop` | NA | NA | NA |
| `signal.turn_finished` | N / P2 | N / P2 | N / P2 |
| `signal.subagent_finished` | N / P2 | GAP / P2 | GAP / P2 |
| `signal.needs_you` | N / P5b | NA | N / P5b |
| `turn.send` | N / P5b | N / P5b | N / P5b |
| `turn.hold` | F held-queue / P4 | F held-queue / P4 | GAP / P4 (F held-queue) |
| `turn.take_back` | F held-queue / P4 | GAP / P4 (F held-queue) | GAP / P4 (F held-queue) |
| `turn.interrupt` | N / P5b | N / P5b | N / P5b |
| `turn.options` | N / P5b | N / P5b | N / P5b |
| `prompt.question` | N / P5b | NA | NA |
| `prompt.permission` | N / P5b | NA | N / P5b |
| `command.slash` | N / P5b | GAP / P5c | GAP / P5c |
| `account.switch` | N / P5h | N / P5h | N / P5h |
| `account.lanes` | N / P5h | GAP / P5g, if wanted | NA |
| `usage.limits` | N / P5h | N / P5h | N / P5h |
| `usage.cost` | N / P5h | N / P5h (fork counted once, P1) | N / P5h |
| `catalog.models` | N / P5f | N / P5f | GAP / P5f |
| `delegate.target` | GAP / P3 | N / P3 | N / P3 |
| `delegate.caller` | N / P3 | GAP / P3 | GAP / P3 (F process-tree) |
| `setup.rules` | GAP design: 0019 | GAP design: 0019 | GAP design: 0019 |
| `setup.skills` | N / P5i | N / P5i | N / P5i |
| `setup.hooks` | N / P5i | GAP design: 0019 (planned NA) | GAP design: 0019 (planned NA) |
| `version.detect` | GAP design: version-sync | GAP design: version-sync | GAP design: version-sync |
| `version.update` | GAP design: version-sync | GAP design: version-sync | GAP design: version-sync |
| `search.text` | N / P5e | N / P5e | GAP, meanwhile title-index / P5e |

Counts at phase 0: Claude Code 32 N, 2 F, 1 NA, 4 GAP; Codex 18 N, 1 F, 6 NA, 14 GAP; Antigravity 19 N, 0 F, 4 NA, 16 GAP. The exact `how`, `why` and `plan` lines are in P0's Task 0.2.

### What the two files hold after each phase

| After | `gaps.json` | `unwired.json` | What moved |
| --- | --- | --- | --- |
| P0 | 34 | 72 | the baselines (call C1) |
| P1 | 26 | 68 | Codex and Antigravity `subagent.*` become Native and wired (−8 gaps); Claude Code's `subagent.*` wired (−4) |
| P2 | 24 | 61 | `signal.subagent_finished` for Codex and Antigravity (−2 gaps); `session.list` ×3, `signal.turn_finished` ×3, Claude Code `signal.subagent_finished` wired (−7) |
| P3 | 21 | 58 | Claude Code `delegate.target`, Codex and Antigravity `delegate.caller` (−3 gaps); Codex and Antigravity `delegate.target`, Claude Code `delegate.caller` wired (−3) |
| P4 | 18 | 55 | Codex `turn.take_back`, Antigravity `turn.hold` and `turn.take_back` (−3 gaps); Claude Code `turn.hold`, `turn.take_back`, Codex `turn.hold` wired (−3) |
| P5 | 13, or 11 with 5g | 0 | 5a −22, 5b −15, 5c −2 gaps, 5d −1 gap, 5e −1 gap −2, 5f −1 gap −2, 5g −2 gaps, 5h −10, 5i −4 |
| P6 | unchanged | 0 | none |
| 0019 and version sync land | 0 | 0 | the 11 design gaps, each wired or NotApplicable in the pull request that closes it |

## Calls

### The coordinator's (given with the task)

- **C1. Built setup is Native.** `setup.skills` for all three tools and Claude Code's `setup.hooks` are built today (`install::skills`, `install::hooks`), so they are Native and listed in `unwired.json`, not Gap. Recount: **34 gaps** (Claude Code 4, Codex 14, Antigravity 16), 11 of them `design: 0019` or `design: version-sync` and 23 that name a phase; `unwired.json` starts at **72** (every Native and Fallback cell). "`gaps.json` may only shrink" (the user's decision 14.2) applies from **34** when Phase 0 merges; `unwired.json` the same from 72. The spec's 38 counted the four built cells.
- **C2. `codex.children`'s fallback reads two day folders.** With no daemon (a fixture root, a `--render`) the app reads the parent's start day folder and the next one, at most two (P1 Tasks 1.4, 1.6).
- **C3. The `agy` to `antigravity` key rewrite is maintainer-reviewed.** P1 Task 1.12, allowed as its own pull request.
- **C4. `claude -p` flags are read from `claude --help` first.** P3 Task 3.1, Step 1; the plan names candidates and builds on what the help says.

### Mine

- **C5. Decision number.** `docs/decisions/0028-provider-capability-contract.md` is a placeholder. Sibling plans claim 0019 (agents setup), 0021 (logging; also cluster B and search P4b), 0022 (localization; also cluster C), 0023 to 0025 (cluster D to F), 0026 and 0027 (version sync); 0020 is merged. Two claims collide already; the maintainer assigns numbers at merge.
- **C6. The `Provider` trait grows a port per phase.** Phase 0 has only `tool`, `support`, `wired`; each phase adds its port trait and an accessor whose default is a shared `Unported` value answering `Unsupported`. No empty port traits sit in the code before they are used.
- **C7. Wired per cell; checks in `src`.** Each provider has `const WIRED: &[Capability]`. A registry test holds `unwired.json` equal to the declared Native and Fallback cells minus `WIRED`, so the pull request that routes a cell must cut it from the file. Conformance checks live in `crates/engine/src/contract/conformance_<area>.rs`; `crates/engine/tests/conformance/` is written once in P0 and never edited, so adding checks never trips `WEAK-TEST-EDIT`.
- **C8. `GUARD-PROVIDER-GAPS` lands in Phase 0** (the spec put it in Phase 6): the files must not grow from the day they exist. It fires once on Phase 0's own pull request, which is the maintainer accepting 34 and 72.
- **C9. File format.** `{"entries": ["codex subagent.list", …]}`, sorted, the lint baseline's shape, so the gate reuses `_entries`.
- **C10. No test pins a count.** Counts live in the generated `docs/provider-matrix.md` (auto zone) and in the two files; later phases change no test when a gap closes.
- **C11. Antigravity `catalog.models` is a Gap** until its context windows are in `catalog.json` (P5f); the spec's cell said N with a GAP inside.
- **C12. Codex `subagent.glance` is a Gap** with `delegate-jobs` meanwhile (delegated rows glance today); it becomes Native with the other `subagent.*` in P1.
- **C13. `Subagent` changes in place** to the spec's 8.1 shape; `id` becomes the row id and `own_id()` gives the tool's id for the finished map. Literal edits are listed; one trips `WEAK-TEST-EDIT` (`crates/engine/tests/transcript_subagents.rs`).
- **C14. The app's `chat::delegated` functions stay as wrappers** over the engine's `contract::flows::delegate_jobs` until P6, so `delegated_tests.rs` keeps passing unedited until then.
- **C15. The notch's finish peek** is a new `NotchModel::peek_finished`, asked only after `update` returns none, so the existing order (question, needs you, turn finished) is untouched.
- **C16. The phone gets finish events only when it subscribes with `finished: true`**, as `inbox: true` works; an older phone sees nothing new.
- **C17. `Delegation.background`** is read (serde default) from P2 for the Inbox's `JobFinished`, written from P3.
- **C18. Keep awake has no consumer in the Rust app** on the base (`AwakeGuard` has no caller); P2 adds only the pure `signals::busy`.
- **C19. Daemon file names** `signal_watch.rs`, `signals_ops.rs`: `notchlingd/src/signals.rs` is the SIGTERM handler.
- **C20. The held queue's release** is `Held::ready_for` beside `ready` (whose tests stay); release on `turn.finished`, the 1 s tick kept only for the 20 s start grace.
- **C21. Runners stay one turn at a time.** The agy runner keeps `RunError::Busy`; `CodexRuns` loses its outbox and answers `Busy`; holding happens in the daemon's ops.
- **C22. Phase 5 is one pull request per item**; 5g (Codex lanes) only if the maintainer wants it.
- **C23. Non-provider enums stay.** `versions::Tool` and `cluster::Tool` (with Notchling and Shell) convert to and from `contract::Tool`; the ninth copy, `delegate::skill::SkillTool`, goes in P6 unless decision 0019 removed it first.
- **C24. Log kinds** `turn.finished` and `subagent.finished` land in P2 only if logging P1 has merged; logging's `tool.turn` is retired for them, and its tool values use `antigravity`.
- **C25. Words.** User-facing words follow the locale rule in force when a task runs (messages after localization L1; a literal fails once its area is armed). The contract's `how`, `why` and `plan` lines are engine data in English, shown as they are.
- **C26. `NOTCHLING_SESSION` on the agy runner** is set only for a session loaded by id; a brand-new session's first `agy acp` has no id yet, so its jobs fall to the process tree.
- **C27. "What Notchling Can Do" is a page-level part** of Settings > Agents, not a tool section, so `agents_tests.rs` (which pins `Tool::ALL == [ClaudeCode]` and one section) is untouched.
- **C28. `sessions.list` stays Claude Code only by default**; `{"tools": "all"}` is the additive way to ask for every tool (`notchling sessions`, the bridge, `SignalWatch`).
- **C29. Codex forks are one fact.** `own_start(path)`, the byte where a child's own turn starts, is computed once per file stamp and every Codex reader (parser, `turn_open`, glance, Search, cost, title) ignores bytes before it. `lineage(path)` is a new function; `Meta` keeps its fields and `read_meta` keeps the first `session_meta`.
- **C30. Ids are normalized on read.** From P1, writers emit `antigravity:`; every reader of a stored id maps `agy:` to `antigravity:` first. The key rewrite (C3) then only tidies stored data: nothing depends on it having run, and the app may read the store before the daemon rewrites it.
- **C31. The phone protocol keeps `agy`.** Its schema documents `Session.tool` as `claude, codex or agy`, no phone reads it, and the protocol changes additively only, so the value stays `agy` and the new `Subagent.tool` uses the same words. Row ids on the phone become `antigravity:` like everywhere.
- **C32. Removing the alias ops is the design's one non-additive local API change** (`crates/AGENTS.md`: the local API changes additively only). The approved design removes them in P6; the pull request says so first, after checking no client uses them.
- **C33. What a closed gap becomes.** Codex `delegate.caller` Native (`CODEX_THREAD_ID` is Codex's); Antigravity `delegate.caller` Fallback `process-tree` (the id is Notchling's); Codex `turn.take_back` and Antigravity `turn.hold`, `turn.take_back` Fallback `held-queue`; Antigravity `session.remove` Fallback `hide-row`; everything else that closes becomes Native.

## Dependencies on other work

| Work | Branch | What it means here |
| --- | --- | --- |
| Logging stream | `agent/logging-design` (decision 0021 placeholder) | P2 2.11 only if logging P1 merged (C24). Logging P2 2.18 edits `held_ops.rs` (P2 2.5, P4); P5 5.7 edits `delegate_cli.rs` (P3); P5 5.8 edits `codex_ops.rs`, `agy_ops.rs` (P4, P5b, P6.1) and `bridge/hub.rs`, `router.rs` (P2 2.9); P3 3.5 edits `sheets/agents.rs` and `agents_tests.rs` (P0 0.7 edits `agents_view.rs` only); P7 edits the protocol schema (P2 2.9). Its `LINT-LOG-ONE-WRITER` names the command-line files allowed to print: `providers_cli.rs` writes through `&mut dyn Write` and is added to that list if the lint needs it |
| Localization L1 | `agent/locale-design` (0022) | New words in P0 0.7, P1 1.9, P2 2.8, P4 4.5, P5c, P5d are messages once L1 has merged; L3 arms `settings`, L4a to L4e arm `chat`, L5 the notch (C25). L2b makes bridge and daemon strings English: the Inbox `JobFinished` title is English data |
| Version sync | `agent/version-sync-plans` (0026, 0027) | `version.detect` and `version.update` leave `gaps.json` in its pull requests, wired or NotApplicable in the same pull request. Its `versions::Tool` stays (C23); its stored and wire values should say `antigravity` (C30). It edits `wire.rs`, `daemon_wiring.rs`, `inbox/model.rs`, `settings/keys.rs`, `quiet_ops.rs`, `delegate_cli.rs` (P1 T10) and `sheets/agents.rs` (P1 T9 adds Codex and Antigravity to `sheets::agents::Tool`, which P6 6.2 may then replace) |
| Agents setup | `agent/agents-setup-decision` (0019, Proposed) | `setup.rules`, and `setup.hooks` for Codex and Antigravity, leave `gaps.json` when 0019 lands. If 0019 moved the delegate skill to `crates/engine/data/instructions/notchling-delegate.md`, P3 3.6 edits that file and bumps its version. 0019 also replaces tool enum copies with `catalog::Tool` (now `contract::Tool`): whichever lands first, the other rebases. P5i builds on its `instructions::Target` |
| Cluster plans | `agent/cluster-v1-plans` (0021 to 0025 claimed) | B T13 adds `--machine` to delegate options and `delegate_cli.rs` (P3: merge both into `USAGE`); B T18, D T10, F T5 edit the protocol schema (P2 2.9: rerun `gen.py` after rebasing); B T11, C T8, E T8 add inbox `Kind`s (P2 2.6); C and D edit `inbox_ops.rs`; E edits `codex_ops.rs`, `agy_ops.rs`, `install/hooks.rs`. `cluster::Tool` stays (C23) |
| Hybrid search | `agent/hybrid-search-plans` | P5 T6 changes `search/index/read.rs` to `read_with_lines(meta, tool: SessionTool)` (P1 1.2, 1.10, P5e, P6 6.2); P1 T8 adds `effect` to `KeySpec` (P2 2.8's key takes `.effect(..)`); P1 T2 changes `fts.rs` (P1 1.10's `SessionMeta` literal) |

**Files several plans edit** (serialize; rebase on whichever merged first): `protocol/agentconsole.v1.schema.json`, `crates/engine/src/inbox/model.rs`, `crates/engine/src/settings/keys.rs`, `crates/local-api/src/wire.rs`, `crates/notchlingd/src/daemon_wiring.rs`, `crates/notchlingd/src/delegate_cli.rs`, `crates/notchlingd/src/held_ops.rs`, `crates/notchlingd/src/codex_ops.rs`, `crates/notchlingd/src/agy_ops.rs`, `crates/engine/src/bridge/hub.rs`, `crates/engine/src/bridge/live.rs`, `crates/engine/src/search/index/read.rs`, `crates/app/src/sheets/agents*.rs`, `docs/feature-map.md`, `crates/AGENTS.md`.

## Maintainer only

- Accept the decision record (C5) and merge every phase; nothing here merges itself.
- **P0:** the contract module, the starting `gaps.json` (34) and `unwired.json` (72) (`GUARD-PROVIDER-GAPS` fires on this pull request by design), the rewritten `.claude/skills/add-provider/SKILL.md`, the `docs/rules.md` row and `tools/guard` change.
- **P1:** the Codex child index (`CAP-FILEWRITE`, a new SQLite file under `<data>/daemon/`) and its app-tier op `codex.children`; changes to `tools/codex`, `tools/agy`, `cost`, `contract`; the key rewrite (C3); `WEAK-TEST-EDIT` on `crates/engine/tests/transcript_subagents.rs` and the id assertions that change to `antigravity:`.
- **P2:** the protocol schema and generated types, the phone's `lib/app/message_alerts.dart`, the `notifyFinished` key, the daemon's watch and ops, the quiet rule's 30 s window.
- **P3:** all of it (`delegate`, `notchlingd`, `contract`): `CAP-PROCESS` (the `claude -p` runner, the detached background run, `--stop`'s signals) and `CAP-FILEWRITE` (records for every job, `.out` files); `WEAK-TEST-EDIT` on `crates/notchlingd/tests/delegate_cli.rs` (a `claude` case that used to be refused); the first real Claude delegation, which costs a turn.
- **P4:** sending is the maintainer's zone; the Codex outbox tests that change from one turn per message to one joined turn (spec call 7).
- **P5:** whether Codex lanes are wanted (5g); `catalog.json` data (5f); every item outside the auto zone.
- **P6:** removing the alias ops (C32), the enum removals' test edits, `GUARD-TOOL-BRANCH` and its row, the decision's status.

## Found on the base while planning

1. **The phone through the daemon lists Claude Code only.** `DaemonSources` (`notchlingd/src/bridge_backend.rs`) does not implement `Sources::foreign_rows`, whose default is empty. P2 2.2 fills it.
2. **Keep awake is not wired in the Rust app.** `platform::awake::AwakeGuard` has no caller; the app's `SetAwake` only stores the choice. P2 2.10 adds the pure part only (C18).
3. **A ninth tool enum**, `delegate::skill::SkillTool`, beside the eight the spec counts (C23).
4. **`notchlingd/src/signals.rs` is the SIGTERM handler**, hence the file names in C19.
5. **`WEAK-TEST-EDIT` trips only on paths under `crates/*/tests/`, `apps/flutter/test/`, `tools/guard/tests/`, `legacy/macos/Tests|Fixtures/` and golden PNGs.** Edits to in-crate `src/*_tests.rs` and inline test modules are not caught; every plan lists them for the reviewer anyway.
6. **Search indexes Codex children as sessions today**, the same bug as the Sidebar's (P1 1.10).
7. **The phone protocol documents `Session.tool` as `agy`**, and no phone code reads it (C31).
8. **Three tests pin `claude` as a tool `notchling delegate` refuses**, one under `crates/notchlingd/tests/` (P3 3.1).
9. **`app/src/chat/view/tool/mod.rs` pins that only Codex queues** (`only_codex_queues_through_the_daemon_so_far`; P4 4.5).
10. **`.claude/skills/add-protocol-field` still has Swift fixture steps** (`legacy/macos/Tests/...`); P2 2.9 follows its schema and generator steps and writes protocol fixtures by hand.
11. **`transcript::subagents::console_list` has no caller** (P6 6.4 removes it).
12. **Decision numbers collide among the sibling plans** (C5).
