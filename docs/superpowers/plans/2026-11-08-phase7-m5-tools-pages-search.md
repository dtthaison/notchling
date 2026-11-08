# Milestone 5: Other tools, pages and search (built, acting parts switched off)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** the daemon and the app run Codex and Antigravity sessions the way the Swift app does, Delegate and second opinions work from the `notchling` binary, the Artifact library and the session index live in the daemon, and the app gets the ⌘⇧P search panel and the pages window. Anything that launches a tool or types into a session is behind the `acting` switch from M3. Anything that installs a hook or skill waits for E13 (M6).

**Spec:** `docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md`: Parts E6, E7, U3, U4; Milestone 5. Decisions 0004, 0007, 0011. The Swift sources are listed per task. Behavior notes are in `legacy/macos/Sources/AGENTS.md` (Codex sessions, Antigravity CLI sessions, ACPRunner, Search, Delegation, Artifacts).

## Global Constraints

### Acting off

These ops answer `forbidden` with M3's off message while `acting` is off:

- every op that runs `codex`, starts `agy acp`, answers an Antigravity permission, or sends SIGINT;
- every op that switches a Codex or Antigravity login.

Reads always work:

- Codex rollouts;
- the Antigravity summaries copy;
- the pages library;
- the session index.

### No live tools in tests

- No test runs `claude`, `codex` or `agy`.
- Process launching goes through M3's `Launcher` trait, or a new `ToolProcess` trait (stdin, stdout lines, kill) with a scripted fake.
- ACP is tested against `legacy/macos/Fixtures/acp-agy.jsonl`, which is a real recording.
- Codex is tested against `exec --json` lines taken from the Swift tests' fixtures.

### No real config or network

- No test reads `~/.codex`, `~/.gemini`, `~/.claude` or the real Notchling data folder. Roots are parameters.
- Owner publish never makes a network call in tests. Its HTTP client is a trait.
- Owner publish needs saved OAuth tokens, which arrive with accounts in M6. So the real token source is not wired in this milestone; it returns `Unavailable("accounts arrive in M6")`.
- Antigravity's database is never opened in place. Read a copy of the db plus its `-wal`, as `AntigravitySessions` does.

### Faithful to the Swift app

- **Codex turns.**
  - One `codex exec --json` per turn, `resume <id>` after the first, in the workspace-write sandbox.
  - `-c sandbox_workspace_write.network_access=true` while the setting `codexNetwork` is on (default on).
  - `thread.started` gives a new session's id.
  - Messages typed mid-turn queue for the next turn.
  - Stop sends SIGINT.
- **Codex slash commands** follow `CodexCommands.swift`.
- **Antigravity.**
  - One `agy acp` per opened session, closed after 10 minutes unused.
  - `session/load` replays the history.
  - Calls: `session/new`, `session/prompt`, `session/cancel` and `session/set_config_option`.
  - Request ids can be strings.
  - `initialize` offers no fs and no terminal, so other agent calls get -32601.
  - Nothing is auto-approved.
- **Delegate.**
  - Flags: `notchling delegate <codex|agy> [--model --effort --folder --out --write --session --timeout --json] -- <prompt>` and `notchling delegate-models`.
  - Same output format and exit codes (0, 1, 124) as `--delegate`.
  - Same read-only defaults and nesting limit (`NOTCHLING_DELEGATE_DEPTH`, 2).
  - agy's subagent wait follows `Delegate.agyProgress`.
  - Delegate is a client mode of the binary and does not need the daemon. It runs a tool only because the user's agent asked it to, so it is **not** behind `acting`. It never touches sessions it didn't start.
- **Pages library.** Same files as `PageLibrary`:
  - `pages.json` is written under a flock;
  - each copy is wrapped to render from disk;
  - `account` is the first publisher;
  - each publish keeps its `version`.
  - The daemon reads and writes a data folder that is a parameter. While the Swift app owns the page hook (until E13), the daemon's library is read-only over the Swift app's folder unless the folder is a test folder.
- **Session index.**
  - Re-read only changed files (mtime and size), with the same 14-day window and head+tail of big files.
  - The daemon keeps its own cache, `search-index.json`, in its own data folder, so it doesn't write the Swift app's file.

### Code

- Files under 600 lines.
- No unwrap or expect outside tests.
- `cargo deny` passes, and `deny.toml` is never edited.
- Linux and Windows type-check.
- Each new app view has a `--render` name.

## Tasks

Wave 1 runs in parallel (Tasks 1–4). Wave 2 (Tasks 5–6) starts after wave 1 merges.

### 1. E6a, Codex

Port these to `engine::tools::codex`:

- **`CodexRunner`**: argv and env planner, the `exec --json` event parser (thread.started, item events, turn.completed, errors), the turn queue and the stop.
- **`CodexCommands`**: the command list, `~/.codex/prompts/*.md` as `/prompts:<name>`, and what each command does: picker, new, delete or terminal.
- **`CodexModels`**.

Add these daemon ops, app tier, audit-logged and behind `acting`:

- `codex.send {id|draft_folder, text}`, which starts or queues a turn;
- `codex.stop {id}`;
- `codex.delete {id}`.

Add events `codex.turn {id, state}` and `session.changed` for the row.

Port the Swift tests: `CodexRunnerTests`, `CodexCommandsTests` and whatever else covers these files.

