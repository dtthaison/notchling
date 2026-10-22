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
- **Has a pet.** A ✻ rolls around the notch while agents work and tows an orange badge with the number waiting on you. Turn the movement off with **Animate Pet**.

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
| Keep Mac Awake → Off / Always On / While Agents Are Working | Holds a power assertion so the Mac doesn't idle-sleep |
| Keep Display On | Keeps the screen awake as well |
| Show Agents in Notch | Shows or hides the notch panel |
| Notch Width → Small / Medium / Large | How wide the list, peeks and chat open |
| Animate Pet | Lets the ✻ roll around the notch |
| Answer Prompts from the Notch | Installs or removes the Claude Code hook (see below) |
| Launch at Login | Registers the app as a login item |

## How it works

Notchling only reads files Claude Code already writes. It never talks to the Claude API.

| Source | Used for |
|---|---|
| `~/.claude/sessions/<pid>.json` | Live sessions and their status (busy, waiting, idle) |
| `~/.claude/jobs/<id>/state.json` | Background agents: state, summary, what they need from you |
| `~/.claude/projects/<project>/<session>.jsonl` | Conversation text, current activity, open questions (last ~2 MB only) |

**Replying** to a background agent runs `claude attach <id>` in a hidden pseudo-terminal, types the text and detaches. Terminal and VS Code sessions can't take typed replies from outside their window; the ↗ button jumps to the right tab or window instead.

**Answering prompts** uses a Claude Code hook. Turning on **Answer Prompts from the Notch** adds two entries to `~/.claude/settings.json` (a backup is saved as `settings.json.notchling-backup`):

- `PermissionRequest` (all tools) and `PreToolUse` for `AskUserQuestion`, both running `Notchling --hook`.

The hook writes the request to `~/Library/Application Support/Notchling/requests/` and waits up to 290 seconds for an answer in `responses/`. If you don't answer, or the app isn't running (no heartbeat in the last 15 seconds), the hook steps aside and Claude Code asks in the terminal as usual. Headless `claude -p` sessions are always left alone.

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
  NotchModel.swift        notch state, sizes and actions
  NotchController.swift   the transparent panel over the notch and mouse handling
  NotchView.swift         all SwiftUI views (list, peek, cards, chat, pet)
Icon/make-icon.swift      draws the app icon
```

## Uninstall

1. Turn off **Answer Prompts from the Notch** so the hook entries are removed from `~/.claude/settings.json`.
2. Turn off **Launch at Login**, then quit Notchling.
3. Delete `/Applications/Notchling.app` and `~/Library/Application Support/Notchling`.
