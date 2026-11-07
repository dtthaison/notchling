# Notchling feature guide

This guide covers what Notchling does beyond the notch, one feature at a time: what it is, why it's there, how to use it, and when you'd reach for it. The README covers the notch, keeping the Mac awake, and Claude account switching. [CHANGELOG.md](../CHANGELOG.md) lists changes release by release.

Most of the app now lives in one window, **Chat**. The menu bar keeps only the account in use, **Open Chat** (⌘O), **Keep Mac Awake**, **Settings…** (⌘,) and **Quit**.

- [Chat: every session in one window](#chat-every-session-in-one-window)
  - [Reading a session](#reading-a-session)
  - [Replying, stopping, and taking a message back](#replying-stopping-and-taking-a-message-back)
  - [Model, effort, and slash commands](#model-effort-and-slash-commands)
  - [New Session](#new-session)
  - [Which account a session uses](#which-account-a-session-uses)
  - [Files, paths, and commands in messages](#files-paths-and-commands-in-messages)
  - [Subagents](#subagents)
  - [Background shells](#background-shells)
  - [The ? button](#the--button)
- [Codex and Antigravity CLI sessions](#codex-and-antigravity-cli-sessions)
- [Second Opinion](#second-opinion)
- [Delegating a job to another tool](#delegating-a-job-to-another-tool)
- [Secret Vault](#secret-vault)
- [Search (⌘⇧P)](#search-p)
- [Phone](#phone)
- [Machines](#machines)
- [Accounts, limits, and cost for every tool](#accounts-limits-and-cost-for-every-tool)
- [Lanes](#lanes)
- [Artifacts](#artifacts)

---

## Chat: every session in one window

**What it is.** A full-size window for your agent sessions: Claude Code (background and terminal sessions, in every lane), Codex, and Antigravity CLI. The sidebar on the left lists them; the conversation is in the middle; the Details panel on the right shows the session's tasks, git status, subagents and shells.

**Why.** The notch is good for a glance and a quick answer. Following a long turn, reading a diff, or running five sessions at once needs room, and the terminal hides background sessions altogether.

**How to use.** Click the cup in the menu bar, then **Open Chat** (⌘O). Click a session in the sidebar to open it. Right-click a row for Reveal in Finder, Copy Session ID, Hide or Close Session, and Remove Background Session… (this one deletes the session).

**Use it when** you run several agents and want to follow one in detail without finding its terminal, or when a background session (`claude --bg`) needs a reply.

### Reading a session

- **Messages** render as Markdown, with tables, code blocks (with Copy) and links. Text selects across paragraphs like any Mac text.
- **Tool steps** fold into one line ("Read 3 files, ran 2 commands"). Open a step to see each command with its output, and file edits as a diff.
- **`!` shell commands** show as "! command" with their output. **Slash commands** show with what they printed.
- **The header** shows the session's context use and the account's 5-hour and weekly limits, with when each resets.
- **The Details panel** lists the session's tasks, files changed, git branch and status (refreshed every 5 s without taking git's lock), subagents and background shells.

### Replying, stopping, and taking a message back

- **Return** sends; Option-Return or Shift-Return adds a line. A message you send while the session works is **queued**, and shows as a Queued bubble until the session takes it.
- **Stop** (or Esc) stops a working session's turn, the same as pressing Esc in its terminal.
- **Edit** on your newest queued message takes it back into the reply box, so you can change it before the session reads it.
- Replies are typed into the session locally through `claude attach`; nothing listens on a port. A reply never gets typed into an open question or permission dialog: Chat shows that dialog as a card to answer instead.

**Use it when** you've changed your mind mid-turn: Stop, or Edit what you queued.

### Model, effort, and slash commands

- The **model picker** under the reply box switches the session's model (it sends `/model`). The **effort picker** sends `/effort`. Claude Code also saves the effort as the default for new sessions.
- Type `/` for completion of every slash command the session knows: built-in, plugin, and project commands.
- Commands that open a menu (`/plugin`, `/mcp`, `/agents`…) open in a **terminal sheet** inside Chat, attached to the session, since they draw a full-screen menu.

### New Session

**What it is.** A sheet (the **+** beside Sessions, or the empty screen) with three columns: the account (any Claude Code, Codex or Antigravity account, with an Add row for each tool), the folder, then Start.

**How to use.** Pick an account and a folder, then press Start. Claude Code starts `claude --bg` there; Codex and Antigravity sessions start with your first message. A folder Claude Code hasn't trusted yet is refused with "Workspace not trusted". **Trust Folder and Start** accepts the trust prompt for you.

**Use it when** you want a new agent without opening a terminal, or want it on a specific account from the start.

### Which account a session uses

**What it is.** The account name in the session header is a menu. Pick another account and only this session moves there: it stops and resumes the same conversation in the background on that account, keeping its name, model and permission mode. **Move All Sessions To** puts every session on one account. **Add Account…** and **Codex Accounts…** sign in without leaving Chat.

**Why.** When one account nears its limit, you can move the busiest session instead of switching everything.

**Use it when** an account is about to hit its limit mid-task, or you want a long job to run on a quieter account.

### Files, paths, and commands in messages

- A **path** that exists becomes a link. A folder opens in Finder. A file opens in a panel beside the chat with a viewer that suits it: Markdown rendered, HTML in a web view, code monospaced, images, PDF, video and audio, or Quick Look for anything else. The panel reloads when the file changes.
- A **shell code block** gets a **Run** button, and a **command in inline code** is a link that asks before running. Both run in a terminal sheet with your own login shell, in the folder where the command's paths exist.
- **JSON** code blocks and `.json` files show pretty-printed, with a Pretty/Raw toggle. Messages that name image or video files show a preview card underneath.
- The panel stays put while you switch sessions, scroll position included.

**Use it when** an agent says "I wrote the report to `out/report.md`": click the path and read it beside the conversation.

### Subagents

**What it is.** Agents a session started with the Agent tool. The Details panel lists each one with its status; **Open** reads its own transcript in place of the conversation (Esc or ⌘[ goes back). The notch shows a badge on sessions whose subagents are active.

**Use it when** a session fans work out to subagents and you want to see what each one did, not only the summary it handed back.

### Background shells

**What it is.** Commands a session started with `run_in_background` (dev servers, long builds). The Details panel lists them with **Output** (live, in the file panel) and **Stop** (SIGTERM, then SIGKILL two seconds later). The header shows an "N shells" chip while any are running.

**Use it when** a session left a dev server running and you want to see its log, or stop it, without finding the process.

### The ? button

**What it is.** The ? at the bottom left of the sidebar names every part on screen, in Chat and in the notch.

**Use it when** you want to tell someone (or an agent) exactly which part of the window you mean: "the Context Meter", "the Session Menu".

---

## Codex and Antigravity CLI sessions

**What it is.** Codex CLI and Antigravity CLI (`agy`) sessions appear in Chat and in the notch beside Claude Code's, with their tool's icon. You can read them (commands, edits as diffs, reasoning) and continue them.

- **Codex**: sessions from the last 14 days are listed. Each message runs one `codex exec` turn in the workspace-write sandbox: the folder is writable and nothing else is. The network is on by default, so the Secret Vault and API calls work; turn it off with **Settings > Allow Network in Codex Turns**. Codex's slash commands are there too: `/model` opens Codex's own model and effort picker, `/new` and `/clear` start a new session, and the rest run in a terminal sheet. Stop sends an interrupt.
- **Antigravity CLI**: Chat runs `agy acp` (the Agent Client Protocol) for each open session. Turns stream live, Stop cancels, permission requests show as cards with Antigravity's own options (nothing is auto-approved), and a picker sets Model, Thinking Level and Mode.
- In the **notch**, Codex and Antigravity rows peek when a turn ends, and take replies and Antigravity permission answers. Answering in one place clears the card in the other.

**Why.** If you use more than one coding agent, one window for all of them beats three terminals.

**Use it when** you split work across tools (say Claude Code for the main task and Codex for a side job) and want to follow both in one place.

*Gemini CLI is not supported: Antigravity CLI replaced it on 2026-06-18.*

---

## Second Opinion

**What it is.** Ask Codex or Antigravity CLI to review an edit or a reply, straight from Chat. Click the check-bubble button on the edit or reply: a popover lets you pick the tool, model and effort, and fills in a prompt. A card shows the review while it runs (with Cancel), then the reply in Markdown with **Open Session** to continue the conversation. Reviews are kept per session, across launches.

**Why.** A model from another family catches different mistakes. Agreement between two models is a stronger signal than one model checking itself.

**Use it when** an agent made a risky change (a migration, concurrency, security) and you want a quick independent review before you accept it.

---

## Delegating a job to another tool

**What it is.** A command any agent can run to hand a job to another tool and wait for the result:

```sh
Notchling --delegate codex -- "Review the diff on this branch for bugs"
Notchling --delegate agy --model <model> --effort high --out ~/Desktop/img -- "Draw an app icon: a green monster"
Notchling --delegate-models   # each tool's models and efforts
```

It runs a Codex `exec` turn or an `agy acp` session, blocks until it ends, prints the reply, and copies generated images to `--out`. It's read-only unless you pass `--write`. Other flags: `--folder`, `--session` (continue one), `--timeout`, `--json`. The `notchling-delegate` skill teaches Claude Code, Codex and Antigravity how to use it, so you can just ask: "have Codex review this".

**Why.** Each tool is good at different things: image generation, a second model's review. An agent can't drive another tool's interactive CLI, but it can run one command and wait.

**Use it when** Claude Code needs an image (Codex or Antigravity can generate one), or you want a different model's review as part of a task.

---

## Secret Vault

**What it is.** Passwords and API tokens that agents can *use* without ever *seeing*. Open it with the key button at the bottom of Chat's sidebar. Each secret has a value, the URL it's for, the environment variable names it's passed as, a note for agents, how to renew it, and an optional expiry day.

**How it works.**

- Values live in one Keychain item that only Notchling can read. The list itself (`vault.json`) holds no values.
- An agent runs a command with the secret in its environment:

  ```sh
  Notchling --secrets github.com                  # which secrets fit (names, never values)
  Notchling --secret my-token -- gh api user      # runs with $MY_TOKEN set
  ```

  Every value is replaced with `[secret:<name>]` in the command's output, so it never lands in the chat.
- **Ask me before an agent uses it** shows an approval prompt (Deny / Allow Once / Always Allow) with the folder and the command.
- An agent can ask you for a missing secret: `Notchling --secret-request <name> --url … --how …` opens the form, filled in, and nothing is saved until you press Save.
- Notchling reminds you 7 days and 1 day before an expiry, on the day, and once it has expired. **Renew…** opens the token's page and its form.
- The `notchling-vault` skill is installed for Claude Code, Codex and Antigravity, so agents use the vault instead of asking you to paste tokens.

**Why.** Pasting a token into a chat puts it in transcripts, logs and model context. The vault keeps it out of all three.

**Use it when** an agent needs to call an API, publish a package, or log in to something.

**Limits.** A command an agent is allowed to run could still encode a value past the redactor. That's what "Ask me before" is for.

---

## Search (⌘⇧P)

**What it is.** A Spotlight-style search box from anywhere (⌘⇧P, no Accessibility permission needed). It finds **Artifacts** you published and **conversations**: Claude Code transcripts in every lane, Codex sessions, and Antigravity titles from the last 14 days. Results show the tool, folder, age and a snippet around the match.

**How to use.** Type, use ↑↓ to pick, and press Return to open it in Chat. A session that isn't in the sidebar is added. Esc closes and returns you to the app you were in.

**Use it when** you remember a conversation ("the one where we fixed the login race") but not which session or tool it was.

---

## Phone

**What it is.** Your sessions on your iPhone through the mobile app (`apps/flutter/`, named Agent Console on the phone): read conversations, reply, answer questions and permissions, take over terminal sessions, and send photos. Claude Code, Codex and Antigravity sessions are all there, with each session's context and account limits.

**How to use.** In Chat, click the **Phone** button at the top right of the header and scan the QR code (or type the one-time code) in the phone app. Paired phones are listed there and can be removed. The bridge runs over TLS on your network, or through a Cloudflare tunnel with Access when you're away.

**Why.** Agents keep asking questions while you're away from the desk.

**Use it when** a long job is running and you step away: answer its permission prompt from your phone instead of coming back to a stalled session.

*This replaces the separate Agent Console app. Pairings and the tunnel setup carried over.*

---

## Machines

**What it is.** Turn your other Macs and Linux servers into agent workers, and manage them from one window (Chat sidebar > **Machines**). A worker machine picks up GitHub issues from the repositories you give it and works on them with Claude Code while you're away. It can read the code and comment on issues, but never push. In the morning, **Jobs** lists what the workers left for you: a branch to merge, a question to answer, or a job to retry. **Machines** shows each machine as a card of setup steps.

**How it works.**

- Each step (reachable, worker user, stays awake, FileVault, base tools, Claude Code, Claude login, Xcode, deploy key, token scope, the worker job…) has a **Check**, which only reads, and a **Set up**, which runs only when you click it, after showing you the exact commands.
- **Check** re-runs the checks. Cheap ones refresh every minute; slow ones (Xcode, GitHub) run when the window opens, on Check, and every 10 minutes. **Set up all** runs every step that isn't ✓ yet.
- Secrets reach the machine only over ssh's standard input, into mode-600 files, and show everywhere as `[secret:<name>]`. When a vault value changes, a banner offers to update the machines that use it.
- An admin step asks for the admin password in a secure field for that one step and keeps it nowhere. On Linux the worker account must not be able to use sudo, so admin steps run as root over SSH instead.
- **Replace key…** rotates a machine's read-only deploy key. The old key keeps working until the new one is proven.

**Why.** A spare Mac mini or a cheap server can keep agents working through the night, so issues are done when you wake up. Setting one up by hand is twenty steps across ssh, GitHub and Keychain, and it's easy to get one wrong: a token with too much scope, a worker that can push, a login in the wrong account.

**Use it when:**

- you have a backlog of small, well-described issues (bug fixes, tests, refactors) and want them worked on overnight, ready for review in the morning;
- you add a new worker machine, or rotate its keys and tokens;
- a worker stopped making progress and you want to see at a glance which step broke.

---

## Accounts, limits, and cost for every tool

**What it is.** The Accounts page (Chat sidebar > **Settings**, or menu bar > Settings…) has a tab per tool: Claude Code, Claude Desktop, Codex and Antigravity. Each saves every account you sign in to and switches without signing in again.

- **Codex accounts**: each ChatGPT login the Codex CLI uses is saved, with its Codex limits. Add Account signs in through `codex login` while Codex stays on the account in use.
- **Antigravity accounts**: switching restarts Antigravity on the other Google account. Antigravity CLI uses the same login.
- **Limit watching** covers Codex (Notify, or Notify and Switch, which switches only when no `codex` process is running and the ChatGPT app is closed) and Antigravity (Notify), as well as Claude Code.
- **Token cost** counts Codex and Antigravity CLI use beside Claude Code and Desktop, by day, account, model and project, at API prices.

**Use it when** you have several subscriptions and want to use them evenly, or see what a month of agent use would cost at API prices.

---

## Lanes

**What it is.** Run Claude Code on several accounts at once. A lane is a Claude Code config folder of its own (`~/.claude-lanes/<name>`) signed in to another account. Settings, skills, plugins and history stay shared with your main `~/.claude`; the login, background host and sessions are the lane's own.

**How to use.** You rarely manage lanes directly. Picking an account in New Session, or in a session's account menu, opens a lane when one is needed. An account is signed in to in only one place at a time.

**Use it when** one account's limit isn't enough for the sessions you run in parallel.

---

## Artifacts

**What it is.** A local copy of every Artifact (claude.ai page) Claude Code publishes on this Mac, so a page stays readable after you switch accounts. claude.ai only shows a page to the account that published it.

**How to use.** Chat sidebar > **Artifacts** opens the library: search and filter by account, read in place, and see which account owns a link. Artifact links in a conversation open beside the chat. Agents find pages with `Notchling --page <link | id | words>` and the `notchling-pages` skill. A publish to a page another saved account owns is sent with that account's login.

**Use it when** you published reports or dashboards from several accounts and want them all in one place.