### 2. E6b, Antigravity over ACP

Port these:

- **`ACPConnection`**: JSON-RPC 2.0 lines, string or integer ids, -32601 for unknown agent calls.
- **`ACPRunner` / `AntigravityRunner`**: one process per session, the 10-minute idle close, load, new, prompt, cancel, set_config_option and request_permission.
- **`AntigravityCLI`**: the session list read from a copy of the summaries db.

They go to `engine::tools::acp` and `engine::tools::agy`. The transcript mapping already exists in `engine::transcript::acp`; reuse it.

Add these daemon ops, behind `acting` where they act:

- `agy.open {id}`, which starts `agy acp` and loads the session. It is acting, because it starts a process.
- `agy.send {id|draft_folder, text}`
- `agy.cancel {id}`
- `agy.set_option {id, option, value}`
- `agy.permission {id, rpc_id, option_id}`

Add events `agy.permission.pending`, `agy.permission.resolved` and `session.changed`. Pending permissions also go to the M3 `prompt.*` events, so the notch shares one answered state.

Tests use the recording and a scripted fake process.

### 3. E7, pages library and session index

Port these:

- **`PageLibrary`** to `engine::pages`: record a publish from a PostToolUse(Artifact) payload, wrap the fragment, update `pages.json` under a flock, list, find (reuse `engine::search::find_pages`), the account rule and versions.
- **`OwnerPublish`**: the pure parts (base58 id ↔ UUID, the request body, the version check that refuses a publish over a newer version unless `force`) behind an `HttpClient` trait and a `TokenSource` trait. The real `TokenSource` returns `Unavailable` (see Global Constraints).
- **`SessionIndex`** to `engine::search::index`: scan roots (every config dir's projects folder, Codex rollouts, Antigravity titles), reuse changed-file entries only, the cache file with a version, and a 14-day window.

Add daemon ops, agent tier (reads):

- `search.query {text, limit}`, which returns pages and sessions grouped;
- `pages.list`;
- `pages.find {query}`.

Add the CLI client `notchling pages [words]` and `notchling page <link|id|words>`, which print the same text as `--pages` and `--page`.

The hook handling for Artifact events (record a publish, refuse a whole-page read of a saved link, and point a failed read at the local copy) is written as pure functions with fixture tests. It is not wired into `notchling hook` until E13.

### 4. U3 and U4, the search panel and the pages window

U3, the search panel, in `crates/app`:

- **The hot key.** A global ⌘⇧P through the Carbon hot key API on macOS, behind a `HotKey` trait. Nothing on other platforms yet; say so in the code.
- **The panel.** Borderless and non-activating. It activates the app while shown and reactivates the previous app on Esc.
- **Its contents.**
  - It holds a query field, then results grouped Pages and Sessions, from `search.query`.
  - Return on a page opens the pages window.
  - Return on a session opens Chat on that session.

U4, the pages window:

- **The list.** A library list from `pages.list`, with search.
- **Opening a page.** It shows in a `wry` web view in its own window, from the local copy. Reuse the Phase 1 spike's wry-beside-GPUI code.

Wire both against the op contracts in Task 3, using a fake client in tests, so this task doesn't wait for Task 3.

Render names: `search-panel`, `search-panel-results` and `pages-window`.

### 5. Delegate and second opinions (wave 2)

Port these:

- **`Delegate` and `DelegateCLI`** as `notchling delegate` and `notchling delegate-models`, over Tasks 1 and 2's runners.
- **`SecondOpinion` and `SecondOpinionStore`** to the engine, as pure job planning and a store.
- **`DelegateSkill`'s text**, as a string. It is installed in M6, not here.

Media pickup from `~/.codex/generated_images/<session>/` and `brain/<session>/`, and `--out` copies, take roots as parameters in tests.

### 6. The app side of the tools (wave 2)

- **Chat.**
  - Codex and Antigravity rows reply through `codex.send` and `agy.send`.
  - The options picker sends `agy.set_option`.
  - The Codex model picker and the Antigravity permission card send `agy.permission`.
  - Stop sends `codex.stop` and `agy.cancel`.
- **The notch.** It shows Codex and Antigravity rows with their live state (Working, Input, Done), and the Antigravity permission card answers from the notch.
- **The Second Opinion sheet** (U8's last sheet).
- **While acting is off**, every action control is disabled and shows the off message.

Render names: `agy-permission-card`, `codex-model-picker` and `second-opinion-sheet`.

## Review Focus

- **Acting off.** No code path may start `codex` or `agy`, send SIGINT, or answer a permission while acting is off. Delegate is the named exception, because it runs outside the daemon on an agent's own request.
- **ACP framing.** Each of these must be refused without hanging the runner or other sessions:
  - a line over 1 MiB;
  - a non-JSON line;
  - a response to an unknown id;
  - a request with a string id;
  - a process that dies mid-turn.
- **Pages.** Two publishes landing at once must not lose either entry (`pages.json` is written under a flock). An artifact id given to `page` with `../` in it must not escape the pages folder.
- **Owner publish.** It must never send a token for an account other than the page's owner. It must never publish over a newer version without `force`.
- **The index.** A 2 GB transcript, a line of 200 MB, or a file deleted mid-scan must not stall or crash the scan. The cache is rebuilt when its version changes.
