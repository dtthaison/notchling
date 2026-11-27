# Providers, Phase 5: The Other Ports, One Pull Request per Item Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Route every remaining Native and Fallback cell through the registry (decision 0016's ports: Sessions, Conversation, Turns, Prompts, Commands, Usage, Catalog, Search text, Setup), and close the phase gaps that are not designs: slash commands for Codex and Antigravity in Chat, Antigravity's Remove as `hide-row`, Antigravity's Search text from its logs, Antigravity's context windows in `catalog.json`, and Codex lanes if the maintainer wants them. At the end `unwired.json` is empty and `gaps.json` holds only the 11 design gaps (13 without Codex lanes).

**Architecture:** Each item adds one port trait to `contract/` with an accessor on `Provider` defaulting to `Unported` (call C6), implements it per tool over today's code (the code moves behind the port; it is not rewritten), routes the surface that used to branch on the tool through the registry, adds the cells to `WIRED`, cuts them from `unwired.json`, and adds the area's conformance checks in `contract/conformance_<area>.rs` (call C7). Behavior does not change except where an item says so (5c, 5d, 5e, 5f, 5g).

**Tech Stack:** Rust 1.99.1, the existing seams (`Launcher`, `ToolSpawner`, ACP `Spawner`, pty fakes, `HttpClient`, `Keychain`), GPUI through the kit for 5c and 5d.

