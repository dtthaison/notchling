# Notchling

A small macOS menu bar app for people who run several [Claude Code](https://claude.com/claude-code) agents at once. It keeps the Mac awake while agents work and turns the camera notch into a live view of every session. From the notch you can read conversations, answer Claude's questions, approve permission prompts and reply to background agents without hunting for the right terminal.

## What it does

- **Keeps the Mac awake.** Off, Always On, or only While Agents Are Working. Optionally keeps the display on too.
- **Shows agents in the notch.** Hovering over the notch opens a list in three tabs: **Input** (waiting on you), **Working** and **Done**. Each row shows the session name, where it runs (Background, VS Code, Terminal, `claude -p`), when it last changed, your last prompt and what it's doing now.
- **Peeks when something happens.** When an agent finishes or needs you, the notch slides open briefly with a short summary and plays a sound.
- **Shows conversations like the terminal.** Click a row to see `❯` prompts, `⏺` replies and `⎿` tool summaries ("Read 2 files, ran 1 shell command").
- **Replies to background agents.** The reply box sends text, including slash commands with completion, to agents started with `claude --bg`.
- **Answers prompts.** Questions (`AskUserQuestion`) and permission requests from any session can be answered from the notch with ⌘1–9 or a click. Allow, Always allow and Deny all work, and so does typing your own answer.
- **Manages sessions.** Right-click a row to **Hide** it, or **Remove Agent…** to delete a background agent (`claude rm`). **Clear all** in the Done tab hides every finished item.
- **Switches Claude accounts.** Every account you sign in to is saved, and one click in the menu points Claude Code at another one without signing in again. Each account shows its 5-hour and weekly limits and when they reset.
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
| Claude Accounts | One row per saved account with its limits; click one to switch Claude Code to it |
| Add Account… | Opens the Claude Accounts window and signs in to another account in the browser |
| Manage Accounts… | The Claude Accounts window: every account with its limits, Switch, Remove, and a Token Cost tab |
| Antigravity | The Google account Antigravity is on, and Switch Account with every saved one plus Add Account… |
| Token Cost | Cost at API prices by day, model and project |
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

**Antigravity.** Antigravity and Antigravity IDE share one Google login in `~/.gemini/jetski-standalone-oauth-token` and read it at launch. Notchling copies each login it sees into its own Keychain items (`Notchling Antigravity`, one per Google account). Switching quits Antigravity, writes the saved login back to that file and opens Antigravity again. **Add Account** saves the current login, removes the file and opens Antigravity at its sign-in screen; the next login is saved as soon as it appears. While Antigravity is open, Notchling reads the active account's limits from Antigravity's own local language server (the source of its View Usage panel): it finds the process, its CSRF token and port, and calls `RetrieveUserQuotaSummary` over localhost. Nothing goes over the network for Antigravity.

**Cost** is estimated from the `usage` of every assistant message in the transcripts of the last 30 days, each message counted once, with cache writes and reads priced separately. Totals and read positions are cached in `cost-cache.json`, so only new lines are read after the first scan.

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
4. To drop saved logins, delete the `Notchling Accounts` and `Notchling Antigravity` items in Keychain Access.
