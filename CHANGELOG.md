# Changelog

Notable changes to Notchling. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Tool versions, read only: Settings > Agents shows where the Mac's Claude Code was found, its version and how it was installed, and each worker's version against the Mac's. Machines gets a Tool Versions row on every card (same, behind, ahead, or not installed) and says when Notchling isn't running. Nothing is installed or changed on a worker.
- A notifications inbox: a bell in Chat (and a badge in the open notch) collects what needs you, such as a vault approval, a secret an agent asked for, or a limit notice. Answer from Chat or from the phone, and get a system notification when something new arrives.

### Fixed

- Clicking an `.html` file in Chat (a link or path in a message, or a new file in the Details Panel's Git section) now shows it as a page, with scripts off, instead of its source. The header's Page and Source buttons switch between them, and a changed file's diff still opens first, with the page one click away.
- The Artifact pages library saves again. Since the Rust app replaced the Swift one, published pages were no longer copied to `~/Library/Application Support/Notchling/pages`, reads of a saved link no longer came from the local copy, and a failed read no longer pointed at it. `notchling hook` now handles the Artifact tool's events itself, with or without the app running.
- Chat showed almost nothing of a long session that had read pictures (a Read of a .jpg writes megabytes of base64 into the transcript, which filled the part Chat reads). Pictures no longer count toward what is read, so the conversation before them is back, the Read steps finish instead of showing as running, and the notch and Search read around them too.

## [1.2.0] - 2026-11-07

What each new feature is for and how to use it: [docs/features.md](docs/features.md).

### Added

- Codex and Antigravity CLI sessions in Chat and in the notch, beside Claude Code's. Read them (commands, edits as diffs) and continue them: Codex turns run with `codex exec` in the workspace-write sandbox, and Antigravity CLI runs over `agy acp` with live turns, Stop, permission cards and a model, thinking and mode picker. New Session starts either kind on any of their accounts.
- Second Opinion: ask Codex or Antigravity CLI to review an edit or a reply from Chat, with the tool, model and effort you pick. The review shows under the message and is kept across launches.
- `Notchling --delegate <codex|agy> -- <prompt>` lets one agent hand a job to another tool (an image, a second model's review) and wait for the result. Generated images are copied to `--out`. The `notchling-delegate` skill teaches the agents to use it.
- Secret Vault (key button in Chat's sidebar): tokens and passwords agents use without seeing them. `Notchling --secret <name> -- <command>` passes the value in the environment and hides it in the output. Optional approval before each use, expiry reminders, and `--secret-request` for an agent to ask for a missing one.
- ⌘⇧P also searches conversations: Claude Code transcripts in every lane, Codex sessions and Antigravity titles from the last 14 days. Return opens the session in Chat.
- The phone app (Agent Console) is built into Notchling: pair a phone from the Phone button in Chat's header to read and reply to Claude Code, Codex and Antigravity sessions, answer prompts and send photos. Existing pairings and tunnel settings carry over.
- Machines (Chat sidebar): turn other Macs and Linux servers into workers that pick up GitHub issues and work on them with Claude Code overnight, then review what they left in the morning (branches to merge, questions, jobs to retry). Each setup step has a read-only Check and a Set up that runs only on a click, after showing its commands. Secrets reach the machine only over ssh's standard input, and Replace key… rotates a deploy key.
- In Chat: the session's context and account limits in the header; model and effort pickers; Stop (or Esc) to stop a turn; Edit to take a queued message back; menu commands (`/plugin`, `/mcp`…) in a terminal sheet; `!` shell commands; paths that open in Finder or in a file panel with a viewer for each type; Run on shell code blocks; pretty JSON; image and video previews; subagents and background shells (with live output and Stop) in the Details panel; a git section; and a ? button that names every part on screen.
- Codex CLI accounts: save each ChatGPT login, switch without signing in again, see Codex limits, and get notified (or switched) near the limit. Antigravity limit alerts too.
- Token cost counts Codex and Antigravity CLI use as channels beside Claude Code and Desktop.
- Search Artifacts from anywhere with ⌘⇧P (or menu bar > Search Artifacts…): a Spotlight-style box near the top of the screen. Type to filter by title, project, account or a pasted link; ↑↓ to pick, Return opens the page beside the chat, ⌘Return opens it on claude.ai, ⌘C copies its link, Esc closes.
- Move Only This Session To (the account menu in a background session's header): moves one session to another account without touching the rest. It stops the session and resumes the same conversation in the background on that account (a free account gets a lane), keeping its name, model and permission mode. The first reply there re-reads the whole conversation, since the prompt cache stays with the old account.
- Right-click a session in Chat's sidebar to Hide it, or Remove a background session (stops it and deletes it with `claude rm`, after asking).
- Chat stays on the newest message while a step or reply grows (it stopped following before), until you scroll up; scrolling up while a session works no longer snaps back, and the Latest button shows at once. Chat reopens on the same session after Notchling relaunches (an update), if it was open.
- Chat's conversation and reply box use the full window width, and replies no longer have an avatar on the left. Your short messages are bubbles the size of their text.
- After a turn ends, Chat no longer shows "Thinking…" while only a background command is still running; it says that instead.
- Chat's working row shows the time since your prompt and the tokens written, like Claude Code (47s · ↓ 2.3k tokens). Drop files or images (a screenshot's thumbnail, Photos, a browser) anywhere on a background session's conversation to add their paths to the reply; images and temporary files are kept in ~/Library/Application Support/Notchling/drops for a week, so the path still works.
- Chat starts background sessions on any account: the + beside Sessions (pick an account, then a project: the folders your sessions work in, or Add Project Folder…), or New Background Session Here in a session's account menu. A free account gets a lane; the new session opens in Chat, idle until you type.
- Chat shows each session's Claude account: in the header always, and in the sidebar while lanes are in use. Click it to switch that session's lane (or the main login) to a free saved account, or to open a new terminal session on another account. A usage-limit switch shows in the sessions it moved. Background sessions in a lane show (and take replies) too.
- Chat shows what an agent did, not just what it said: tool steps folded into one summary line (open it for each command and its output, file edits as a diff, errors in red), thinking, the task list pinned on top with progress, a footer per turn with time and model, day separators, Copy on messages and code, and a Latest button when you scroll up. A side panel (the sidebar button) shows the session's branch, model, tokens, tasks and changed files.
- Chat renders markdown: headings, lists, quotes, code blocks with Copy, and tables, including ones pasted without the `|---|` row. Links are clickable; an artifact link with a saved copy opens in a panel beside the chat (drag its edge to resize, Esc to close), so it works from any account. Text selects like any Mac text, across paragraphs and lists.
- While a Notchling window is open (Chat, Artifacts, Accounts), the app shows in the Dock and in ⌘Tab, so you can switch away and back. Clicking the Dock icon brings the window back, even when minimized.
- A Chat window (menu bar > Chat…): every Claude Code session in a sidebar, its whole conversation, and a reply box for background sessions, with slash command completion, the suggested reply, and question and permission cards. Terminal sessions are read-only there, with a button to open their terminal. Replies are typed in locally with `claude attach`, as from the notch; nothing listens on a port.
- Claude Code can find a saved Artifact by link, id or title: `Notchling --page <link | id | words>`, `Notchling --pages`, and a `notchling-pages` skill that Notchling installs with the Artifacts hook.
- Search in the Artifacts window matches artifact ids and links.
- Keep Local Copies of Artifacts, on by default: every Artifact Claude Code publishes is also saved under `~/Library/Application Support/Notchling/pages`, so it stays readable after you switch accounts (claude.ai shows a page only to the account that published it).
- Lanes (menu bar > Claude Code > Lanes): run Claude Code on several accounts at once. A lane is a config folder of its own (~/.claude-lanes/<name>) on another account, for terminal and background sessions alike; settings, hooks, skills, plugins and history stay shared. New Lane starts one on a free account in a folder you pick, Switch To moves a lane to another account mid-session, and each lane switches on its own when its account nears the limit. An account runs in one place at a time; `/status` in a lane names the lane's account.
- Claude Code can update an Artifact another saved account published, without switching: Notchling publishes it with the owning account's saved login, and the page stays that account's. It refuses when the page changed on claude.ai since the last publish from this Mac, unless the publish says `force`.
- An Artifact read in Claude Code of a page Notchling keeps is answered from the local copy, so it works from any account and never waits on claude.ai or auto mode's check. Pages it doesn't keep are read from claude.ai as before.
- An Artifacts window: search and filter every saved page by account, read it in place, and see which account owns its claude.ai link. The Artifacts menu lists the newest 12; each opens beside the chat.
- Continue Agents After Sleep, on by default: when the Mac wakes, background agents whose reply was cut off by sleep are told to continue.
- When the Account in Use Nears Its Limit: warns at 90% and 100%, and switches to the saved account with the most room at a point you set (80–99%, default 95%). After a switch, background agents stopped on the limit are restarted and told to continue.
- Limit alerts show a Notchling banner with the app icon when macOS doesn't allow the app's notifications.
- Reads the usage endpoint's `limits` list, so per-model limits and new kinds of limit show up by themselves.

### Changed

- Accounts without a 5-hour window, such as Max 20x since October 2026, show and watch only their weekly limit.
- The menu bar keeps only the account in use, Open Chat, Keep Mac Awake, Settings… and Quit. Accounts, lanes, limits, token cost, Artifacts, Machines and the other settings are in Chat (the sidebar's bottom bar and its Settings sheet), so menu paths named above now start there.
- Closing a lane keeps the copies it had set aside (`.lane-old-*`) in `~/.claude-lanes/.kept` instead of deleting them.
- Login files Notchling writes (Codex `auth.json`, the Antigravity token, a lane's `.claude.json`, `vault.json`) are readable only by you from the moment they're created.

### Fixed

- Removing an account while its limits were still loading brought it back, with its saved login gone (#7).
- A reply containing an end-of-paste escape sequence could be typed into a session as keystrokes.
- An Antigravity session whose load failed could leave a reply stuck on "sending" and never retry.
- Remove or Always Allow in the Secret Vault could empty the vault list if `vault.json` was briefly unreadable.
- New Session and Move could hang on the first background session in a new lane.
- Chat shows slash commands and what they printed (/cost, /context, /model…); Claude Code 2.1.31x logs them in a new place, so they were missing. After /clear, Chat moves to the cleared session right away instead of showing the old conversation until the job caught up.

## [1.1.0] - 2026-10-27

First public release.

### Added

- Agents in the notch, in Input, Working and Done tabs, with peeks when an agent finishes or needs you.
- Keeping the Mac awake: Off, Always On, or While Agents Are Working.
- Replies to background agents, and answers to questions and permission prompts from the notch.
- Account switching for Claude Code, Claude Desktop and Antigravity, with each account's limits.
- Claude Token Cost: today's cost in the menu, and a window with cost at API prices by day, channel, account, model and project.
- The web-swinging pet, which sits on the frontmost window's title bar when no agent is working.
- Open source under the MIT License, with a contributing guide, code of conduct and security policy.

### Fixed

- The menu bar menu no longer lags or skips items when moving between them.

[Unreleased]: https://github.com/dtthaison/notchling/compare/v1.2.0...HEAD
[1.2.0]: https://github.com/dtthaison/notchling/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/dtthaison/notchling/releases/tag/v1.1.0
