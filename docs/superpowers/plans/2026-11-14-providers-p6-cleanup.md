# Providers, Phase 6: Clean-up Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Leave one way to name a tool and one way to act on a session. The `codex.*` and `agy.*` alias ops go once nothing calls them; the tool enums that copied `contract::Tool` go; a guard rule, `GUARD-TOOL-BRANCH`, stops a surface from branching on the tool again; the notes and the add-provider skill describe what is there.

**Architecture:** No new behavior. The app and the phone bridge already call `session.*` (Phases 4 and 5); this phase deletes the aliases, replaces each copied enum by `contract::Tool` (with `From` conversions where a non-provider enum stays), and adds a lint over new and changed lines in `tools/guard`.

**Tech Stack:** Rust 1.99.1; Python 3.9 standard library for the guard.

**Spec:** sections 6.1, 11.5, 11.6, 12 (phase 6). Phase 5 merged (at least 5a and 5b, which move every caller of the aliases). Calls C8, C14, C23 in the overview.

**Branch:** `agent/providers-p6-cleanup`.

## Global Constraints

- Phase 0 to 5 constraints hold.
- **The one non-additive local API change in this design.** `crates/AGENTS.md` says the local API changes additively only; the approved design removes the alias ops in this phase. The app and the daemon ship in one bundle, and no agent-tier client (`notchling` commands, the hook) calls them (checked in Task 6.1). The pull request says this in its first line so the maintainer sees it.
- `versions::Tool { Claude, Codex, Agy, Notchling }` (version sync) and `cluster::Tool { Claude, Codex, Agy, Shell }` (cluster plans) are not providers and stay; each gets `TryFrom<…> for contract::Tool` and `From<contract::Tool>` where they meet (call C23).
- `GUARD-TOOL-BRANCH` looks only at new or changed lines, like the architecture lint, so today's code is not a baseline to clear.

## Review Focus

1. No caller of an alias op is left in the app, the phone bridge, the hook or a `notchling` command before the ops go. (Task 6.1 test `no_client_names_an_alias_op`.)
2. The serde names of every replaced enum stay the same on disk and on the wire (`search.db` metas, delegation records, foreign rows): a value written before this phase reads after it. (Task 6.2 test `stored_values_read_after_the_enums_go`.)
3. `GUARD-TOOL-BRANCH` fires on a new `match tool { Tool::Codex => … }` in `crates/app/src/chat/` and stays quiet in `contract/`, `providers/` and `Tool::of_row`. (Task 6.3 tests `test_a_new_branch_in_chat_fires`, `test_the_contract_may_branch`.)
4. Removing `chat::delegated`'s wrappers leaves no behavior untested: each removed app test has its engine twin from Phase 1. (Task 6.4 lists the pairs.)

## Order and lanes

```
6.1 alias ops ─┐
6.2 enums ─────┼─► 6.4 notes, skill, leftovers
6.3 GUARD-TOOL-BRANCH (any time; tools/guard only)
```

## Zones and gates

| Task | Zone | Gates expected | Existing tests edited |
| --- | --- | --- | --- |
| 6.1 | Maintainer (`local-api/**`, `notchlingd/**`); auto (`app/**`) | none | in-crate, listed: `notchlingd/src/codex_ops_tests.rs`, `codex_acting_tests.rs`, `codex_slow_tests.rs`, `quiet_ops_tests.rs` and the agy ops tests call `session.*` instead of the aliases (same assertions); `app/src/tools/requests.rs` tests build `session.*` requests |
| 6.2 | Mixed: maintainer (`bridge/**`, `delegate/**`, `tools/**`); auto (`app/**`, `search/**`) | none | in-crate, listed: tests that name `bridge::foreign::Tool::Agy`, `delegate::options::Tool::Agy`, `search::meta::SessionTool`, `chat::data::Tool`, `tools::requests::RowTool`, `delegate::skill::SkillTool` switch to `contract::Tool` (no assertion changes). Tripped `WEAK-TEST-EDIT` only if a file under `crates/*/tests/` names one (check with `grep -rln 'SessionTool\|RowTool\|SkillTool\|options::Tool' crates/*/tests`) |
| 6.3 | Maintainer (`tools/**`, `docs/rules.md`) | the rule's probe | none: new `tools/guard/tests/test_tool_branch.py` |
| 6.4 | Maintainer (`AGENTS.md` files, `.claude/skills/`, `docs/decisions/`, feature map); auto (`app/**`) | `ZONE` | in-crate, listed: `app/src/chat/delegated_tests.rs` removed with `delegated.rs`'s wrappers (call C14); their engine twins are `contract/flows/delegate_jobs_tests.rs` from Phase 1 |

