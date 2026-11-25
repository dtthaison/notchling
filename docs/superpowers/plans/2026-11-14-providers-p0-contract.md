# Providers, Phase 0: the Contract and the Matrix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new engine module `contract/` declares every one of the 39 capabilities for Claude Code, Codex and Antigravity as Native, Fallback, NotApplicable or Gap, with the two shrink-only files `gaps.json` (34 entries) and `unwired.json` (72 entries), a conformance suite that runs only wired cells, the generated `docs/provider-matrix.md`, `notchling providers [--json]`, a guard rule that stops either file from growing, and a read-only "What Notchling Can Do" part in Settings > Agents. No behavior changes.

**Architecture:** `contract::Tool` (moved from `catalog::Tool`, re-exported there) and `contract::Capability` are the two closed lists. Each provider folder has a `support.rs` that is one exhaustive `match` under `#![deny(clippy::wildcard_enum_match_arm)]`, plus a `WIRED` list of the cells the registry routes (empty in this phase). `Registry` answers `support(tool, capability)` and `is_wired`; `matrix.rs` renders the registry as Markdown and JSON. Ports arrive in the phase that wires them (call C6 in the overview).

**Tech Stack:** Rust 1.99.1, `serde`, `serde_json` (already engine dependencies); Python 3.9 standard library for the guard; GPUI through the kit for the Settings part.

**Spec:** `docs/superpowers/specs/2026-11-13-provider-contract-design.md` (b34fabf), sections 5, 6.1 to 6.4, 7.1, 11, 12 (phase 0), "Calls settled after review" 1, 2 and 8, and 14.2. Read sections 6.2 and 11 before Task 0.1. The overview, `2026-11-14-providers-overview.md`, holds the calls (C1 to C33) these tasks refer to.

**Branch:** `agent/providers-p0-contract`, from `origin/merge/settings-batch` (cc28da7) or `agent-run-oss` once that branch has it. Never `main`.

## Global Constraints

