# Changelog

Notable changes to Notchling. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [2.0.2] - 2026-11-28

### Added

- Cost Profile picks for the code and fix roles can route work to Antigravity or Codex, updating the generated Claude Code rule and `notchling delegate-models --for`.
- Keep Finished Sessions in Settings > Chat lets you keep finished sessions in Chat's Done list and the notch's Completed tab for 1, 7 or 30 days.
- Ended Claude Code sessions on this Mac can now be resumed as background sessions from Chat and with `notchling session resume`.

### Fixed

- A message sent from Chat to a background session no longer waits forever under "Claude is replying" after the session has finished its turn.

## [2.0.1] - 2026-11-27

### Fixed

- The 2.0.0 release was built but never published, because its upload step failed. 2.0.1 is the same app, published.

## [2.0.0] - 2026-11-27

Notchling 2.0 is a new app, written in Rust, and it replaces the Swift app, which is retired. It is for Macs with Apple silicon, and it is ad hoc signed, not notarized (macOS asks you to allow it once). The big new thing is the Cluster: your own Macs and Linux machines join one cluster, and you see and answer their sessions from one place.

- Cluster: your machines join one cluster over Tailscale, and sessions on other machines show in Chat with their replies, answers and prompts.
- Add Machine over SSH installs Notchling on a Mac or Linux machine and joins it; Machines still runs your overnight workers.
- Ask: an agent can put a question to you with `notchling ask`, and your answer goes straight back to it.
- The Inbox: a bell in Chat collects what needs you, from every machine, and answering anywhere answers once.
- Standby snapshots keep a copy of your sessions on another Mac. Moving the primary to it (Move to <machine> Now) is built and stays off until the resume proof is recorded; failover is by hand.
- The Secret Vault reaches your Workers: jobs there use the secrets you allow, and only the Mac that holds the vault hands them out.
- Git branches in Chat's Details Panel: every worktree of the repository, with a Branches panel for what is working, not merged or merged.
- Chat: long sessions with pictures read fully again, and messages you send are kept and go in as soon as the session can take them.

### Added

