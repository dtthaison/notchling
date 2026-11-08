# Milestone 3: The daemon acts (built and switched off)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** the daemon and the app can do everything the Swift app does to a session: reply, answer questions and permissions, stop, interrupt, remove, start, trust a folder, move it between accounts, and run commands. The hook runs under the daemon. Everything is built and tested against fixtures and fakes, but it ships **switched off**: the Swift app owns the hook and every live session until the maintainer records the reply fixtures and turns acting on.

**Spec:** `docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md`: Parts E3, E4, E11, U2, U7, U8; Milestone 3; the "Swift and Rust side by side" table; "The hook under a daemon". Decisions 0004, 0007, 0011. The recording phase is in `docs/recording.md`.

## Global Constraints

### Off by default

The daemon setting `acting` (in its settings store, read at start) defaults to `off`.

- While it's off, every acting op answers `forbidden` with the message `acting is off: the Swift app owns sessions until the maintainer turns this on`.
- While it's off, the hook mode always answers pass.
- Turning it on is one app-tier op, `settings.set_acting`, plus a line in `docs/dev-profile.md`. Nothing in this milestone turns it on.

### No live sessions in tests

- No test attaches to, types into or starts a real Claude Code session.
- No test runs `claude`, `codex` or `agy`.
- Process launching goes through a `Launcher` trait. Tests use a recording fake.
- A pty test may spawn `/bin/sh` or `/bin/cat` in a temp folder.
- The live tests in `crates/engine/tests/live_reply.rs` stay env-gated (`NOTCHLING_LIVE=1`), as in M1.

### No real config

- No writes to `~/.claude/settings.json`, `~/.claude.json`, Keychain or `~/Library/LaunchAgents`.
- Trusting a folder writes `~/.claude.json` only through the real `claude` trust prompt, and only when acting is on. That matches what the Swift app does (`ReplySender.trustFolder`).

### Faithful to the Swift app

Reply behavior follows `legacy/macos/Sources/AGENTS.md`, "Things that bit before", exactly:

- The clear keys are Ctrl+E, Ctrl+U, then Backspace once per line. Never Ctrl+K, Esc or Ctrl+C.
- There are pauses between the clear keys, the paste and Enter.
- Text is sent as a bracketed paste.
- Use `forkpty`.
- Never answer device-attribute queries.
- Drop `NO_COLOR` from the child's environment.
- Check for a dialog before typing (`Failure.dialog`).
- `takeBackQueued` works by clearing, pressing Up, and reading `popAll` from the transcript.
- Send Esc only while the session is working.
- Answers are digits. A multi-select page takes Tab; the last page ends on the review screen and `1`.

Where a behavior has no recording yet, implement it from the Swift code. List it in `docs/recording.md` as "ported from code, recording pending".

### Daemon ops

All of these are **app tier** and audit-logged:

- `session.reply {id, text}`
- `session.answer {id, answers: [[int]|string]}`
- `session.permission {id, decision: "allow"|"deny"|"always"}`
- `session.interrupt {id}`
- `session.stop {id}`
- `session.remove {id}`
- `session.take_back {id}`
- `session.new {folder, config_dir?, flags?}`
- `folder.trust {folder}`
- `session.move {id, to_config_dir}`
- `session.command {id, command}`
- `settings.set_acting {on}`

Also:

- Read ops are agent tier: `session.question {id}` (the full AskUserQuestion read from the screen) and `git.status {folder}`.
- Hook ops come from the hook client: `hook.request {...}`, answered with the decision or `pass`.
- Events: `prompt.pending` and `prompt.resolved` (questions and permissions), so the app and later the phone share one answered state.

### The hook

`notchling hook` reads Claude Code's hook JSON on stdin and writes the answer on stdout. Its rules:

- It passes at once when the daemon is unreachable.
- It passes when no client that can answer (the app or a phone) is attached.
- It passes on any socket error.
- It passes when the session is a `claude -p` (`sdk-cli`) session.
- It keeps its own deadline, under the hook's 300 s timeout, and passes when the deadline is reached.

