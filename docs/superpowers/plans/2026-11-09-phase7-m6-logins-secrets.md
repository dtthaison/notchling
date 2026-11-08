# Milestone 6: Logins and secrets (built and switched off)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** the daemon and the app can do everything the Swift app does with accounts, limits, lanes, cost, the Secret Vault, installing hooks and skills, first run and Machines. All of it is built and tested against fakes, and all of it ships **switched off**. Each area is handed over by the maintainer, one at a time (decision 0011), and then the Swift app can retire (Phase 8, which needs the maintainer's go-ahead).

**Spec:** `docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md`: Parts E8, E9, E10, E13, U5, U9, U11; Milestone 6; "Secrets per OS"; the local API tiers. Decisions 0002, 0007, 0010, 0011. Behavior notes are in `legacy/macos/Sources/AGENTS.md`: accounts, lanes, Codex, Antigravity, Claude Desktop, limits, cost, Secret Vault, Machines and settings.json. Treat every "never" there as a requirement.

## Global Constraints

### Switched off

The daemon gets three more switches with M3's `acting` rule:

- they are off at every start, whatever any file says;
- only an app-tier `settings.set_switch {switch, on}` turns one on;
- the app re-asserts the user's choice.

| Switch | What it covers while off |
| --- | --- |
| `accounts` | Every account op that reads or writes a login (Keychain, `~/.claude.json`, `auth.json`, Antigravity's item, Desktop cookies), every switch, every lane change, limit polling and auto-switch. All of them answer `forbidden` with "accounts are off: the Swift app owns logins until the maintainer turns this on". Listing saved account names from the daemon's own files still works. |
| `vault` | The daemon binds no vault socket, never opens the vault Keychain item, and answers every vault op with "the vault is off: the Swift app owns it until the maintainer turns this on". |
| `install` | Nothing is written into `~/.claude/settings.json`, any tool's skills folder, or a LaunchAgent. Install ops answer with the plan they would carry out (a dry run) and the off message. |

Cost tracking only reads transcripts, so it is not switched.

### Never, from the Swift notes

- Never renew a token that is in use. Refresh tokens rotate.
- An account is live in one place: the main login or one lane.
- Switching replaces only `claudeAiOauth` in `Claude Code-credentials` and only `oauthAccount` in `~/.claude.json`.
- Account logins go through `/usr/bin/security` with hex on stdin, never the Security framework.
- The vault item goes through the Security framework, so that only this build is trusted.
- Lane Keychain names are reproduced exactly: `Claude Code-credentials-<sha256(path)[:8]>`.
- Antigravity's Keychain item (service `gemini`, account `antigravity`, go-keyring encoding) is updated in place, and the mirror file is set too.
- Desktop switching swaps the whole Cookies file while Claude Desktop is not running.
- Codex `auth.json` is written under a flock on `auth.lock`, and `codex app-server daemon restart` runs with its output to /dev/null.
- Editing `~/.claude/settings.json` touches only our own entries (matched on our command), keeps a backup, and keeps the user's other hooks.
- No secret, token or cookie in code, tests, fixtures, logs, issues or pull requests.

### Tests

- Tests never touch Keychain, real logins, ssh, `gh`, the network, `~/.claude*`, `~/.codex`, `~/.gemini`, Claude Desktop's folder or LaunchAgents.
- Every outside effect sits behind a trait with a fake:
  - Keychain (`/usr/bin/security` runner, Security framework item);
  - HTTP (usage and profile endpoints);
  - process launch and the app-running check;
  - the file roots;
  - ssh/`gh` for Machines (`ScriptedRunner`-style).
- Port the Swift tests for each part, including `MachinesSafetyTests`' pinned guarantees.

### Tiers

- Account switching, lanes, vault values, vault edits, Machines fixes, install and settings are **app tier**, audit-logged.
- The agent tier gets the narrow vault ops (`vault.find`, `vault.use`, `vault.request`) and read-only cost.
- Vault use still needs the approval prompt for entries marked "ask first". Approval comes only from an attached app client, authenticated as app tier. When no app is attached, approval is denied.

### Code

- Files under 600 lines.
- No unwrap or expect outside tests.
- `cargo deny` passes, and `deny.toml` is never edited. If a needed crate fails `cargo deny`, stop and report it.
- Linux and Windows type-check, with `Unsupported` where a part is macOS-only, and that said in `docs/goals.md`'s per-OS note when it is user-visible.
- Each new view has a `--render` name.

## Tasks

Wave 1 runs in parallel (Tasks 1–5). Wave 2 (Tasks 6–7) starts after wave 1 merges. Wave 3 is Task 8.

### 1. E8a, Claude Code accounts and lanes (`engine::accounts::claude`, `engine::lanes`)

- **`AccountStore` and `AccountLogin`.**
  - Saved slots and the live login.
  - The `security` runner (hex on stdin).
  - `oauthAccount` in `.claude.json`.
  - The profile endpoint check.
  - Switching rules.
  - Sign-in through the browser flow's pure parts.
- **`LaneStore`.**
  - `~/.claude-lanes/<name>`, shared symlinks, `relink`, `lanes.json`, `current()` from `CLAUDE_CONFIG_DIR`.
  - The lane Keychain name.
  - The one-place rule.
  - Switching a lane (copy back after the profile check, then set).
- **Move All's account side**, on top of M3's `MoveAllPlan`.
- **Daemon ops**, behind `accounts`: `accounts.list`, `accounts.switch`, `accounts.add.begin/finish`, `accounts.remove`, `lanes.list`, `lanes.create`, `lanes.remove` and `lanes.switch`.

### 2. E8b, the other providers (`engine::accounts::{codex, antigravity, desktop}`, `engine::providers`)

- **The provider registry and `UsageReport`/`GroupedUsage`**, ported from the Swift sources.
- **Codex.** Ported from `Codex.swift`.
- **Antigravity.**
  - The Keychain item and the mirror file.
  - Limits from its local language server (port by `lsof -a`, CSRF token from its arguments).
- **Claude Desktop.** Cookies capture and restore, and config.json.
- **Daemon ops**, behind `accounts`: `providers.list`, `providers.switch {provider, id}`, `providers.add.begin`, `providers.usage {provider}`.

### 3. E8c, limits, auto-switch and cost (`engine::limits`, `engine::cost`)

- **`LimitWatcher`, `ProviderLimits` and `LimitLogic`.**
  - `apply(limits:)`, with fixed keys as a fallback.
  - Thresholds at 90 and 100.
  - `pickTarget`, `maySwitch`, `blockedUntil`, `decideSwitch`.
  - Modes per provider, with the setting keys from the Swift app.
  - Codex is idle-only.
- **The watcher loop.** It lives in the daemon behind `accounts` and calls Tasks 1 and 2 through traits, so it can merge first with fakes.
- **`CostTracker` and `CostSources`.**
  - Every channel.
  - Dedupe by `message.id|requestId`.
  - 7, 30 and all-time summaries per account.
  - `OtherPrices`.
  - Its own versioned cache in `<data>/daemon/`.
- **Ops.**
  - `limits.status` (app) and `limits.set_mode` (app).
  - `cost.summary {range}` (agent tier, read-only, not switched).

### 4. E9, the Secret Vault (`engine::vault`, the vault service in the daemon, CLI)

- **The pure parts of `SecretVault`.**
  - Entries, `find` ranking, `prefilled`.
  - Expiry and reminders logic.
  - `Redactor`: values of 4 bytes or more, holding back only a tail.
- **`SecretRunner`**, streaming through the redactor.
- **`VaultStore` behind a `VaultKeychain` trait.**
  - One item: service `Notchling Secret Vault`, account `values`.
  - The Security framework implementation, trusting this build only, macOS only.
- **`vault.json` (0600)**, in its own folder (`<data>/daemon/vault.json`) until handover. It never writes the Swift app's `vault.json`.
- **The vault service in the daemon**, behind `vault`.
  - Agent-tier ops: `vault.find`, `vault.use {names, folder, command}`, `vault.request {...}`.
  - App-tier ops: `vault.list`, `vault.save`, `vault.remove`, `vault.approval.answer`.
  - Events: `vault.approval.pending` and `vault.request.pending`.
- **CLI.** `notchling secrets [words]`, `notchling secret <a,b> -- <command>`, `notchling secret-request <name> [...]`, with the Swift `--secrets`/`--secret`/`--secret-request` output.
- **The skill text.** `VaultSkill`, as a string.

### 5. E13, installing into other tools (`engine::install`)

- **Hooks.**
  - Plan, then apply, the prompt hooks and the page hooks (`PreToolUse`, `PostToolUse`, `PostToolUseFailure`, with the `Artifact` matcher told apart) into a given `settings.json`.
  - Our entries are matched on our command.
  - Backup, keeping the user's other hooks, and remove/detect.
  - The command is `notchling hook`.
- **Skills.** Write the vault, pages and delegate skills into the Claude Code, Codex and Antigravity skills folders, and remove them.
- **Service.** `notchling service install` as today: print, never run `launchctl`.
- **Ops**, app tier, behind `install` (a dry run while off): `install.plan`, `install.apply`, `install.remove`.
- **Tests** run against temp copies of real-shaped `settings.json` fixtures, including ones with the user's own hooks, the Swift app's `--hook` entries, comments of odd shapes and broken JSON (refused, not overwritten).

### 6. U5 and U9, the app's accounts, vault, limits and cost (wave 2)

- **Windows and sheets.**
  - The Accounts window: tabs per provider, the shared `AccountRow`, Add Account and Move All To.
  - The Accounts sheet from Chat.
  - The header's account menu.
  - The vault sheet: add, edit, remove, how-to, expiry, Renew.
  - The vault approval prompt (Deny / Allow Once / Always Allow), which can't be faked by a non-app client, and the request form panel.
  - Limit bars (`GroupedLimits`) and the cost view.
- **Wiring.** Everything goes through Tasks 1–4's ops, and is disabled with the off message while its switch is off.
- **Render names:** `accounts-window`, `vault-sheet`, `vault-approval`, `vault-request`, `limit-bars` and `cost`.

### 7. U11, first run and updates (wave 2)

- **The first-run flow.**
  - It detects a running or installed Swift app and says it owns the hook, the vault and the logins until handover.
  - It offers to install hooks only when no Swift `--hook` entry exists, which is a dry run while `install` is off.
- **Migration** of the Swift app's settings (all 30 keys from E14) into the app's settings, read-only from the Swift domain.
- **About.**
- **The update check** against the Phase 6 feed, behind its kill-switch variable, with nothing downloaded while it is off.
- **A handover checklist view** that lists each switch, who owns the area now, and what the maintainer does to hand it over.
- **Render names:** `first-run`, `about` and `handover`.

### 8. E10, Machines (wave 3)

- **A separate binary, `notchling-machines`** (a GPUI window plus its engine module), not inside the daemon.
- **What is ported.**
  - `MachineRegistry` (v2, with the v1 backup).
  - `StepCatalog`, `SetupEngine` (Check plus Fix, batched probes, Fix only on a click after its commands were shown).
  - Secrets over ssh stdin only, via the vault.
  - The admin password on `sudo -S` stdin for one step only, `env -i` with a fixed PATH.
  - The Linux rules for root and the worker account.
  - `TokenScope`, Replace key, pending updates and the morning items.
- **Data.** It reads the Swift app's `machines.json` read-only. Its writes go to `<data>/daemon/machines/` until handover.
- **Tests.** Port every Machines test, including `MachinesSafetyTests`.

## Review Focus

- **Switches off.** With every switch off, no code path may:
  - run `/usr/bin/security` or a SecItem call;
  - read a token;
  - make a usage or profile request;
  - write `~/.claude.json`, `settings.json`, `auth.json`, Cookies or a skills folder;
  - bind `vault.sock`.
- **Tokens.** No path renews a token that is in use. No token, cookie or vault value reaches a log, an event, an error message, the audit log or a reply to an agent-tier client.
- **The vault.** An agent-tier client can't approve its own use, can't read values except through `vault.use` with approval when required, and can't learn values through error messages. The redactor holds across chunk boundaries.
- **settings.json.** The user's other hooks survive install, remove and a broken file. A backup is written before every change.
- **Machines.** No Fix runs without a click after its commands were shown. A hostile remote value can't inject a command. Secrets never appear in argv.
