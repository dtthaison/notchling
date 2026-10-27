# Notchling

[![Build](https://github.com/dtthaison/notchling/actions/workflows/build.yml/badge.svg)](https://github.com/dtthaison/notchling/actions/workflows/build.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

A small macOS menu bar app for people who run several [Claude Code](https://claude.com/claude-code) agents at once. It keeps the Mac awake while agents work and turns the camera notch into a live view of every session. From the notch you can read conversations, answer Claude's questions, approve permission prompts and reply to background agents without hunting for the right terminal.

Notchling is free and open source, and isn't affiliated with Anthropic or Google.

## What it does

- **Keeps the Mac awake.** Off, Always On, or only While Agents Are Working. Optionally keeps the display on too.
- **Shows agents in the notch.** Hovering over the notch opens a list in three tabs: **Input** (waiting on you), **Working** and **Done**. Each row shows the session name, where it runs (Background, VS Code, Terminal, `claude -p`), when it last changed, your last prompt and what it's doing now.
- **Peeks when something happens.** When an agent finishes or needs you, the notch slides open briefly with a short summary and plays a sound.
- **Shows conversations like the terminal.** Click a row to see `❯` prompts, `⏺` replies and `⎿` tool summaries ("Read 2 files, ran 1 shell command").
- **Replies to background agents.** The reply box sends text, including slash commands with completion, to agents started with `claude --bg`.
- **Answers prompts.** Questions (`AskUserQuestion`) and permission requests from any session can be answered from the notch with ⌘1–9 or a click. Allow, Always allow and Deny all work, and so does typing your own answer.
- **Manages sessions.** Right-click a row to **Hide** it, or **Remove Agent…** to delete a background agent (`claude rm`). **Clear all** in the Done tab hides every finished item.
- **Switches Claude Code accounts.** Every account you sign in to is saved, and one click in the menu points Claude Code at another one without signing in again. Each account shows its 5-hour and weekly limits and when they reset.
- **Switches Claude Desktop accounts.** The Claude desktop app's account is swapped too (by restoring its saved session and restarting it). Accounts are shared with the Claude Code list; an account can be switched to once it has been signed in to in Desktop at least once.
- **Switches Antigravity accounts.** Each Google account you sign in to Antigravity with is saved too. Switching restarts Antigravity on the other account, with no new sign-in. The account in use shows its Gemini, Claude/GPT and Gemini 4 Argon limits while Antigravity is open.
- **Shows token cost.** What your Claude Code use would cost at API prices: today, 7 and 30 days, a 14-day chart, and totals by model and by project.
- **Has a pet.** A little green monster web-swings along the whole menu bar while agents work: at the top of each swing it lets go, tumbles through the air, shoots a strand of silk to the edge ahead and swings on that. It picks a spot along the bar, heads there with long hops and short ones, rests to wave or doze when it arrives, then picks another. Each strand gets its own length, swing and poses. It holds an orange badge with the number waiting on you, and goes faster while that's above zero. Turn the movement off with **Animate Pet**.

## Requirements

- macOS 13 or later; built for Apple Silicon and Intel. Designed around a notched MacBook screen. Without a notch it hangs under the menu bar.
- Claude Code installed at `~/.local/bin/claude` (or `/usr/local/bin/claude`).
- Xcode Command Line Tools (`swiftc`). No Xcode project is needed.

## Build and install

```sh
./build.sh            # builds build/Notchling.app
./build.sh --install  # also quits the running copy, installs to /Applications and opens it
```

`build.sh` compiles `Sources/*.swift` for arm64 and x86_64, joins them with `lipo`, re-renders the icon when `Icon/make-icon.swift` changes, and signs the app ad hoc.

To start at login, use **Launch at Login** in the menu, or run:

```sh
/Applications/Notchling.app/Contents/MacOS/Notchling --enable-login-item
```

## Menu

Click the cup in the menu bar (filled while the Mac is being kept awake):

| Item | What it does |
|---|---|
| Claude Code | The account in use with its limits, and Switch Account with every saved one plus Add Account… |
| Claude Desktop | The account the desktop app is on, and Switch Account with the ones Notchling has captured plus Add Account… |
| Antigravity | The Google account Antigravity is on, and Switch Account with every saved one plus Add Account… |
| Manage Accounts… | The accounts window: a tab per provider (Claude Code, Claude Desktop, Antigravity), plus Claude Token Cost and Settings |
| Claude Token Cost | Cost at API prices by day, channel (Claude Code / Desktop), account, model and project |
| Keep Mac Awake → Off / Always On / While Agents Are Working | Holds a power assertion so the Mac doesn't idle-sleep |
| Keep Display On | Keeps the screen awake as well |
| Show Agents in Notch | Shows or hides the notch panel |
| Notch Width → Small / Medium / Large | How wide the list, peeks and chat open |
| Animate Pet | Lets the monster web-swing along the menu bar |
| Answer Prompts from the Notch | Installs or removes the Claude Code hook (see below) |
| Launch at Login | Registers the app as a login item |

## How it works

Notchling reads files Claude Code already writes. The only network calls are for accounts: with your own login it asks Anthropic which account a login belongs to (`/api/oauth/profile`), how much of each limit is used (`/api/oauth/usage`, every 5 minutes), and renews the token of a saved account that isn't in use.

| Source | Used for |
|---|---|
| `~/.claude/sessions/<pid>.json` | Live sessions and their status (busy, waiting, idle) |
| `~/.claude/jobs/<id>/state.json` | Background agents: state, summary, what they need from you |
| `~/.claude/projects/<project>/<session>.jsonl` | Conversation text, current activity, open questions (last ~2 MB only) |

**Replying** to a background agent runs `claude attach <id>` in a hidden pseudo-terminal, types the text and detaches. Terminal and VS Code sessions can't take typed replies from outside their window; the ↗ button jumps to the right tab or window instead.

**Answering prompts** uses a Claude Code hook. Turning on **Answer Prompts from the Notch** adds two entries to `~/.claude/settings.json` (a backup is saved as `settings.json.notchling-backup`):

- `PermissionRequest` (all tools) and `PreToolUse` for `AskUserQuestion`, both running `Notchling --hook`.

The hook writes the request to `~/Library/Application Support/Notchling/requests/` and waits up to 290 seconds for an answer in `responses/`. If you don't answer, or the app isn't running (no heartbeat in the last 15 seconds), the hook steps aside and Claude Code asks in the terminal as usual. Headless `claude -p` sessions are always left alone.

**Accounts.** Claude Code keeps its login in the Keychain item `Claude Code-credentials` and the account's profile under `oauthAccount` in `~/.claude.json`. **Add Account** runs the same OAuth sign-in as `claude auth login` (PKCE, with the browser sending the code back to a listener on 127.0.0.1), but saves the result without touching Claude Code's current login. Notchling also copies each login it sees into its own Keychain items (`Notchling Accounts`, one per account). Switching writes the saved login back into Claude Code's item (MCP logins in it are kept) and replaces `oauthAccount` (the previous file is kept as `~/.claude.json.notchling-backup`). The login Claude Code is using is never renewed by Notchling, since that would sign Claude Code out.

**Antigravity.** Antigravity and Antigravity IDE both read one Google login, `~/.gemini/jetski-standalone-oauth-token`, at launch, so only one account is ever signed in. Notchling keeps several and swaps which one is in that file.

- **Saving.** On launch and every 60 seconds, Notchling reads the login file, decodes its id token for the account's Google id and email, and copies the whole login into its own Keychain items (`Notchling Antigravity`, one per account). So any account you sign into while Notchling is running is remembered; the account list is in Notchling's preferences, the tokens only in the Keychain.
- **Switching** saves the current login, quits Antigravity and Antigravity IDE (they only read the file at startup), writes the chosen account's saved login into the file, and reopens whichever apps were running. It asks first when Antigravity is open, since running agents stop. Google refresh tokens don't rotate, so a set-aside login keeps working.
- **Add Account** saves the current login, deletes the file, and restarts Antigravity at its sign-in screen. Notchling watches the file and saves the new login as soon as you finish signing in; cancelling puts the previous account back.
- **Limits and plan.** While Antigravity is open, Notchling reads the active account's limits and plan from Antigravity's own local language server (the source of its View Usage panel): it finds the process, its CSRF token and port, and calls `RetrieveUserQuotaSummary` and `GetUserStatus` over localhost. Nothing goes over the network for Antigravity.

Unlike Claude Code, switching here **restarts** Antigravity rather than being seamless, limits and plan show only while it is open, and an account appears only after it has been signed in while Notchling was running.

**Claude Desktop.** The desktop app (`com.anthropic.claudefordesktop`) is a separate login from Claude Code, though the OAuth tokens are the same system. What decides its account is the claude.ai **web session cookies** in `~/Library/Application Support/Claude/Cookies`, not the token — so, like Antigravity, Notchling captures the whole Cookies file while an account is active and restores it to switch, setting `lastKnownAccountUuid` and restarting the app. Accounts are matched to the Claude Code list by account UUID. **Add Account** reopens Desktop signed out (the Cookies file is removed, backed up as `Cookies.notchling-backup`) so you can sign in to another account, which is then captured; cancelling puts the previous account back. An account can only be switched to after it has been signed in to in Desktop once, since that is the only way its cookies exist. Captured sessions live in `~/Library/Application Support/Notchling/desktop/<uuid>.cookies`.

**Claude token cost** is estimated from the `usage` of every assistant message in the transcripts of the last 30 days, each message counted once, with cache writes and reads priced separately. It reads Claude Code's transcripts (`~/.claude/projects`) and the Claude desktop app's own Claude Code transcripts (under its Application Support folder), tagging each as the **Claude Code** or **Desktop** channel — so Desktop cost only appears when you run Claude Code agents in it; normal Desktop chat is web-based and has no local token record. The window has a Last 7 Days / Last 30 Days / All Time selector and breaks cost down by channel, account, model (with each model's input/output/cache token totals and its share of the cost) and project. Each message is counted once (deduplicated by id), so totals are lower than Claude Code's `/stats`, which counts a message again each time a conversation is compacted or resumed. Totals, token counts and read positions are cached in `cost-cache.json`, so only new lines are read after the first scan.

## Project layout

```
Sources/
  main.swift              entry point: --hook, login item flags, app start
  AppDelegate.swift       menu bar item, settings menu, 3-second polling loop
  AwakeController.swift   IOKit power assertion
  AgentMonitor.swift      reads sessions and background jobs into Agent values
  TranscriptReader.swift  parses transcripts into chat lines, activity and open questions
  HookBridge.swift        the --hook process and installing the hook into settings.json
  ReplySender.swift       types replies and answers into background agents via claude attach
  AgentActions.swift      opens or focuses a session's terminal or editor window
  SlashCommands.swift     built-in, user, project and plugin commands for completion
  Accounts.swift          saved logins, switching, limits from the usage endpoint
  Antigravity.swift       saved Antigravity logins and switching
  ClaudeDesktop.swift     Claude Desktop cookie-session capture and switching
  CostTracker.swift       token cost at API prices from transcripts
  UsageMenuViews.swift    the account rows and cost chart in the menu
  AccountLogin.swift      in-app OAuth sign-in for adding an account
  AccountsWindow.swift    the Claude Accounts window (accounts, sign-in, token cost)
  NotchModel.swift        notch state, sizes and actions
  NotchController.swift   the transparent panel over the notch and mouse handling
  NotchView.swift         all SwiftUI views (list, peek, cards, chat, pet)
Icon/make-icon.swift      draws the app icon
```

## Uninstall

1. Turn off **Answer Prompts from the Notch** so the hook entries are removed from `~/.claude/settings.json`.
2. Turn off **Launch at Login**, then quit Notchling.
3. Delete `/Applications/Notchling.app` and `~/Library/Application Support/Notchling`.
4. To drop saved logins, delete the `Notchling Accounts` and `Notchling Antigravity` items in Keychain Access, and remove `~/Library/Application Support/Notchling/` (captured Claude Desktop sessions).

## Contributing

Bug reports, ideas and pull requests are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) covers building, the few rules around real logins, and what fits the project. Notchling is maintained in spare time, so expect a reply within about a week. Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

Found a security problem? Please report it privately as described in [SECURITY.md](SECURITY.md), not in a public issue.

## License

[MIT](LICENSE) © 2026 Son Doan and Notchling contributors.
