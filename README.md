# Notchling

[![Tests](https://github.com/dtthaison/notchling/actions/workflows/rust.yml/badge.svg)](https://github.com/dtthaison/notchling/actions/workflows/rust.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

A macOS menu bar app for people who run several coding agents at once. It shows every Claude Code, Codex and Antigravity session in the camera notch and in one Chat window, keeps the Mac awake while agents work, and switches accounts when a limit runs out. You can read a session, answer its questions and permission prompts, and reply to it without hunting for the right terminal.

Notchling is free and open source, and isn't affiliated with Anthropic, OpenAI or Google.

Notchling 2.0 is written in Rust and replaces the Swift app, which is retired. It is for Macs with Apple silicon. [CHANGELOG.md](CHANGELOG.md) says what is new.

**Some parts are off until you turn them on.** The daemon starts with its switches off, every time: **Act on Sessions**, **Phone Bridge**, **Logins and Lanes**, **Secret Vault**, **Install Hooks and Skills** and the cluster. The first five are in Settings, in the Daemon section; the cluster has a **Turn On** button in the Machines sheet. A feature below says which switch it needs.

## What it does

### The notch

The notch lists your sessions in three tabs: **Input** (waiting on you), **Working** and **Done**. When an agent needs you or finishes, the notch peeks open for a moment. A small pet in the notch moves while agents work; **Animate pet** in Settings turns it off. Settings also has **Show agents in the notch** and the notch's width.

Questions and permission prompts can be answered from the notch. That needs **Act on Sessions** and **Install Hooks and Skills** on, and **Answer prompts from the notch** (a setting that starts on).

### Chat

Chat is the full-size window for a session. Open it from the menu bar cup with **Open Chat** (⌘O).

- **Sidebar.** Your sessions, with Claude Code, Codex and Antigravity side by side. **New Session** starts one on an account and in a folder you pick. Sessions on other machines of your cluster show here too, with the machine's name.
- **The conversation.** Messages as Markdown, tool steps folded into one line each, file edits as diffs, and the model and effort pickers under the reply box.
- **Details Panel.** On the right: **Tasks**, **Session**, **Subagents**, **Shells** and **Git**. The Git section lists every worktree of the session's repository. A click opens **Changes**, and **Branches** sorts the repository's branches into **Working**, **Not Merged** and **Merged**.
- **Second Opinion.** Ask Codex or Antigravity to review an edit or a reply.
- The footer of the Sidebar holds the buttons for Settings, Accounts, the Secret Vault and the Cluster. The cup in the menu bar has **Settings…** (⌘,) too.
- **Search.** ⌃⌥P opens a search box from any app (change it in Settings under **Search Shortcut**, or turn it off). It finds saved Artifact pages and conversations.

### Replying

The reply box types into the session through `claude attach`, so it needs **Act on Sessions** on. **Return** sends; Option-Return or Shift-Return adds a line. A message you send while the session is working is kept as **Queued**, and goes in as soon as the session can take it. **Edit** takes your newest queued message back into the box. Stop (or Esc) ends the turn. A reply is never typed into an open question or permission dialog: Chat shows that dialog as a card to answer.

### Ask and the Inbox

An agent can put a question to you with `notchling ask`. It shows as a card under the session in Chat and in the notch, and your answer goes straight back to the waiting command, so the agent spends no tokens while it waits.

The bell in Chat is the **Inbox**: what needs you, in one list. It holds questions, vault approvals, limit notices and messages that didn't go in. In a cluster it collects items from every machine, and answering one anywhere answers it once.

### Accounts and limits

Accounts for **Claude Code**, **Claude Desktop**, **Codex** and **Antigravity** are saved, and one click switches to another without signing in again. Each account shows its limits and when they reset. The Accounts window also has a **Token Cost** tab: what your use would cost at API prices.

Switching needs **Logins and Lanes** on. For the Claude Code account in use, Settings has a limit choice: **Off**, **Notify** or **Notify and Switch**, which moves to the saved account with the most room. Codex and Antigravity have their own limit choices. A **lane** is a Claude Code config folder signed in to another account, so sessions on two accounts can run at once.

Notchling never renews the token of the account in use, because the session holding the old one would be signed out. Claude Desktop and Antigravity read their login when they start, so switching them restarts them.

### Keep the Mac awake

The menu bar cup has **Keep Mac Awake**: **Off**, **Always On** or **While Agents Are Working**, and **Keep Display On**. The cup is filled while the Mac is being kept awake. Closing the lid still puts the Mac to sleep. With **Continue agents after sleep** on, background agents whose reply sleep cut off are told to continue.

### Secret Vault

Passwords and API tokens that agents can use without seeing. Values live in one Keychain item, and an agent runs a command with a secret in its environment (`notchling secret <name> -- <command>`); the value is hidden in the output. An entry can be set to **Ask me before an agent uses it**. An agent can ask you for a missing secret with `notchling secret-request`. It needs **Secret Vault** on, and the `notchling-vault` skill (installed with **Install Hooks and Skills**) teaches agents to use it.

### Delegation

`notchling delegate codex` and `notchling delegate agy` hand a job to another tool, wait for it and print the reply, with `--model` and `--effort` to pick them. A delegated job is read-only unless you pass `--write`. `notchling delegate-models` lists what each tool offers. `notchling delegate agy --machine <name>` runs it on another machine of your cluster.

### Cluster and Machines

Your own Macs and Linux machines can join one cluster over Tailscale, and you see and answer their sessions from one place. The Cluster button in Chat's Sidebar opens the sheet. The cluster is off until you press **Turn On** there, and you start one with **Create Cluster** or join one with a join token.

- **Add Machine…** takes `user@host`, checks the machine over ssh and shows a plan, each step with its commands first. Install and Join sets the machine up. A Mac runs the daemon as a system service as your user, with no app; a Linux machine runs a user service. For Linux, the app takes Notchling's Linux build from the release and checks it against the release's SHA-256 before it installs it. Machines is the list of machines Notchling can reach over ssh that Add Machine saves into.
- A machine's role is **Primary**, **Standby** or **Worker**. **Update N Machines** brings machines to this Mac's build, one at a time.
- **Run On** in New Session starts a Claude Code session or a shell job on a Worker, and you can bring its work back as a branch that is never merged for you.
- **Standby.** A machine marked **Can Be Master** that is ready (disk encryption on, the same user and home folder, enough space, the same tool versions) gets a copy of your Claude Code and Codex sessions at every turn end. It never holds your code. **Move to <machine> Now…** hands the primary over to it, and a Standby can take the primary from its last copy when the primary can't be reached. A move that would resume sessions is refused until the resume proof is recorded, so failover is by hand and isn't something to rely on yet. **Automatic Failover** isn't offered.
- Secrets reach a Worker for a job only if you allow them: each machine has a list of allowed names, empty at first.

### The phone app

`apps/flutter/` is a phone app that follows your sessions through the Mac, to read them, reply and answer prompts. You build it from source; it isn't part of this release. On the Mac it needs **Phone Bridge** on.

## Install

Download `Notchling-<version>.dmg` from the [latest release](https://github.com/dtthaison/notchling/releases/latest) and drag Notchling to Applications. It needs macOS 13 or later on a Mac with Apple silicon.

The app is ad hoc signed, not notarized, so macOS blocks the first launch. Open System Settings > Privacy & Security and choose **Open Anyway**, or run:

```sh
xattr -dr com.apple.quarantine /Applications/Notchling.app
```

Linux machines for the cluster need nothing downloaded by hand: **Add Machine** in the Machines sheet installs Notchling's Linux build on them, taken from the release and checked against its SHA-256.

## Requirements

- macOS 13 or later on Apple silicon. Designed around a notched MacBook screen. Without a notch it hangs under the menu bar.
- Claude Code installed at `~/.local/bin/claude` (or `/usr/local/bin/claude`).
- To build it yourself: Rust (the version in `rust-toolchain.toml`, installed by `rustup`).

## Build from source

```sh
tools/bundle-next.sh            # builds build/next/Notchling.app
tools/bundle-next.sh --install  # also quits the running copy and installs it to /Applications
```

`tools/bundle-next.sh` builds the app, its daemon and its terminal host with `cargo` and signs the app ad hoc. `--install` refuses to replace an app that is still the Swift one. Linux node binaries for development come from `tools/build-node.sh`.

To start it at login, add Notchling in System Settings > General > Login Items.

## Menu

Click the cup in the menu bar:

| Item | What it does |
|---|---|
| Claude Code | The account in use |
| Open Chat (⌘O) | The Chat window |
| Keep Mac Awake | Off, Always On, While Agents Are Working, and Keep Display On |
| Settings… (⌘,) | Accounts, token cost and every setting |
| About | About Notchling, and Welcome… |
| Update Available: <version>… | Only when a newer release is out; opens its page |
| Quit Notchling | |

## How it works

Notchling reads the files your agents already write, and talks to the Mac's daemon, `notchling daemon`, over a local socket. It sends no telemetry. With your own logins it asks Anthropic and OpenAI for account limits, and it reads one public file from GitHub to see whether a newer release is out. The phone bridge, the tunnel and the cluster make calls only when you turn them on. [docs/policies/privacy.md](docs/policies/privacy.md) lists every call.

**Update check.** Notchling checks for a newer release when it starts and then at most once a day, by reading the release feed on GitHub. When one is out, the menu bar cup gets an **Update Available** item and **About Notchling** shows the version, both linking to the release page. Nothing is downloaded or installed for you. Turn it off with **Check for updates automatically** in Settings, or by starting the app with `NOTCHLING_RELEASES=off`. A development build doesn't check.

**The hook.** **Install Hooks and Skills** adds Notchling's entries to `~/.claude/settings.json` (a backup is kept, and your other hooks stay). The hook never holds Claude Code up: with no app, no daemon or any error, it prints nothing and Claude Code asks in its own terminal. **Welcome…**, under **About** in the cup menu, shows what will change before anything does.

More: [docs/features.md](docs/features.md) is a guide to each feature, [docs/goals.md](docs/goals.md) says what Notchling will never do, and [ARCHITECTURE.md](ARCHITECTURE.md) says how it is built.

## Logs

Every log stays on your Mac. `Notchling` below is `~/Library/Application Support/Notchling`.

| File | What it holds | Size |
| --- | --- | --- |
| `Notchling/audit/YYYY-MM.jsonl` | The Audit log: what agents, the app and phones did that matters (a secret used, a setting changed, a session replied to, an account switched), one sealed line each. A line is a kind of event and its fields, never message text, and not the polls that succeed. | One file a month, kept a year, 500 MB at most |
| `Notchling/daemon/events.db` | The Activity store: the same events with their titles, for the Activity page. | Old rows expire by kind |
| `Notchling/notchlingd.log` (and `.1`) | The daemon's own output: start, errors, a missing price. | Rotates at 5 MB, one old copy |
| `AgentConsole/bridge/remote-log.jsonl` | Only if the daemon was down when a phone acted: the device, the operation, never a message. The daemon reads it in when it starts. | Grows rarely |
| `Notchling/account-log.json` | The old record of when each account became the active one. It is read once into the Activity store and no longer written. | Does not grow |
| `Notchling/logs/crash.log` | A panic's message and place. It can quote what the app was showing. | Grows only on a crash |
| `Notchling/permission-screen.txt` | The last permission prompt the daemon could not read. | One screen |
| `Notchling/reply-screen.txt` | The last reply the daemon could not type into a session: why, and the terminal screen it ended on. What the input box held is masked, so your message is not in it. | One screen |

**Settings > Activity** shows the Activity store, newest first: refusals and failures stand out, and you can filter by kind and search. Open a row for its event and outcome, and **Check Against the Audit Log** compares it with its sealed line. Each Audit line is sealed with a key kept in your Keychain, so a line that was edited or cut shows as broken; this is tamper-evident, not tamper-proof.

## Uninstall

1. Turn off **Install Hooks and Skills**, so Notchling's entries are removed from `~/.claude/settings.json`.
2. Quit Notchling, and remove it from System Settings > General > Login Items if you added it.
3. Delete `/Applications/Notchling.app` and `~/Library/Application Support/Notchling`.
4. To drop saved logins, delete the `Notchling Accounts` and `Notchling Antigravity` items in Keychain Access.
5. For a machine you added to a cluster, see [docs/notes/cluster.md](docs/notes/cluster.md) (Taking a machine out).

## Contributing

Bug reports, ideas and pull requests are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) covers building, the few rules around real logins, and what fits the project. Notchling is maintained in spare time, so expect a reply within about a week. Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

Found a security problem? Please report it privately as described in [SECURITY.md](SECURITY.md), not in a public issue.

## License

[MIT](LICENSE) © 2026 Son Doan and Notchling contributors.