- Search covers everything on disk: sessions and saved pages, not only the last 14 days. Settings > Search sets how far back sessions are indexed (14 days, 30 days or everything), which kinds of results show and in what order.
- Search filters: type `tool:`, `project:`, `account:`, `owner:`, `section:` or `state:` with a value, or a day or month, and the results narrow. Tab completes the value.
- Search filter chips for projects: two projects with the same folder name show their parent folder, and `project:b/app` picks one.
- Semantic Search, off by default (Settings > Search): finds sessions and pages by meaning as well as by words, with models you download on your click and keep on this Mac. Hits found only by meaning are tagged Related. Indexing runs in the background at half the CPU on power and a quarter on battery, and pauses on Low Power Mode or a hot Mac.
- Agents can search too: `notchling search`, `notchling facets` and `notchling semantic` read what the agent's own organization has from the last 14 days, with emails and tokens masked. An agent that wants a model asks for it on a card; nothing downloads without your yes.
- Clickable buttons show the pointing hand, and a disabled one keeps the arrow.
- Organizations: Settings > Organizations groups your accounts, machines and secrets. Everything starts in Personal.
- Models & Costs: one table of which model and effort each kind of work gets, with Set Up for Claude Code, Codex and Antigravity (a plan first, a backup of every file it changes, and Undo).
- `notchling models` and Refresh Models read each tool's current models, efforts and prices.
- Claude in Chrome is supported, and `/chrome` works from Chat.
- Antigravity's app sessions show in Chat, and Settings > Agents has a tab for each tool.
- Add Machine > From Tailscale… lists the machines on your tailnet, and a machine can have several addresses.
- Show in Dock (Settings) keeps the Notchling icon in the Dock; it is on by default.
- Phone chat: a reply of up to 100,000 characters reaches the phone whole, and a longer one is cut with a note instead of dropped. The phone replies with a picture from the picture viewer, and a marked-up picture is shrunk to fit the upload limit. On Android the page slides up with the keyboard.
- After an update, the phone fetches again the chats it had cut before.
- Tool versions, read only: Settings > Agents shows where the Mac's Claude Code was found, its version and how it was installed, and each worker's version against the Mac's. Machines gets a Tool Versions row on every card (same, behind, ahead, or not installed) and says when Notchling isn't running. Nothing is installed or changed on a worker.
- Tool versions, checked: a new Claude Code, Codex or Antigravity version on the Mac is checked by itself with files and flags (its version, `claude doctor`, Codex's flags and a count of transcript lines the readers can't place). The result says Partly Checked and what was not checked; workers compare with the last version that passed, and a failed version raises a notice in the inbox. Settings > Agents shows the newest version out, Update (with the Install switch on), Check Again, the soak line and Go Back to the previous checked version.
- Tool versions, workers stay: Machines gets a Self-Update Off step for each tool a worker has. One click, after a plan that shows the exact lines, turns off Claude Code's and Codex's own updating on that worker (a backup is kept beside the file, and the Mac's own settings are never written); Self-Update On takes back only what Notchling set, and the Remove alert says what is still off. The sd service files carry the same line. A worker running another version than the Mac's checked one raises a notice in the inbox and shows "Drifted" with both versions on its Tool Versions row; nothing is moved by it.
- Cluster: your own machines can join one cluster over Tailscale (Chat's footer, Cluster). Create it on one Mac, copy a join token, paste it on another machine and Allow it in the Inbox; every link is encrypted and pinned to the machine's certificate. The sheet shows each machine's role (Primary, Standby, Worker), whether it is online, its versions and load, with Can Be Master, Acting and Remove in a row's menu. Machines can add a machine as a node.
- Sessions on other machines show in Chat's sidebar with the machine's name, open with their transcript, and take your replies, answers and permission decisions; they show on the phone with the machine's name too. A machine that is offline dims its sessions and says so in the reply box.
- Run On in New Session starts a Claude Code session or a shell job on a Worker. Use a folder there, or send this repository (its unpushed commits go by git, in a branch on the Worker), and bring the work back later from the row's menu (Bring Back Branch) as a branch that is never merged for you. A shell job on a Worker shows in Chat while it runs, with its log and Stop. Codex, Antigravity and secrets on another machine come in a later version. With Allow Acting off, Stop on a Worker's shell job and Run On are refused too.
- `notchling delegate --machine <name>` hands a job to another machine's Antigravity and waits for its result.
- Automatic Failover isn't offered in v1: failover is manual. Settings > Cluster, on a Mac in a cluster, shows the Automatic Failover row saying "Not available yet" and pointing to Move to <name> Now in the cluster view and the phone's Machines popup, with no choice to make. A Standby never takes over by itself and the primary never pauses sessions ahead of a deadline. See [docs/cluster-failover.md](docs/cluster-failover.md).
- The fence hook: Install Hook also adds a hook before every tool call (`notchling hook --fence`). The daemon now writes the fence file it reads (`cluster/fence.json`), so a session that moved to another machine is denied its next tool call on the old one ("This session continues on m1"), and a Codex turn on it is interrupted. Outside a cluster the hook always lets the call through. The pause before a failover deadline isn't built yet.
- Move the primary when it can't be reached: with m5 gone, the phone's Machines popup ("Choose a Machine") and the app's Move to <name> Now on a Standby's row promote that Standby from its last set. If another machine still reaches m5 nothing moves; if none does, you confirm the risk first (Move Anyway). It is a move you start; failover itself isn't acting yet, and a move that would resume sessions is refused until the resume proof is recorded.
- Take Back and side copies: when the old primary comes back it steps down and offers Take Back or Keep m1 in its inbox; its next tool call in a moved session is denied. Work it did after the snapshot stays on it as a side copy, listed in the cluster view (Open, Hide) and never deleted or merged. A session waiting for a newer copy, with Continue From This Copy, is built but nothing raises it yet.
- Install Hook for a headless Standby: from the primary's cluster view, a node's menu now has Install Hook, which shows what will change on that machine first and installs only after you confirm, and only if that machine's Install switch is on (Turn Install On is in the same menu).
- Standby snapshots: the primary of a cluster now sends a set, a copy of its Claude Code and Codex sessions, to each Can Be Master machine at every turn end. A set holds their transcripts and session files, the inbox, the paired phones and saved pages, never your code. A machine gets sets only once it is ready: disk encryption on, the same home folder and user, enough free space, and the same Claude Code and Codex versions. The cluster view shows whether each one is ready, the age and size of its last set, and when it is behind. To leave a session out, check Keep on <machine> in its right-click menu.
- Move to <machine> Now, in a ready Standby's menu, hands the primary over to it. Sessions whose code is committed and pushed continue there; the rest keep running where they are. The sheet shows each step. It stays off until the resume proof is recorded.
- Phone continuity: when your Mac can't be reached, the phone connects on the LAN to another Can Be Master machine of its cluster, but only to the certificate your Mac listed. That machine passes your messages, answers and new sessions on to the primary. If the primary can't be reached either, it holds them and sends each one once when the primary is back. Held commands show on the phone with Cancel, and one held for over an hour asks "Still send?".
- Cluster secrets: a job on a Worker can use the vault's secrets you attach to it, or ask for one by name. Each machine has its own list of allowed names (empty by default) and an Any Secret switch (off by default), set from Secrets… on its row in the cluster sheet. Only the Mac whose Keychain holds the vault hands a secret out, and only while it is the primary. If another machine is primary, a dispatch that needs a secret is refused and says which machine holds the vault, so dispatch it again once that machine is primary. An entry marked Ask me first still waits for an Allow on the Mac or the phone. The value reaches the Worker as a private file that expires after 12 hours at the latest. It is deleted when a shell job ends. A value that arrives before its job starts waits up to 8.5 minutes. A Worker that is cut off from the cluster keeps the file until it expires. Notchling hides every value that is live on the Worker in a job's output, in what other machines read of it and in a delegate job's reply, but not everywhere: unusual encodings of a value, a repository change a job returns, and a Claude job's output just after a restart are not fully covered. Jobs on a Worker run as the same user, so one job can read another job's secret file. Notchling stops the value coming back to another machine, it does not stop that read. No screen sends a secret list with a dispatch yet: today the feature is reached through `notchling secret` inside a shell job on a Worker, or through the local API. Claude and Antigravity jobs on a Worker can't use secrets yet.
- Machines: the Cluster sheet is now the Machines sheet (Chat's footer, Machines). The cluster is its own section at the top; every machine saved in Machines that isn't in the cluster is listed under Not in the Cluster with its address, system, whether the last check reached it and when, a Check button, Join Cluster… and a menu with Remove from Machines…. Add Machine… at the bottom checks a machine over SSH and only saves it to Machines: nothing is installed on it and it doesn't join the cluster until you choose Join Cluster… on it. A member's menu has Leave Cluster….
- Add Machine over SSH: the Cluster sheet's Add Machine takes `user@host`, checks the machine and shows a plan (Worker user, Stays Awake, Git, Tailscale, Notchling installed, the service, Reaches the Primary, Joined), each step with its commands shown first. Install and Join runs it over ssh and the machine joins the cluster with no Add click: Notchling makes a one-use invite for that machine alone, which lasts 10 minutes, and the Inbox says it joined. A Mac runs the headless daemon as a system service as your user, with no app, so it stays online with nobody logged in; a Linux machine runs a user service with linger on, and Tailscale can be installed and signed in from the plan. Can Be Master is on for an added Mac only with FileVault on and the same user and home as the primary, and Acting is on by default as a visible choice. Join Cluster on a machine already in Machines, such as an overnight worker, checks and runs the node steps only and doesn't touch the worker. The Cluster sheet lists the machines in Machines that haven't joined under Not in the Cluster. When this Mac's build differs from a machine's, Update N Machines updates them one at a time (a machine running a shell job asks first). The app gets the Linux build from the release and checks it against the release's `manifest.json` (SHA-256) before it installs it; `tools/build-node.sh` is for development builds. Taking a machine out is by hand: see docs/notes/cluster.md.
- Cluster inbox: items from every machine (a secret approval on a Worker, a sign-in notice) show in one list on the primary and on every Standby, and answering one anywhere answers it once. The vault list of another machine shows as a Vault on Another Machine tab in the Secret Vault.
- Cluster logins: Workers report whether Codex and Antigravity are signed in, shown in each node row (Signed In, Signed Out, Not Installed), and a signed-out tool is refused at dispatch with a notice that says what to run on that machine. Nothing in the cluster renews a login. Sending Claude Code's long-lived login to Workers is built but switched off in this version.
- A notifications inbox: a bell in Chat (and a badge in the open notch) collects what needs you, such as a vault approval, a secret an agent asked for, or a limit notice. Answer from Chat or from the phone, and get a system notification when something new arrives.
- Agents can ask you a question with `notchling ask`. It shows as a card under its session in Chat, in the notch and in the Inbox, also on the phone, naming the agent and its branch, and your answer goes straight back to the agent that asked.
- The Details Panel's Git section now shows every worktree of the session's repository, one line each: its branch, how big its work is, and a dot where a session or subagent is working. A last row, "main and 39 branches", opens a new read-only Branches panel with Working, Not Merged and Merged tabs, a filter, and Changes, Open Session and Show in Finder on each row. Every number is against the branch's nearest base, so a lane is shown against the branch it was cut from, not against the whole history of `main`, and Changes opens against that base too. Nothing fetches: remote branches are as fresh as the last fetch, and the panel says when that was.
- Messages you send to a session are saved first and go in as soon as the session can take them, even mid-turn, in order. One waiting in the session's own queue shows as Queued in the session until the model reads it. A message that may already be in the session asks before sending again. Anything shown as Queued just before this update may need sending again.

### Fixed

- A session that can't wake (its conversation is open in another terminal or in Claude Desktop) now says so in Chat, with Claude Code's reason, and the message is tried again.
- Sessions that exited are kept for 24 hours across a restart of the daemon. A finished job no longer hides a later exit. A session that ends while the daemon is down is not recorded.
- Codex's own failure reason, such as a usage limit, is no longer replaced by a later "Error: turn failed" line. A Codex installed under nvm is found.
- A long reply from Claude Code (over 100 KB, and over 1 MB) shows in Chat and on the phone instead of being dropped or filling memory. An escaped or duplicate `type` on a line over 100 KB is still skipped.
- Search: turning sessions or pages off and on quickly no longer leaves them empty, a failed clear is tried again, and an agent can no longer confirm a guessed email by filtering on it.
- The cluster sheet showed Antigravity as Not Installed on a machine that has it, because the version check only read the old `agy 1.4.2` form and agy 1.5.9 prints just `1.5.9`. It also showed Claude Code as Unknown on a machine where you are signed in; it now says Signed In when that machine has its own Claude Code login, and shows the cluster's Worker Login (On or Off) apart from it on machines that run jobs.
- Clicking an `.html` file in Chat (a link or path in a message, or a new file in the Details Panel's Git section) now shows it as a page, with scripts off, instead of its source. The header's Page and Source buttons switch between them, and a changed file's diff still opens first, with the page one click away.
- The Artifact pages library saves again. Since the Rust app replaced the Swift one, published pages were no longer copied to `~/Library/Application Support/Notchling/pages`, reads of a saved link no longer came from the local copy, and a failed read no longer pointed at it. `notchling hook` now handles the Artifact tool's events itself, with or without the app running.
- A `!` command that waits (a login that prints a link and waits for the browser) showed no output in Chat until it ended, because Claude Code writes the output to the transcript only then. For a background job, Chat now shows what the command has printed under its row while it runs, with links you can click, and the transcript's output replaces it when the command ends.
- A background shell stopped with `TaskStop` or `KillShell` now shows as Stopped in Chat's Details Panel instead of being counted as still running.
- Stop on a background shell (a pipeline such as `( tail -f log & wait ) | grep -m1 done` included) ended nothing and said "Couldn't stop everything" for Claude Code sessions that run as `claude bg-spare` or on a Mac outside UTC: Notchling didn't recognise the session's process, so it treated the shell as an orphan whose parent was alive and left it alone. It now finds the session's process by its title and by the UTC start time Claude Code records, and Stop ends every process of the shell.
- Chat showed almost nothing of a long session that had read pictures (a Read of a .jpg writes megabytes of base64 into the transcript, which filled the part Chat reads). Pictures no longer count toward what is read, so the conversation before them is back, the Read steps finish instead of showing as running, and the notch and Search read around them too.
- A reply you sent from Chat that Claude Code had already taken no longer stays shown as Sending, Queued or Needs You. Notchling now finds the take when the transcript is full of pictures or big tool results, when the text starts with a tag or is a long paste, and when the session was continued in another folder or by `/clear`.
- The Codex tab in Accounts now shows what is left, as ChatGPT's usage page does: a 5-hour limit that is used up reads "0% left" and a week that is a third used reads "67% left", where the card used to say "100%" and "33%". The bar drains as you use it and turns red when little is left. Claude Code, Claude Desktop and Antigravity still show the share used. When ChatGPT has refused Codex because the workspace is out of credits, the card now says so under the bars, and shows a credit balance when the account has one.
- Codex sessions you started yourself no longer drop out of Chat, the notch and the phone's list when many sessions made by `notchling delegate` are newer than them. Jobs handed to Codex are no longer counted against the 30 rows shown.
- Chat's Details Panel showed a background agent as Completed after Claude sent it a follow-up message to bring it back, so a job with an agent at work listed none running. It now shows Running until the agent reports again. A command that Claude Code moved to the background after its timeout is now listed under Shells too.
- Keep Mac Awake works in the new app again: the Mac stays awake while agents work, or always, as you chose, and the menu bar choices are saved.
- Keep Mac Awake now matches the old app: the Mac is held awake only while Notchling is open, the menu bar cup is filled only while it is really held, and "Continue agents after sleep" works again, telling background agents whose reply the sleep cut off to carry on.
- Refresh in Accounts now fixes "Couldn't read limits. Its login has expired" on a Claude Code account in a lane or the main login. When no Claude Code session is running there, Refresh asks Claude Code itself to renew the login (`claude auth status`, no prompt and no model use), shows "Renewing login…" on the card, then reads the limits again. If Claude Code can't renew it, the card says to sign in again from the ⋯ menu. A cost of zero no longer reads "$-0.00".
- A background session whose terminal box held leftover text could no longer be replied to from Chat. A message Notchling typed earlier and didn't send is now cleared before the next one goes in. Any other text in the box is shown above the reply box with a Clear Box and Send button, so you can always send.

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

[Unreleased]: https://github.com/dtthaison/notchling/compare/v2.0.2...HEAD
[2.0.2]: https://github.com/dtthaison/notchling/compare/v2.0.1...v2.0.2
[2.0.1]: https://github.com/dtthaison/notchling/compare/v2.0.0...v2.0.1
[2.0.0]: https://github.com/dtthaison/notchling/compare/v1.2.0...v2.0.0
[1.2.0]: https://github.com/dtthaison/notchling/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/dtthaison/notchling/releases/tag/v1.1.0