**Spec:** sections 6.3, 6.5, 6.6, 7.1, 7.2, 11.5, 12 (phase 5), decision 0016. Phases 0 to 4 merged (5b needs Phase 4's held queue in place). Call C22 (one pull request per item) in the overview.

**Branches:** one per item, `agent/providers-p5a-sessions`, `-p5b-turns`, `-p5c-slash`, `-p5d-hide-row`, `-p5e-search-text`, `-p5f-catalog`, `-p5g-codex-lanes`, `-p5h-usage`, `-p5i-setup`.

## Global Constraints

- Phase 0 to 4 constraints hold. Each item's pull request leaves the app working and both JSON files no larger.
- A port method takes its roots as parameters and keeps effects behind today's seams; its conformance check runs over fixtures and fakes only (`FakeLauncher`, `FakeSpawner`, pty fakes, `MemoryKeychain`, `ScriptedHttp`). No test starts a real tool.
- Moving code behind a port keeps its tests where they are; new tests are the conformance checks and, for 5c to 5g, the behavior tests named in each item.
- A surface stops branching on the tool when its port lands: no new `Tool::` match outside `contract/`, the account providers and `Tool::of_row` (Phase 6 turns this into `GUARD-TOOL-BRANCH`).

## Review Focus

1. 5a: Stop, Remove, Move and Trust Folder answer exactly as before for every tool (same ops, same refusals): the port is a move, not a rewrite. (5a conformance checks `session_stop`, `session_remove` per tool over `FakeLauncher` and the ACP fake.)
2. 5c: a Codex slash command that Codex runs as a prompt is sent as text, one that Notchling must refuse says why; Chat never shows a command the tool's version does not have. (5c test `codex_commands_follow_its_version`.)
3. 5d: Remove on an Antigravity row hides it in Notchling and touches no Antigravity file; Search still finds it. (5d test `remove_hides_and_writes_nothing_of_antigravitys`.)
4. 5e: Search over Antigravity logs indexes only the user's and the model's words, never tool output over the parser's cap, and a child conversation files under its parent (Phase 1). (5e tests `antigravity_text_is_indexed_without_tool_output`, `an_antigravity_child_files_under_its_parent`.)
5. 5g, if built: a lane's `CODEX_HOME` never points two accounts at one folder, and a switch never writes while Codex runs. (5g test `a_switch_waits_for_codex_to_stop`.)

## Order and lanes

```
5a sessions ─► 5b turns, prompts, commands ─► 5c slash commands in Chat
5a ─► 5d hide-row        5a ─► 5g Codex lanes (only if wanted)
5e search text   5f catalog   5h usage and accounts   (any time after Phase 0)
5i setup         (after decision 0019 lands)
```

Each item is its own pull request; 5e, 5f and 5h can run beside 5a.

## Zones and gates

| Item | Zone | Gates expected | Existing tests edited |
| --- | --- | --- | --- |
| 5a | Maintainer (`contract/**`, `control/**`, `notchlingd/**`); auto (`app/**`, `transcript/**`) | none new (the spawns already exist; their `CAP-PROCESS` lines move, so `guard lint` sees changed lines: the maintainer gate reads them as moves) | none |
| 5b | Maintainer (`reply/**`, `terminal/**`, `tools/**`, `notchlingd/**`) | as 5a | none |
| 5c | Auto (`app/**`); maintainer (`tools/codex/commands.rs`, `tools/agy/live.rs`) | none | in-crate, listed: `app/src/chat/view/slash_menu.rs` tests if they pin the tool list |
| 5d | Auto (`app/**`) | none | none |
| 5e | Auto (`search/**`); maintainer (`tools/agy/**`) | none | none; if hybrid search P5 T6 has merged, `read_with_lines(meta, tool)` is the entry point to extend |
| 5f | Maintainer (`catalog/**`, `crates/engine/data/catalog.json`) | none | none |
| 5g | Maintainer (`lanes/**`, `accounts/**`, `notchlingd/**`) | `CAP-FILEWRITE` (a lane folder per account) | none |
| 5h | Maintainer (`accounts/**`, `providers/**`, `limits/**`, `cost/**`) | none new | none |
| 5i | Maintainer (`install/**` or 0019's `instructions/**`) | none new | none |

## Maintainer only (Phase 5)

- Whether Codex lanes are wanted at all (5g); if not, the two cells (`session.move`, `account.lanes`) stay Gap, or become NotApplicable by the maintainer's decision, and `gaps.json` ends at 13.
- Every item but 5c, 5d and 5e's search part is in the maintainer's zone.
- 5f edits `catalog.json` (data the app ships).

---

### Item 5a: Sessions and Conversation ports (`agent/providers-p5a-sessions`)

**Files:**
- Modify: `crates/engine/src/contract/sessions.rs` (Phase 2's port gains `status`, `new`, `stop`, `remove`, `move_to`, `trust`), new `crates/engine/src/contract/conversation.rs` (`read`, `follow`, `context`), `contract/{claude,codex,antigravity}/` (an implementation file per port, over `control::*`, `tools::codex`, `tools::agy`, `transcript::*`), `crates/notchlingd/src/control_ops.rs`, `codex_ops.rs`, `agy_ops.rs` (the `session.*` ops call the registry by id), `crates/app/src/chat/data.rs` (`sessions_from` and `conversation_shared` through the registry), `crates/app/src/chat/view/tool/antigravity.rs` (Trust Folder offered for an Antigravity session: the surface gap of `folder.trust`), `contract/*/mod.rs` (`WIRED` +22), `unwired.json` (−22), `contract/conformance_sessions.rs`, `contract/conformance_conversation.rs`
- Test: the conformance checks; `crates/app/src/chat/trust_tests.rs` (new): `an_antigravity_session_offers_trust_folder`

**Interfaces:**
- `pub trait Sessions { fn list(..); fn status(&self, id: &str, roots: &Roots) -> Result<SessionStatus, Unsupported>; fn new(&self, folder: &Path, pick: &Pick, deps: &ActDeps) -> Result<String, Unsupported>; fn stop(&self, id: &str, deps: &ActDeps) -> Result<(), Unsupported>; fn remove(&self, id: &str, deps: &ActDeps) -> Result<(), Unsupported>; fn move_to(&self, id: &str, account: &str, deps: &ActDeps) -> Result<(), Unsupported>; fn trust(&self, folder: &Path, deps: &ActDeps) -> Result<(), Unsupported>; }`; `ActDeps` bundles the seams (`Launcher`, Codex `ToolSpawner`, ACP `Spawner`, the acting switch).
- `pub trait Conversation { fn read(&self, s: &SessionRef) -> Result<Arc<transcript::Conversation>, Unsupported>; fn follow(&self, s: &SessionRef) -> Result<Vec<PathBuf>, Unsupported>; fn context(&self, s: &SessionRef) -> Result<Option<Context>, Unsupported>; }` (`follow` names the files to watch; the watching stays in the app and the daemon).
- Cells wired: Claude Code 9 (`session.status`, `.new`, `.stop`, `.remove`, `.move`, `folder.trust`, `conversation.read`, `.follow`, `.context`), Codex 6 (`session.status`, `.new`, `.remove`, `conversation.*`), Antigravity 7 (`session.status`, `.new`, `.stop`, `folder.trust`, `conversation.*`).

- [ ] **Step 1:** write the conformance checks (they fail: cells not wired, `check` answers `NotWritten`). **Step 2:** run `cargo test -p notchling-engine --test conformance`; with the cells added to `WIRED` first, expected FAIL. **Step 3:** implement the ports by moving code; route the ops and the app. **Step 4:** run `cargo test --workspace`, the e2e tests, `bench_chat` on the Mac. Expected: PASS; summary `unwired 33`. **Step 5:** regenerate the matrix; commit `git commit -m "contract: sessions and conversations through the registry"`.

### Item 5b: Turns, Prompts and Commands ports (`agent/providers-p5b-turns`)

**Files:**
- Create: `crates/engine/src/contract/turns.rs` (`send`, `interrupt`, `options`; `hold` and `take_back` answer `Unsupported` and the registry answers `HeldQueue`, wired in Phase 4), `contract/prompts.rs` (`question`, `permission`, `needs_you`), `contract/commands.rs` (`list`, `run`)
- Modify: the per-tool implementations over `reply::*`, `terminal::ask_dialog`, `terminal::permission_dialog`, the hook (`hook_ops.rs`), `control/slash.rs`, `tools/codex/runs.rs`, `tools/agy/runner.rs`; `crates/notchlingd/src/acting_ops.rs`, `codex_ops.rs`, `agy_ops.rs`; `crates/app/src/chat/view/tool/*.rs` (the adapters present the registry's answers, decision 0015); `contract/*/mod.rs` (`WIRED` +15), `unwired.json` (−15), `contract/conformance_turns.rs`, `conformance_prompts.rs`, `conformance_commands.rs`
- Cells wired: `turn.send`, `turn.interrupt`, `turn.options` for all three; Claude Code `prompt.question`, `prompt.permission`, `signal.needs_you`, `command.slash`; Antigravity `prompt.permission`, `signal.needs_you`.

- [ ] **Steps 1 to 5** as 5a; expected summary `unwired 18`; commit `git commit -m "contract: turns, prompts and commands through the registry"`.

### Item 5c: Slash commands for Codex and Antigravity in Chat (`agent/providers-p5c-slash`)

**Files:**
- Modify: `contract/codex/commands.rs` (over `tools::codex::commands::available(codex_home)` and `route`), `contract/antigravity/commands.rs` (the last `available_commands_update` the ACP session sent, kept by `tools/agy/live.rs`), `crates/app/src/chat/view/slash_menu.rs` (the menu lists the registry's commands for the selected session's tool), `contract/codex/support.rs` and `contract/antigravity/support.rs` (`command.slash` Native), `WIRED`, `gaps.json` (−2)
- Test: `crates/app/src/chat/view/slash_tools_tests.rs` (new): `codex_commands_follow_its_version`, `antigravity_commands_come_from_its_session`, `a_command_codex_runs_as_a_prompt_is_sent_as_text`, `an_unknown_command_is_sent_as_text`

- [ ] **Steps 1 to 5:** failing tests, run (FAIL), implement, run `cargo test -p notchling-app -p notchling-engine` and render `--render chat` with the menu open over a Codex fixture (PASS), commit `git commit -m "chat: Codex's and Antigravity's slash commands"`.

### Item 5d: Antigravity's Remove as `hide-row` (`agent/providers-p5d-hide-row`)

**Files:**
- Create: `crates/engine/src/contract/flows/hide_row.rs` (the app's hidden list, `hiddenAgents`, as a flow)
- Modify: `contract/antigravity/support.rs` (`session.remove` Fallback `HideRow`: `agy has no delete; Notchling hides the row and keeps the data`), `WIRED`, `gaps.json` (−1), `crates/app/src/chat/view/row_menu.rs` (Remove on an Antigravity row says `Hide` with the line as its note), `contract/conformance_sessions.rs`
- Test: `crates/app/src/chat/hide_row_tests.rs` (new): `remove_hides_and_writes_nothing_of_antigravitys` (a fixture root's `~/.gemini` files unchanged byte for byte), `a_hidden_row_is_still_found_by_search`, `opening_a_hidden_row_from_search_shows_it_again`

- [ ] **Steps 1 to 5** as 5c; commit `git commit -m "antigravity: Remove hides the row"`.

### Item 5e: Search text through the registry; Antigravity's from its logs (`agent/providers-p5e-search-text`)

**Files:**
- Create: `crates/engine/src/contract/text.rs` (`SessionText::read(&self, s: &SessionRef, budget: u64) -> Result<Vec<Line>, Unsupported>`), `crates/engine/src/search/index/agy_read.rs` (log lines: user requests and planner responses, tool output skipped past the parser's cap, from `elsewhere`'s line reader)
- Modify: `crates/engine/src/search/index/read.rs` (Antigravity no longer `Read1::Empty`; all three through the registry's `text`), `contract/antigravity/support.rs` (`search.text` Native: `its brain logs`), `WIRED` (+3: Claude Code and Codex were Native and unwired), `gaps.json` (−1), `unwired.json` (−2), `contract/conformance_search.rs`
- Test: `crates/engine/src/search/index/agy_read_tests.rs` (new): `antigravity_text_is_indexed_without_tool_output`, `an_antigravity_child_files_under_its_parent`, `a_log_over_the_budget_reads_its_tail`

- [ ] **Steps 1 to 5** as 5c; run `cargo test -p notchling-engine search`; commit `git commit -m "search: Antigravity's text from its logs, every tool through the registry"`.

### Item 5f: Catalog through the registry; Antigravity's context windows (`agent/providers-p5f-catalog`)

**Files:**
- Create: `crates/engine/src/contract/catalog.rs` (`CatalogSource::models(&self, roots) -> Result<Vec<Model>, Unsupported>`)
- Modify: `crates/engine/data/catalog.json` (the Gemini models Antigravity offers, with context windows; numbers from Google's published model pages, cited in the pull request), `crates/engine/src/catalog/mod.rs` (rows with no price allowed for a window-only entry, if the format needs it), `contract/antigravity/support.rs` (`catalog.models` Native: `configOptions, cached; windows from catalog.json`), `WIRED` (+3), `gaps.json` (−1), `unwired.json` (−2), `contract/conformance_catalog.rs`
- Test: `crates/engine/src/catalog/windows_tests.rs` (new): `every_antigravity_model_has_a_window`, `the_context_ring_uses_it` (Antigravity's `conversation.context` gets a window)

- [ ] **Steps 1 to 5** as 5c; commit `git commit -m "catalog: Antigravity's context windows; every tool's models through the registry"`.

### Item 5g: Codex lanes, `CODEX_HOME` per account (`agent/providers-p5g-codex-lanes`, only if the maintainer wants it)

**Files:**
- Modify: `crates/engine/src/lanes/` (a Codex lane: its own `CODEX_HOME` under `<data>/lanes/codex/<account>/`, linking the shared files as Claude Code lanes do), `crates/engine/src/accounts/` (Codex's switch per lane), `crates/notchlingd/src/codex_ops.rs` (a turn runs with its session's lane), `contract/codex/support.rs` (`session.move`, `account.lanes` Native), `WIRED`, `gaps.json` (−2)
- Test: `crates/engine/src/lanes/codex_tests.rs` (new): `a_lane_has_its_own_codex_home`, `two_accounts_never_share_one`, `a_switch_waits_for_codex_to_stop`, `move_takes_the_rollout_to_the_lane`

- [ ] **Step 0:** ask the maintainer (an accounts decision, outward-facing per AGENTS.md). If no, close the item and record the decision; the cells stay Gap, or become NotApplicable by that decision.
- [ ] **Steps 1 to 5** as 5c; commit `git commit -m "codex: lanes with their own CODEX_HOME"`.

### Item 5h: Accounts and usage through the registry (`agent/providers-p5h-usage`)

**Files:**
- Create: `crates/engine/src/contract/usage.rs` (`Usage<'a> { account: &'a dyn AccountProvider, limits: Arc<dyn ProviderSource>, cost: CostParser, lanes: Option<&'a dyn Lanes> }` per spec 6.3)
- Modify: the three providers return their `AccountProvider` (`providers/mod.rs`), `ProviderSource` (`limits/sources.rs`) and cost parser (`cost/other.rs`, `cost/claude*`); the daemon's accounts, limits and cost `OpGroup`s look them up through the registry; `WIRED` (+10: `account.switch` ×3, Claude Code `account.lanes`, `usage.limits` ×3, `usage.cost` ×3), `unwired.json` (−10), `contract/conformance_usage.rs` (over `MemoryKeychain`, `ScriptedHttp`, `FakeHttp`)

- [ ] **Steps 1 to 5** as 5a; summary `unwired 4`; commit `git commit -m "contract: accounts, limits and cost through the registry"`.

### Item 5i: Setup through the registry (`agent/providers-p5i-setup`, after decision 0019)

**Files:**
- Modify: `contract/*/mod.rs` (`fn setup(&self) -> &dyn instructions::Target` once 0019's `instructions` module exists; before it, over `install::skills` and `install::hooks`), `WIRED` (+4: `setup.skills` ×3, Claude Code `setup.hooks`), `unwired.json` (−4, empty), `contract/conformance_setup.rs` (temp roots only: never the real `~/.claude`, `~/.codex`, `~/.gemini`)
- The design gaps (`setup.rules` ×3, `setup.hooks` for Codex and Antigravity, `version.*` ×6) leave `gaps.json` in the pull requests that land decision 0019 and the version-sync plans, not here.

- [ ] **Steps 1 to 5** as 5a; summary `unwired 0`; commit `git commit -m "contract: setup through the registry; unwired.json is empty"`.

## Checkpoint P5 (after the last item)

- `unwired.json` is `{"entries": []}`; `gaps.json` holds the 11 design gaps (13 if 5g was declined).
- Every item's pull request ran `cargo test --workspace --locked`, clippy, fmt, deny, and the conformance suite.
- `notchling providers` shows no ` · unwired` cell.