Each of these rules has a fixture test (decision 0004). The hook is not installed anywhere in this milestone; installing it is E13, in M6.

### Lanes

- Commands for a session in a lane run with that lane's `CLAUDE_CONFIG_DIR`, found the way `AgentMonitor.configDir(forJob:)` finds it.
- The lane Keychain names are not touched; accounts arrive in M6.

### The app

- U2 completes the notch: answering cards, Peek, Tab Perch and the pet.
- U7 adds the banners and the Trust Folder pop-up; the vault approval comes in M6.
- U8 adds the New Session and Settings sheets; Add Account and Second Opinion come with M5 and M6.
- When acting is off, every action control is shown disabled, with the off message as its tooltip.
- Each part is rendered with `--render`.

### Code rules

- Files are under 600 lines.
- No unwrap or expect outside tests.
- Every dependency passes `cargo deny`.
- Every crate type-checks for Linux and Windows. On non-macOS, a pty that isn't built yet returns `Unsupported`.

## Tasks

1. **E3a, terminal.** Lives in `crates/engine/src/terminal/` and `crates/ptyhost`.
   - Port `TerminalScreen` and test it against every `Fixtures/terminal/*.bin`, porting `TerminalScreenTests` and `AskDialog` with its tests.
   - A pty session using `forkpty` that drops `NO_COLOR` and stays silent on DA queries. Test it with `/bin/cat`.
   - `crates/ptyhost`: a port of `Sources/hostd` with the same socket contract. It is built but not installed (the Swift hostd keeps the LaunchAgent).
2. **E3b and E4, replies and the hook.**
   - `engine::reply`: pure key-sequence planners for reply, clear, answer, take-back and interrupt, with timing given as data. Test them against the Swift constants and against `RecordReplyFixturesTests`' pinned sequences.
   - An executor that plays a plan into a pty with its pauses. Test the executor with a fake pty.
   - The daemon ops above, with the `acting` switch and `prompt.*` events.
   - `notchling hook`, with its fixture tests.
3. **E11, session control.** Lives in `engine::control`.
   - Pure planners, each producing argv, env, cwd and the expected outcome:
     - New Session (`claude --bg`, refusing an untrusted folder);
     - Trust Folder (Down then Enter in a hidden pty);
     - Stop, Remove (`claude rm` on every copy);
     - Move (`claude stop`, then `claude --bg --resume <transcript id>` with the target's config dir and `respawnFlags`);
     - Move All (a port of the pure `MoveAllPlan` with its tests);
     - takeover;
     - slash commands (`SlashCommands`, with `menuCommands` going to the terminal);
     - resume after wake (`WakeResumer`);
     - git status (`GitInfo.read` parsing).
   - The `Launcher` trait and a real launcher, plus a fake used in tests.
   - Port the Swift tests for each.
4. **U2, U7 and U8 in the app.**
   - The notch's answering cards (questions with tabs, permissions), Peek, Tab Perch and the pet (`PetView`'s `Walk`, with its path computed from the shape).
   - Banners, Trust Folder, and the New Session and Settings sheets.
   - Everything is wired to the daemon ops through the local-api client, and disabled while acting is off.
   - `--render` names for each.
   - Pure view models are unit-tested.
5. **Docs.**
   - `docs/recording.md` gets a status column.
   - `ARCHITECTURE.md`, the feature map and `AGENTS.md` (the `notchling hook` line).
   - Decision 0011 gets a dated note: the daemon holds the code for its M2/M3 resources, and it is switched off until the maintainer hands them over.

## Review Focus

- **Acting off.** With acting off, no code path can type into a pty, launch `claude`, or write a file outside the daemon's own data folder.
- **The hook.** It must never hang Claude Code: any error, slow daemon or missing client means pass, within its deadline.
- **Shell quoting.** A reply containing control characters, `\x1b[201~` (the end of a bracketed paste), or a lone Esc must not escape the paste.
- **A dialog that opens mid-type.** It must not get its answer from typed text.
- **The agent tier.** It can't reach any acting op, not even through `hook.request`, which must refuse decisions from non-hook clients.