- No comments in code, doc comments included; the why goes in `crates/engine/AGENTS.md` and `crates/AGENTS.md`. `python3 tools/strip_comments.py --check` passes.
- Wire names: `Tool` serializes as `claude`, `codex`, `antigravity`; it reads `agy` as Antigravity. `agy` is only the command's name.
- Every `how`, `why` and `plan` line is one line, at most 120 characters, no trailing period required, never empty.
- Only a Gap whose plan starts with `design: 0019` or `design: version-sync` may name a design instead of a phase; every other Gap plan starts with `phase <1-6>: `.
- `gaps.json` and `unwired.json` are `{"entries": [...]}`, entries `"<tool name> <capability id>"` (for example `"codex subagent.list"`), sorted, unique, one per line, trailing newline. They only shrink once this phase merges (the user's decision 14.2; starting numbers 34 and 72, call C1).
- No count of cells is pinned in a test: counts live in the generated matrix, so later phases change no test when a gap closes.
- Files under 1,000 lines (600 warns). No new crate in `Cargo.lock`.
- User-facing words in `crates/app` follow the locale rule in force when the task runs (call C25): after localization L1 merges they are messages in `locales/en/mac/settings.ftl` called through `t::`, and once L3 arms the `settings` area a literal fails `LINT-LOCALE-LITERAL`. The declaration lines themselves are engine data, English, shown as they are (as `settings/keys.rs` titles are).

## Review Focus

1. Someone adds a fourth `Tool` variant: every `support.rs` match, the conformance macro and `Registry::standard` must fail to build until the tool is declared, and the new tool must start with no Gap and no unwired cell. (Task 0.2 `the_wildcard_lint_is_on_in_every_declaration` check; Task 0.3 test `a_new_tool_starts_with_no_gaps`.)
2. `catalog::Tool` callers in the daemon and the app (`catalog_ops.rs`, `bridge_backend.rs`, `session_source.rs`, `chat/view/tools.rs`) must keep compiling and serializing the same; `Tool::parse("gemini")` must still be `None`. (Task 0.1 tests `catalog_tool_is_the_contract_tool`, `tool_reads_agy_and_writes_antigravity`.)
3. A pull request that moves a cell out of `gaps.json` by turning it NotApplicable to make the count drop: allowed by the guard (the file shrank) but visible in the regenerated matrix diff; the registry test still demands each NotApplicable carry a reason. (Task 0.3 `every_gap_is_listed_and_every_listed_cell_is_a_gap`.)
4. The matrix golden differing between macOS and Linux CI because of map order: `matrix::markdown` and `matrix::json` iterate `Tool::ALL` and `Capability::ALL`, never a `HashMap`. (Task 0.5 test `the_matrix_matches_the_golden` runs on the Linux leg.)
5. The guard reading a `gaps.json` that is malformed at the head: the gate must refuse with a reason, not pass. (Task 0.6 test `test_unreadable_head_is_a_reason`.)

## Order and lanes

```
0.1 Tool and Capability ──► 0.2 declarations ──► 0.3 registry and the two files ──► 0.4 conformance ──► 0.5 matrix and CLI
0.6 GUARD-PROVIDER-GAPS        (beside 0.1 to 0.5: tools/guard and docs/rules.md only)
0.7 What Notchling Can Do      (after 0.3: crates/app only)
0.8 decision, notes, skill, feature map   (beside; its feature-map step after 0.1)
```

Lanes that never touch the same file: **A** (0.1 to 0.5: `crates/engine/**`, `crates/notchlingd/src/{cli,lib,providers_cli}.rs`, `docs/provider-matrix.md`), **B** (0.6: `tools/guard/**`, `docs/rules.md`), **C** (0.7: `crates/app/src/sheets/**`), **D** (0.8: `docs/decisions/`, `.claude/skills/add-provider/`, every `AGENTS.md`, `docs/feature-map.md`).

## Zones and gates

| Task | Zone | Gates expected | Existing tests edited |
| --- | --- | --- | --- |
| 0.1 to 0.4 | Maintainer (`crates/engine/src/contract/**`, `crates/engine/src/catalog/**`, `crates/engine/src/lib.rs`) | none of `CAP-*`; `crates/engine/tests/conformance/` is new | none |
| 0.5 | Maintainer (`crates/notchlingd/**`); `docs/provider-matrix.md` is auto | none (writes through `&mut dyn Write`); `crates/engine/tests/provider_matrix.rs` new | none: `cli.rs`'s `every_client_command_shows_json_in_the_usage` keeps its list; a new test covers `providers` |
| 0.6 | Maintainer (`tools/**`, `docs/rules.md`) | `GUARD-PROVIDER-GAPS` fires on this phase's own PR (the files are new: everything "grew"), which is the moment the maintainer accepts 34 and 72 | none: checks go in a new `tools/guard/tests/test_provider_gaps.py` |
| 0.7 | Auto (`crates/app/**`) | none | none: `agents_tests.rs` keeps `Tool::ALL == [ClaudeCode]` because the part is page-level, not a tool section (call C27) |
| 0.8 | Maintainer (`docs/decisions/`, `.claude/`, `AGENTS.md` files, `docs/feature-map.md`) | `ZONE` | none |

## Maintainer only (Phase 0)

- Accept the decision record (number assigned at merge, call C5) and merge.
- Accept the starting baselines: `gaps.json` 34 entries (Claude Code 4, Codex 14, Antigravity 16; 11 of them `design: …`), `unwired.json` 72 entries (call C1). `GUARD-PROVIDER-GAPS` fires once on this PR for that reason.
- The rewritten `.claude/skills/add-provider/SKILL.md` and the `docs/rules.md` row.

---

### Task 0.1: `contract::Tool` and `contract::Capability`

**Files:**
- Create: `crates/engine/src/contract/mod.rs`, `crates/engine/src/contract/tool.rs`, `crates/engine/src/contract/capability.rs`
- Modify: `crates/engine/src/lib.rs` (`pub mod contract;`), `crates/engine/src/catalog/mod.rs:17-48` (the enum leaves; `pub use crate::contract::Tool;` takes its place; the private `Tool::index` and `Tool::ALL` uses switch to the contract's)
- Test: `crates/engine/src/contract/tool_tests.rs`

**Interfaces:**
- Produces (`notchling_engine::contract`):
  - `pub enum Tool { Claude, Codex, Antigravity }`, derives `Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize`, `#[serde(rename_all = "lowercase")]`, `#[serde(alias = "agy")]` on `Antigravity`; `pub const ALL: [Tool; 3]`; `parse(&str) -> Option<Tool>` (`claude`, `codex`, `antigravity`, `agy`); `name(self) -> &'static str` (`claude`, `codex`, `antigravity`); `title(self) -> &'static str` (`Claude Code`, `Codex`, `Antigravity`); `pub(crate) fn index(self) -> usize`; `of_row(row_id: &str) -> Tool` (`codex:`, `codex-new:` give Codex; `antigravity:`, `antigravity-new:`, `agy:`, `agy-new:` give Antigravity; anything else Claude Code); `row_prefix(self) -> &'static str` (`""`, `"codex:"`, `"antigravity:"`).
  - `pub enum Capability` with the 39 variants of spec section 5 in its order: `SessionList, SessionStatus, SessionNew, SessionStop, SessionRemove, SessionMove, FolderTrust, ConversationRead, ConversationFollow, ConversationContext, SubagentList, SubagentStatus, SubagentGlance, SubagentOpen, SubagentStop, SignalTurnFinished, SignalSubagentFinished, SignalNeedsYou, TurnSend, TurnHold, TurnTakeBack, TurnInterrupt, TurnOptions, PromptQuestion, PromptPermission, CommandSlash, AccountSwitch, AccountLanes, UsageLimits, UsageCost, CatalogModels, DelegateTarget, DelegateCaller, SetupRules, SetupSkills, SetupHooks, VersionDetect, VersionUpdate, SearchText`; `pub const ALL: [Capability; 39]`; `id(self) -> &'static str` (the spec's ids, `session.list` … `search.text`); `title(self) -> &'static str` (Sentence case, at most 40: `Session list`, `Subagent status`, …); `group(self) -> Group`; `parse(&str) -> Option<Capability>`.
  - `pub enum Group { Sessions, Conversation, Subagents, Signals, Turns, AccountsAndUsage, Delegation, Setup, Versions, Search }` with `title()` (`Sessions`, `Conversation`, `Subagents`, `Signals`, `Turns`, `Accounts and usage`, `Delegation`, `Setup`, `Versions`, `Search`).
  - `crate::catalog::Tool` is the same type (re-export), so `notchling_engine::catalog::Tool` keeps every import in `notchlingd` and `app` building unchanged.

- [ ] **Step 1: Write the failing tests** in `tool_tests.rs`:
  - `tool_reads_agy_and_writes_antigravity`: `Tool::parse("agy") == Some(Tool::Antigravity)`; `serde_json::from_str::<Tool>("\"agy\"")` is Antigravity; `serde_json::to_string(&Tool::Antigravity) == "\"antigravity\""`; `Tool::parse("gemini") == None`; `Tool::Antigravity.name() == "antigravity"`.
  - `row_ids_name_their_tool`: `of_row("aa11bb22")` Claude; `"codex:t1"`, `"codex-new:1"` Codex; `"agy:c1"`, `"agy-new:1"`, `"antigravity:c1"`, `"antigravity-new:1"` Antigravity.
  - `thirty_nine_capabilities_with_unique_ids`: `Capability::ALL.len() == 39`; ids unique; each id is lowercase ASCII letters and `_` with exactly one `.` (a byte check, no `regex`); `parse(c.id()) == Some(c)` for all; titles non-empty and at most 40 characters; the groups in `ALL` order are 7 Sessions, 3 Conversation, 5 Subagents, 3 Signals, 8 Turns, 5 AccountsAndUsage, 2 Delegation, 3 Setup, 2 Versions, 1 Search.
  - `catalog_tool_is_the_contract_tool`: `let t: crate::catalog::Tool = Tool::Codex; assert_eq!(t, Tool::Codex)`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine contract::tool_tests` and confirm it fails (no `contract` module).
- [ ] **Step 3: Implement** the three files; `mod.rs` declares `tool`, `capability` and re-exports `Tool`, `Capability`, `Group`; `#[cfg(test)] #[path = "tool_tests.rs"] mod tool_tests;`.
- [ ] **Step 4: Run** `cargo test -p notchling-engine` (the catalog's own `tests.rs` must still pass unchanged), `cargo build -p notchling-notchlingd -p notchling-app`, `cargo clippy -p notchling-engine --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/engine/src/contract crates/engine/src/lib.rs crates/engine/src/catalog/mod.rs` then `git commit -m "contract: Tool moves out of the catalog; the 39 capabilities"`.

### Task 0.2: `Support`, `Flow` and the three declarations

**Files:**
- Create: `crates/engine/src/contract/support.rs`, `crates/engine/src/contract/provider.rs`, `crates/engine/src/contract/claude/mod.rs`, `claude/support.rs`, `crates/engine/src/contract/codex/mod.rs`, `codex/support.rs`, `crates/engine/src/contract/antigravity/mod.rs`, `antigravity/support.rs`
- Test: `crates/engine/src/contract/support_tests.rs`

**Interfaces:**
- Consumes: Task 0.1's `Tool`, `Capability`.
- Produces:
  - `pub enum Support { Native { how: &'static str }, Fallback { flow: Flow, why: &'static str }, NotApplicable { why: &'static str }, Gap { plan: &'static str, meanwhile: Option<Flow> } }` with `kind(&self) -> Kind` (`pub enum Kind { Native, Fallback, NotApplicable, Gap }`, `name()`: `native`, `fallback`, `not_applicable`, `gap`) and `line(&self) -> &'static str`.
  - `pub enum Flow { HeldQueue, HideRow, DelegateJobs, ProcessTree, CatalogData, VersionProbe, VersionCache, TitleIndex }` with `id()` (`held-queue`, `hide-row`, `delegate-jobs`, `process-tree`, `catalog-data`, `version-probe`, `version-cache`, `title-index`) and `covers(self, c: Capability) -> bool`: HeldQueue covers TurnHold, TurnTakeBack; HideRow covers SessionRemove; DelegateJobs covers SubagentList, SubagentStatus, SubagentGlance, SubagentOpen, SubagentStop, SignalSubagentFinished; ProcessTree covers DelegateCaller; CatalogData covers CatalogModels, ConversationContext; VersionProbe covers VersionDetect; VersionCache covers VersionUpdate; TitleIndex covers SearchText.
  - `pub struct Unsupported { pub capability: Capability, pub why: &'static str }`, `Unsupported::not_native(c) -> Unsupported` (`why`: `"not native"`).
  - `pub trait Provider: Send + Sync { fn tool(&self) -> Tool; fn support(&self, c: Capability) -> Support; fn wired(&self) -> &'static [Capability]; }` (later phases add one port accessor each, call C6).
  - `contract::claude::ClaudeProvider`, `contract::codex::CodexProvider`, `contract::antigravity::AntigravityProvider` (unit structs in this phase), each with `pub const WIRED: &[Capability] = &[];` returned by `wired()`, and `support()` calling its folder's `support::support(c)`.
  - Each `support.rs` starts with `#![deny(clippy::wildcard_enum_match_arm)]` and holds `pub fn support(c: Capability) -> Support`, one arm per capability, no `_`.
- The cells are the overview's "The matrix at phase 0" table (spec 7.1 after call C1). Native `how` is the spec 7.1 cell's text after `N:` without its bug and check notes (or a few words naming the source where the cell is just `N`), cut to 120. These lines are fixed exactly:
  - NotApplicable (11). Claude Code `subagent.stop`: `No outside stop for one subagent; Stop interrupts the whole turn`. Codex `session.stop`: `Nothing runs between Codex turns; Stop interrupts the turn`; `folder.trust`: `codex exec asks no trust question; the sandbox is set per turn`; `subagent.stop`: `Only the model's interrupt_agent stops a Codex subagent`; `signal.needs_you`: `codex exec never asks`; `prompt.question`: `codex exec can't ask a question`; `prompt.permission`: `Codex turns run in a sandbox and never ask`. Antigravity `session.move`: `One Antigravity login per Mac; a switch moves every session`; `subagent.stop`: `The Agent Manager's kill is inside agy; Notchling can't reach it`; `prompt.question`: `ACP carries permission requests only`; `account.lanes`: `One Antigravity login per Mac`.
  - Fallback (3). Claude Code `turn.hold` HeldQueue: `Claude Code's own queue is lost on restart and injected mid-turn`; `turn.take_back` HeldQueue: `The held message is Notchling's, so Notchling edits it`. Codex `turn.hold` HeldQueue: `codex exec takes one message per turn; Notchling holds the rest`.
  - Gap (34), `meanwhile: None` unless named. Claude Code: `delegate.target` `phase 3: notchling delegate claude`; `setup.rules` `design: 0019`; `version.detect`, `version.update` `design: version-sync`. Codex: `session.move` `phase 5: a lane needs its own CODEX_HOME`; `subagent.list` `phase 1: child rollouts by parent_thread_id`; `subagent.status` `phase 1: the child's task_complete or turn_aborted`; `subagent.glance` `phase 1: the child's token_count and newest call` (meanwhile `DelegateJobs`, which glances delegated rows today); `subagent.open` `phase 1: the rollout from the child's own turn`; `signal.subagent_finished` `phase 2: the child's task_complete`; `turn.take_back` `phase 4: held-queue`; `command.slash` `phase 5: Codex's slash commands in Chat`; `account.lanes` `phase 5: CODEX_HOME per account`; `delegate.caller` `phase 3: CODEX_THREAD_ID`; `setup.rules` `design: 0019`; `setup.hooks` `design: 0019 (planned not applicable)`; `version.detect`, `version.update` `design: version-sync`. Antigravity: `session.remove` `phase 5: hide-row; agy has no delete`; `subagent.list` `phase 1: summaries rows by parent_conversation_id`; `subagent.status` `phase 1: summaries status and killed, send_message`; `subagent.glance` `phase 1: the child log's tokens and newest step`; `subagent.open` `phase 1: the child's log`; `signal.subagent_finished` `phase 2: the child goes idle after send_message`; `turn.hold` `phase 4: held-queue; the box is off while it works`; `turn.take_back` `phase 4: held-queue`; `command.slash` `phase 5: available_commands_update in Chat`; `catalog.models` `phase 5: context windows in catalog.json` (call C11); `delegate.caller` `phase 3: NOTCHLING_SESSION, then the process tree`; `setup.rules` `design: 0019`; `setup.hooks` `design: 0019 (planned not applicable)`; `version.detect`, `version.update` `design: version-sync`; `search.text` `phase 5: text from its logs` (meanwhile `TitleIndex`).
  - Native, by call C1 (not in the spec's 7.1): `setup.skills` for all three (`install::skills writes the notchling-* skills`), Claude Code `setup.hooks` (`install::hooks writes Notchling's five hooks`).

- [ ] **Step 1: Write the failing tests** in `support_tests.rs` (the providers are iterated through a local `fn all() -> [&'static dyn Provider; 3]`):
  - `every_cell_has_a_short_line`: for each provider and capability, `line()` is non-empty, at most 120 characters, has no leading or trailing space and no newline.
  - `fallbacks_name_a_flow_that_covers_them`: every Fallback's `flow.covers(c)`, and every Gap's `meanwhile` covers `c` when set.
  - `gaps_name_a_phase_or_a_design`: every Gap plan starts with `phase ` and a digit 1 to 6 then `: `, or with `design: 0019` or `design: version-sync`.
  - `the_design_gaps_are_the_setup_and_version_rows`: the set of cells whose plan starts with `design: ` equals Claude Code {setup.rules, version.detect, version.update}, Codex and Antigravity {setup.rules, setup.hooks, version.detect, version.update} (11).
  - `built_setup_is_native`: `setup.skills` is Native for all three; Claude Code `setup.hooks` is Native.
  - `the_wildcard_lint_is_on_in_every_declaration`: reads the three `support.rs` files from `env!("CARGO_MANIFEST_DIR")` and asserts each starts with `#![deny(clippy::wildcard_enum_match_arm)]` and contains no `_ =>`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine contract::support_tests`; expected FAIL (missing items).
- [ ] **Step 3: Implement** the files above.
- [ ] **Step 4: Prove the lint.** Add a temporary `_ => Support::NotApplicable { why: "x" },` arm to `codex/support.rs`, run `cargo clippy -p notchling-engine -- -D warnings`, see `wildcard_enum_match_arm` fail; remove the arm.
- [ ] **Step 5: Run** `cargo test -p notchling-engine contract` and `cargo clippy -p notchling-engine --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 6: Commit** `git add crates/engine/src/contract` then `git commit -m "contract: every capability declared for every tool"`.

### Task 0.3: The registry, `gaps.json` and `unwired.json`

**Files:**
- Create: `crates/engine/src/contract/registry.rs`, `crates/engine/src/contract/baseline.rs`, `crates/engine/src/contract/gaps.json`, `crates/engine/src/contract/unwired.json`
- Test: `crates/engine/src/contract/registry_tests.rs`

**Interfaces:**
- Consumes: `Provider`, `Support`, the three providers.
- Produces:
  - `pub struct Registry { providers: Vec<Box<dyn Provider>> }`, `Registry::standard() -> Registry` (the three, in `Tool::ALL` order), `of(&self, tool: Tool) -> &dyn Provider`, `support(&self, tool: Tool, c: Capability) -> Support`, `is_wired(&self, tool: Tool, c: Capability) -> bool` (`c` in the provider's `wired()`), `cells(&self) -> Vec<Cell>` in `Tool::ALL` then `Capability::ALL` order.
  - `pub struct Cell { pub tool: Tool, pub capability: Capability, pub support: Support, pub wired: bool }`, `Cell::entry(&self) -> String` (`"codex subagent.list"`).
  - `baseline::GAPS: &str = include_str!("gaps.json")`, `baseline::UNWIRED: &str = include_str!("unwired.json")`, `baseline::entries(text: &str) -> Result<Vec<String>, String>`, `pub const FOUNDING: [Tool; 3] = Tool::ALL`.
- The files are generated once, then edited by hand only to remove lines: `gaps.json` holds the 34 Gap cells, `unwired.json` the 72 Native and Fallback cells (none is wired yet).

- [ ] **Step 1: Write the failing tests** in `registry_tests.rs`:
  - `every_gap_is_listed_and_every_listed_cell_is_a_gap`: the set of `entry()` of Gap cells equals `entries(GAPS)`.
  - `unwired_lists_exactly_the_native_and_fallback_cells_not_wired`: the set of Native or Fallback cells with `wired == false` equals `entries(UNWIRED)`; no wired cell is listed; no Gap or NotApplicable cell is listed.
  - `the_files_are_sorted_unique_and_well_formed`: both parse, are sorted, have no duplicate, every entry names a known tool and capability, and the text ends with a newline.
  - `a_new_tool_starts_with_no_gaps`: for every tool not in `FOUNDING`, no cell is Gap and no entry names it in either file (empty today; it fires when a fourth `Tool` is added, decision 14.2).
  - `wired_cells_are_native_or_fallback`: every capability a provider lists in `wired()` is declared Native or Fallback for it.
- [ ] **Step 2: Run** `cargo test -p notchling-engine contract::registry_tests`; expected FAIL.
- [ ] **Step 3: Implement** `registry.rs` and `baseline.rs`, then write the two files once with a throwaway test-only helper or by hand from `Registry::standard().cells()`; check by eye: `gaps.json` has 34 lines of entries (Claude Code 4, Codex 14, Antigravity 16), `unwired.json` 72 (32 + 2 Claude Code, 18 + 1 Codex, 19 Antigravity).
- [ ] **Step 4: Run** `cargo test -p notchling-engine contract`. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/engine/src/contract` then `git commit -m "contract: the registry, gaps.json (34) and unwired.json (72)"`.

### Task 0.4: The conformance suite

**Files:**
- Create: `crates/engine/tests/conformance/main.rs`, `crates/engine/tests/conformance/run.rs`
- Modify: `crates/engine/src/contract/mod.rs` (a `pub mod conformance` with `pub enum Checked { Passed, NotWritten }` and `pub fn check(registry: &Registry, tool: Tool, c: Capability, root: &Path) -> Checked`, which answers `NotWritten` for every capability in this phase)

**Interfaces:**
- Consumes: `Registry`, `Support`.
- Produces: `conformance!(claude, Tool::Claude, "claude"); conformance!(codex, Tool::Codex, "codex"); conformance!(antigravity, Tool::Antigravity, "antigravity");` in `main.rs`, each expanding to a module with one `#[test]` per capability named by its snake id (`session_list`, …, `search_text`), every one calling `run::cell(Tool, Capability, root)` where `root = <manifest>/tests/fixtures/conformance/<tool>/` (the folders arrive with the first wired cell, Phase 1). `run::cell` does: Native or Fallback and wired: `check` must be `Passed`, `NotWritten` fails with `"<entry> is wired but has no conformance check"`; Native or Fallback and not wired: prints `skipped (unwired): <entry>`; NotApplicable or Gap: asserts the registry returns that declaration (ports that answer `Unsupported` are checked from the phase that adds each port). A `summary` test prints `conformance: wired W, unwired U, not applicable N, gaps G`. Later phases put each area's checks in `crates/engine/src/contract/conformance_<area>.rs` (src, not `tests/`), so adding a check edits no test file (call C7).

- [ ] **Step 1: Write the suite** above, with the capability list for the macro written once in `run.rs` as `macro_rules! each_capability` over `(snake_name, Capability::Variant)` pairs.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --test conformance -- --nocapture`. Expected: 117 tests pass, 72 lines `skipped (unwired)`, the summary `wired 0, unwired 72, not applicable 11, gaps 34`.
- [ ] **Step 3: Prove the failure path.** Temporarily add `Capability::SubagentList` to `ClaudeProvider::WIRED` and remove `claude subagent.list` from `unwired.json`; `claude::subagent_list` must fail with "is wired but has no conformance check". Revert.
- [ ] **Step 4: Commit** `git add crates/engine/tests/conformance crates/engine/src/contract/mod.rs` then `git commit -m "contract: the conformance suite, wired cells only"`.

### Task 0.5: `docs/provider-matrix.md` and `notchling providers`

**Files:**
- Create: `crates/engine/src/contract/matrix.rs`, `crates/engine/tests/provider_matrix.rs`, `docs/provider-matrix.md` (generated), `crates/notchlingd/src/providers_cli.rs`
- Modify: `crates/notchlingd/src/cli.rs` (`Command::Providers { json: bool }`, parse `providers [--json]`, a usage line `notchling providers [--json]`), `crates/notchlingd/src/lib.rs` (dispatch)
- Test: `crates/notchlingd/src/providers_cli_tests.rs`; a new test in `cli.rs`'s test module

**Interfaces:**
- Produces: `contract::matrix::markdown(registry: &Registry) -> String` (title `# Provider matrix`, the line `Generated from crates/engine/src/contract by the provider_matrix test; NOTCHLING_UPDATE_GOLDEN=1 rewrites it.`, a counts table `Tool | Native | Fallback | Not applicable | Gap | Unwired`, then per `Group` a `##` heading and a table `Capability | Claude Code | Codex | Antigravity` whose cells read `N: <how>`, `F (<flow id>): <why>`, `NA: <why>`, `GAP: <plan>` plus ` (meanwhile <flow id>)`, each Native or Fallback cell not wired suffixed ` · unwired`); `contract::matrix::json(registry: &Registry) -> serde_json::Value` (`{"tools": [names], "cells": [{"tool", "capability", "support", "line", "flow", "wired"}]}`, arrays in `ALL` order). `providers_cli::run(json: bool, out: &mut dyn std::io::Write) -> i32` prints the Markdown or the JSON (pretty) and returns 0; it needs no daemon.

- [ ] **Step 1: Write the failing tests:**
  - `crates/engine/tests/provider_matrix.rs`, `the_matrix_matches_the_golden`: compares `../../docs/provider-matrix.md` (from `CARGO_MANIFEST_DIR`) with `markdown(&Registry::standard())`; with `NOTCHLING_UPDATE_GOLDEN=1` writes it and passes. `every_cell_appears_once`: each capability id appears in exactly one table row.
  - `providers_cli_tests.rs`: `the_text_is_the_matrix` (output equals `markdown`), `the_json_has_117_cells_and_says_which_are_wired` (`cells.len() == 117`, every cell has the six keys).
  - In `cli.rs`: `providers_parses_and_shows_json_in_the_usage` (`p(&["providers"])` is `Command::Providers { json: false }`, `--json` sets it, a usage line starts with `notchling providers ` and contains `[--json]`).
- [ ] **Step 2: Run** `cargo test -p notchling-engine --test provider_matrix` and `cargo test -p notchling-notchlingd providers`; expected FAIL.
- [ ] **Step 3: Implement**, then generate: `NOTCHLING_UPDATE_GOLDEN=1 cargo test -p notchling-engine --test provider_matrix`. Read the file: 39 rows, 34 `GAP:`, 11 `NA:`, the counts table says Claude Code 32/2/1/4/34, Codex 18/1/6/14/19, Antigravity 19/0/4/16/19.
- [ ] **Step 4: Run** the tests without the variable, and `target/debug/notchling providers --json | python3 -I -m json.tool > /dev/null`. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/engine/src/contract/matrix.rs crates/engine/src/contract/mod.rs crates/engine/tests/provider_matrix.rs docs/provider-matrix.md crates/notchlingd/src` then `git commit -m "contract: the generated provider matrix and notchling providers"`.

### Task 0.6: `GUARD-PROVIDER-GAPS`

**Files:**
- Modify: `tools/guard/guard/gate.py` (a `provider_gap_reasons(repo, base, head)` beside `baseline_reasons`, added to the reasons the gate collects), `tools/guard/guard/rules.py` (a probe `_guard_provider_gaps` registered under `GUARD-PROVIDER-GAPS`), `docs/rules.md` (a row in "The guard itself": `GUARD-PROVIDER-GAPS` | `crates/engine/src/contract/gaps.json` and `unwired.json` only shrink | A new gap or an unwired cell is a capability missing without anyone deciding it | `python3 -m guard gate` (a PR where either grew needs the maintainer))
- Test: `tools/guard/tests/test_provider_gaps.py` (new)

**Interfaces:**
- Produces: `PROVIDER_FILES = ("crates/engine/src/contract/gaps.json", "crates/engine/src/contract/unwired.json")`; `provider_gap_reasons(repo: Path, base: str, head: str) -> List[str]`, reusing `_entries` and `_show`; a reason reads `GUARD-PROVIDER-GAPS: <path> grew by <n> entries, it can only shrink: <first three>`; a file missing at the head is no reason; unreadable at the head is a reason (call C8 moves this rule from the spec's phase 6 to here).

- [ ] **Step 1: Write the failing tests** (unittest, temp repos as `test_gate.py` builds them): `test_shrinking_is_fine`, `test_growing_gaps_is_a_reason`, `test_growing_unwired_is_a_reason`, `test_new_file_counts_as_growth` (base without the file, head with two entries: one reason naming 2 entries), `test_unreadable_head_is_a_reason`.
- [ ] **Step 2: Run** `PYTHONPATH=tools/guard python3 -m unittest discover -s tools/guard/tests -t tools/guard -p test_provider_gaps.py -v`; expected FAIL.
- [ ] **Step 3: Implement**; the probe builds a two-commit repo where `gaps.json` grows and asserts the reason, as `_guard_baseline` does.
- [ ] **Step 4: Run** `tools/test-python.sh`, `PYTHONPATH=tools/guard python3 -m guard rules`. Expected: PASS, and the new rule's probe fires.
- [ ] **Step 5: Commit** `git add tools/guard docs/rules.md` then `git commit -m "guard: GUARD-PROVIDER-GAPS, the two contract files only shrink"`.

### Task 0.7: Settings > Agents, "What Notchling Can Do"

**Files:**
- Create: `crates/app/src/sheets/capabilities.rs` (pure), `crates/app/src/sheets/capabilities_view.rs` (macOS)
- Modify: `crates/app/src/sheets/mod.rs`, `crates/app/src/sheets/agents_view.rs` (the Agents page draws the part after the tool sections), `crates/app/src/sheets/renders.rs` (a `ViewSpec` `settings-capabilities`), `crates/app/src/mac/views.rs` (its arm)
- Test: `crates/app/src/sheets/capabilities_tests.rs`

**Interfaces:**
- Consumes: `contract::Registry::standard()`, `Support::kind`, `Support::line`, `Capability::title`.
- Produces: `pub struct CapabilityRow { pub capability: &'static str, pub state: RowState, pub line: &'static str }`, `pub enum RowState { Fallback, NotApplicable, Gap }`, `pub fn rows(tool: Tool) -> Vec<CapabilityRow>` (every non-Native cell of that tool, in `Capability::ALL` order); `pub fn parts() -> Vec<(Tool, Vec<CapabilityRow>)>` for the three tools. The part is page-level, titled `What Notchling Can Do`, one block per tool (its title, then each row: the capability's title, a state word `Notchling's own` / `Not applicable` / `Not built yet`, the line as the row's detail), tagged `.part("What Notchling Can Do", …)`. No hover or size changes elsewhere on the page.

- [ ] **Step 1: Write the failing tests** in `capabilities_tests.rs`: `native_cells_are_left_out` (no row for a Native cell; Claude Code shows 7 rows today: 2 Fallback, 1 NotApplicable, 4 Gap, computed from the registry, not pinned: assert `rows(t).len()` equals the registry's non-Native count for `t`), `every_row_says_why` (line non-empty), `the_order_is_the_capability_order`.
- [ ] **Step 2: Run** `cargo test -p notchling-app sheets::capabilities`; expected FAIL.
- [ ] **Step 3: Implement** the model and the view with kit tokens only (`TypeRole`, `Ink`, `metric`), words through the locale rule in force (call C25).
- [ ] **Step 4: Run** the tests, `cargo clippy -p notchling-app --all-targets -- -D warnings` on the Mac, and look: `cargo build --release -p notchling-app && target/release/notchling-app --render settings-capabilities --out /tmp/caps.png` (light and dark with `NOTCHLING_RENDER_APPEARANCE`).
- [ ] **Step 5: Commit** `git add crates/app/src/sheets crates/app/src/mac/views.rs` then `git commit -m "settings: What Notchling Can Do, each tool's fallbacks, gaps and not applicable rows"`.

### Task 0.8: Decision record, notes, the add-provider skill, the feature map

**Files:**
- Create: `docs/decisions/0028-provider-capability-contract.md` (0028 is a placeholder; the number is assigned at merge, call C5)
- Modify: `docs/decisions/README.md` (index row), `.claude/skills/add-provider/SKILL.md` (rewritten: spec 11.6's seven steps), `crates/AGENTS.md` (Architecture in one pass: a **Contract** bullet; Crates table unchanged), `crates/engine/AGENTS.md` (`contract/`: what a change could break, by file), `docs/feature-map.md` (a row "Provider contract": `crates/engine/src/contract/`, `docs/provider-matrix.md`, `crates/notchlingd/src/providers_cli.rs`, `crates/app/src/sheets/capabilities.rs`; tests `contract/*_tests.rs`, `tests/conformance/`, `tests/provider_matrix.rs`; zone protected)

- [ ] **Step 1: Write the failing check.** `PYTHONPATH=tools/guard python3 -m guard claudemd` fails naming `crates/engine/src/contract/` as unmapped once Task 0.1 is on the branch.
- [ ] **Step 2: Write** the decision (Context, Decision, Consequences, Status Proposed, Date 2026-11-14) in the format of `docs/decisions/0020-agents-change-settings-by-class.md`: it widens 0016 from six traits to 39 capabilities with four states, the two shrink-only files and their starting numbers, the conformance suite, the generated matrix, the wire name `antigravity`, the six phases. `crates/engine/AGENTS.md` notes: `support.rs` (never a wildcard arm; a line is a tooltip), `gaps.json`/`unwired.json` (only remove lines; the guard refuses growth), `WIRED` (a cell joins it in the PR that routes it and leaves `unwired.json` in the same PR), `matrix.rs` (regenerate the golden), `tool.rs` (`agy` is read, never written).
- [ ] **Step 3: Write** the skill: the spec's 11.6 steps, naming `contract/<tool>/support.rs`, `WIRED`, `tests/fixtures/conformance/<tool>/`, the macro line, `NOTCHLING_UPDATE_GOLDEN=1 cargo test -p notchling-engine --test provider_matrix`, and "no Gap for a new tool".
- [ ] **Step 4: Run** `PYTHONPATH=tools/guard python3 -m guard claudemd`, `python3 tools/strip_comments.py --check`. Expected: PASS.
- [ ] **Step 5: Commit** `git add docs/decisions .claude/skills/add-provider crates/AGENTS.md crates/engine/AGENTS.md docs/feature-map.md` then `git commit -m "docs: decision (provider capability contract), notes, add-provider skill"`.

## Checkpoint P0 (before the pull request)

- `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets -- -D warnings` (on the Mac too, for Task 0.7), `cargo fmt --all --check`, `cargo deny check`.
- `cargo test -p notchling-engine --test conformance -- --nocapture` prints `wired 0, unwired 72, not applicable 11, gaps 34`.
- `tools/test-python.sh`; `PYTHONPATH=tools/guard python3 -m guard lint` (0 new), `rules`, `claudemd`, `codeowners --check`.
- No surface calls the registry yet: Chat, the notch, the phone and the daemon behave as before. The pull request says so, gives the two starting numbers, and names the maintainer-only items above.