## Maintainer only (Phase 6)

- Removing the alias ops (6.1): the one non-additive local API change.
- The enum removals' test edits (6.2), the guard rule and its row in `docs/rules.md` (6.3), the decision record's status and the skill (6.4).

---

### Task 6.1: Remove the `codex.*` and `agy.*` alias ops

**Files:**
- Modify: `crates/local-api/src/wire.rs` (remove `CODEX_SEND`, `CODEX_STOP`, `CODEX_DELETE`, `AGY_SESSIONS`, `AGY_STATE`, `AGY_CONVERSATION`, `AGY_OPEN`, `AGY_SEND`, `AGY_CANCEL`, `AGY_SET_OPTION`, `AGY_PERMISSION`; keep `codex.children` (Phase 1) and the topics), `crates/notchlingd/src/codex_ops.rs`, `crates/notchlingd/src/agy_ops.rs` (their `spec` and `call` drop the aliases; the work is reached through the registry's ports), `crates/notchlingd/src/quiet_ops.rs` (if it lists one), `crates/app/src/tools/requests.rs` (no alias request builders left), `docs/local-api-ops.md`
- Test: `crates/notchlingd/src/alias_gone_tests.rs` (new)

- [ ] **Step 1: Write the failing tests:** `no_client_names_an_alias_op` (reads every `.rs` under `crates/app/src`, `crates/notchlingd/src` and `crates/engine/src/bridge` from `CARGO_MANIFEST_DIR` and finds none of the eleven names outside `wire.rs` history and this test), `an_alias_op_is_unknown` (a real server: `codex.send` answers the unknown-op error).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd alias_gone`; expected FAIL.
- [ ] **Step 3: Remove** the ops and their request builders; switch the listed in-crate tests to `session.*`.
- [ ] **Step 4: Run** `cargo test --workspace --locked`, the e2e tests. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "local api: remove the codex.* and agy.* aliases; session.* serves every tool"`.

### Task 6.2: One `Tool`

**Files:**
- Modify (each enum replaced by `contract::Tool`, its users switched): `crates/app/src/chat/data.rs` (`Tool`), `crates/app/src/tools/requests.rs` (`RowTool`), `crates/engine/src/bridge/foreign.rs` (`Tool { Codex, Agy }`; the phone's `Session.tool` value stays `agy` through a `phone_name(tool)` function, call C31), `crates/engine/src/delegate/options.rs` (`Tool`; `title` keeps `Antigravity CLI` in the delegate's output through `delegate::title(tool)`), `crates/engine/src/search/meta.rs` (`SessionTool`; serde keeps reading `agy`), `crates/engine/src/delegate/skill.rs` (`SkillTool`, unless decision 0019 already replaced it), `crates/app/src/sheets/agents.rs` (`Tool { ClaudeCode }` and any variants version sync P1 T9 added: replaced by `contract::Tool` only if version sync has merged and the page lists all three; otherwise left, it is a page section list), `crates/engine/src/versions/` and `crates/engine/src/cluster/` (conversions only, if those modules exist)
- Test: `crates/engine/src/contract/one_tool_tests.rs` (new)

- [ ] **Step 1: Write the failing tests:** `stored_values_read_after_the_enums_go` (a `SessionMeta` JSON with `"tool": "agy"`, a delegation record with `"tool": "agy"` and one with `"antigravity"`, a foreign row id `agy:x` all read as `Tool::Antigravity`), `the_phone_still_hears_agy`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine contract::one_tool`; expected FAIL (the functions do not exist yet).
- [ ] **Step 3: Replace** one enum per commit, building the workspace after each.
- [ ] **Step 4: Run** `cargo test --workspace --locked`; `grep -rn "enum Tool\b\|enum SessionTool\|enum RowTool\|enum SkillTool" crates/*/src` lists only `contract/tool.rs` and the non-provider enums. Expected: PASS.
- [ ] **Step 5: Commit** (one per enum) `git commit -m "<area>: contract::Tool replaces <enum>"`.

### Task 6.3: `GUARD-TOOL-BRANCH`

**Files:**
- Modify: `tools/guard/guard/lint.py` (a check over added and changed lines of `crates/**/*.rs`: a `Tool::Claude`, `Tool::Codex` or `Tool::Antigravity` pattern in a `match` arm or an `==`/`!=`/`matches!` comparison is reported, except under `crates/engine/src/contract/`, `crates/engine/src/providers/`, `crates/engine/src/accounts/`, in the body of `Tool::of_row`, and in `*_tests.rs` and `crates/*/tests/`), `tools/guard/guard/rules.py` (a probe with a bad example), `docs/rules.md` (row: `GUARD-TOOL-BRANCH` | A surface asks the registry, never branches on the tool | A surface that names tools can leave one out, which the contract exists to prevent | `python3 -m guard lint`; bad example: `match session.tool { Tool::Codex => …, _ => … }` in `crates/app/src/chat/`)
- Test: `tools/guard/tests/test_tool_branch.py` (new)

- [ ] **Step 1: Write the failing tests:** `test_a_new_branch_in_chat_fires`, `test_an_old_line_does_not_fire` (unchanged line), `test_the_contract_may_branch`, `test_of_row_may_branch`, `test_tests_may_branch`, `test_a_comparison_fires`.
- [ ] **Step 2: Run** `PYTHONPATH=tools/guard python3 -m unittest discover -s tools/guard/tests -t tools/guard -p test_tool_branch.py -v`; expected FAIL.
- [ ] **Step 3: Implement**; standard library only.
- [ ] **Step 4: Run** `tools/test-python.sh`, `PYTHONPATH=tools/guard python3 -m guard rules` (the probe fires), `python3 -m guard lint` on this branch (0 new). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "guard: GUARD-TOOL-BRANCH, surfaces ask the registry"`.

### Task 6.4: Notes, the skill, leftovers

**Files:**
- Modify: `crates/app/src/chat/delegated.rs` (the wrappers go; callers use `contract::flows::delegate_jobs`; `delegated_tests.rs` goes with them), `crates/engine/src/transcript/subagents.rs` (`console_list` and `ConsoleSubagent`, which nothing calls, go), `.claude/skills/add-provider/SKILL.md` (checked against what exists; the line `Tried: not yet` becomes the date it was walked through on paper), `docs/decisions/0028-provider-capability-contract.md` (status, placeholder number replaced by the one assigned), `crates/AGENTS.md`, `crates/engine/AGENTS.md`, `crates/app/AGENTS.md`, `crates/notchlingd/AGENTS.md` (every note that still names an alias op or a removed enum), `docs/feature-map.md`, `docs/provider-matrix.md` (regenerated; unchanged unless a line moved)

- [ ] **Step 1:** `grep -rn "codex\.send\|agy\.send\|SessionTool\|RowTool\|SkillTool\|console_list" crates docs .claude` lists only history (decisions, plans).
- [ ] **Step 2:** remove the leftovers; `cargo test --workspace --locked`.
- [ ] **Step 3:** write the notes; `PYTHONPATH=tools/guard python3 -m guard claudemd`, `python3 tools/strip_comments.py --check`.
- [ ] **Step 4: Commit** `git commit -m "docs: the provider contract, finished"`.

## Checkpoint P6

- `cargo test --workspace --locked`, clippy (Linux and the Mac), fmt, deny; `tools/test-python.sh`; every `python3 -m guard` check.
- `gaps.json` holds only design gaps; `unwired.json` is empty; `GUARD-PROVIDER-GAPS` and `GUARD-TOOL-BRANCH` are in `docs/rules.md` and their probes fire.
- The pull request's first line names the alias op removal for the maintainer.
