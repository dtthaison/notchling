# Privacy policy

What data leaves a computer running Notchling: where it goes, when, and why. It was checked against the
code on 2026-11-07: the Swift app in `legacy/macos/Sources/` and the phone app in `apps/flutter/`. Changing this file is a
protected change: it waits for the maintainer. A pull request that adds a network call updates this
page in the same pull request.

## In short

- Notchling has no server. It sends nothing to the project or its maintainer, unless you turn on the tunnel and leave its default hostname and team, which are the maintainer's (see Tunnel below).
- There is **no telemetry today**: no analytics, no crash reports, no usage statistics and no update
  check. The Swift app has no third-party packages.
- Its own calls go only to services you already use, with your own logins, for the features listed
  below: Anthropic, OpenAI (ChatGPT), GitHub and, if you set up the tunnel, Cloudflare. It also
  reaches the machines you list in Machines, over `ssh`. Web pages you open load what they link to.
- The phone bridge and the internet tunnel are off until you turn them on.
- The tools Notchling starts for you (Claude Code, Codex, Antigravity CLI, `ssh`, `gh`) send what they
  always send.
- The skills and rules Notchling adds to Claude Code, Codex and Antigravity go to those tools'
  providers in each session's prompt. The vault skill names your saved secrets, never their values.

## Claude accounts (Anthropic)

Every request below carries `User-Agent: Notchling`. The two `GET`s carry that account's access token.

| Request | When | Why |
| --- | --- | --- |
| `GET https://api.anthropic.com/api/oauth/profile` | When Claude Code is signed in with a login Notchling hasn't matched to an account yet; after you add an account; when a lane's login is copied back to its saved account. | To learn which account a login belongs to. |
| `GET https://api.anthropic.com/api/oauth/usage` | For each saved account, at most every 5 minutes (checked once a minute), and when you press the refresh button in Accounts. | Limit bars, limit warnings and switching accounts at a limit. |
| `POST https://platform.claude.com/v1/oauth/token` | When a saved account's token has expired and Notchling needs it: to read its limits, to open a lane, or to publish as a page's owner. It sends the saved refresh token. | To renew a saved login. Never the login Claude Code or a lane is using. |

**Adding an account** opens `https://claude.com/cai/oauth/authorize` in your browser. The browser sends
the sign-in code back to a listener on `127.0.0.1`, and Notchling exchanges it at
`https://platform.claude.com/v1/oauth/token`, the way `claude auth login` does.

**Claude Desktop** switching sends nothing: it swaps the app's cookie file on this Mac.

## Codex accounts (OpenAI)

| Request | When | Why |
| --- | --- | --- |
| `GET https://chatgpt.com/backend-api/wham/usage` | For each saved Codex account whose login can be read, at most every 2 minutes, and right away when you add one, press the refresh button in Accounts, or open the Codex accounts in Chat. It sends that account's access token and its ChatGPT account id. | Codex limit bars and limit watching. |

Notchling never renews a Codex token.

## Antigravity

Nothing leaves the Mac. Limits and plan come from Antigravity's own language server on `127.0.0.1`,
and only while Antigravity runs. Switching accounts swaps Keychain items and files on this Mac.

## Artifact pages published as their owner

claude.ai lets only a page's owner, or an editor in its organization, publish to it. When **Keep local
copies of artifacts** is on (the default), Notchling's hook can publish for the owner:

- **When:** a Claude Code session publishes an HTML file to a page that Notchling's local library
  says another saved account owns. Nothing else triggers it.
- **What:** `GET https://api.anthropic.com/api/frame/perm/<page>` to read the live version, then
  `POST https://api.anthropic.com/api/frame/deploy/direct` with the file's content, its title, and the
  publish's description and label. Both use the owner's saved login.
- **Why:** so a session signed in to one account can update a page another of your accounts made.

The rest of the hook sends nothing. It talks to the app only through files on this Mac.

## The phone bridge

Off by default. **Let phones connect** in the Phone panel turns it on.

- It listens on port 47821 with TLS, and announces itself on the local network with Bonjour
  (`_agentconsole._tcp`): the Mac's name and the start of its certificate fingerprint.
- It closes any connection that isn't from this Mac, the private LAN, a link-local address or
  Tailscale.
- Only a paired phone gets anything. A phone pairs with a QR code or one-time code shown on the Mac.
  The Mac keeps only hashes of the phones' tokens.
- A paired phone gets: the session list with each session's activity, context use and the account's
  limits; conversations, including tool output and diffs; terminal screens; slash commands; and
  pictures of web pages the Mac renders. Those pages can only be `localhost` or `127.0.0.1`, and are
  loaded with no stored cookies.
- The phone can send messages, answers, images and new sessions to the Mac.
- A paired phone that asks for the inbox gets each item's title and detail. For a vault item that is
  the secret's name, the command that wants it (cut to 120 characters) and its folder. The phone can
  answer those items only while the bridge, **acting** and the vault are all on. It can allow a
  secret's use (Allow Once or Always Allow), deny it, or cancel a request; the first answer, from the
  Mac or a phone, wins. It never opens the form that takes a secret's value. Pairing is a file on the
  Mac that an agent with a shell could write, so a forged device could answer too; the maintainer
  accepted that risk until pairing has an anchor an agent can't write.
- Every remote change is logged on the Mac as an event (Audit and Activity), with the device's id and the
  length of any text sent, never the text itself.

## The internet tunnel (Cloudflare)

Off by default. **Reach this Mac over the internet** in the Phone panel turns it on, and needs a
Cloudflare Tunnel and Cloudflare Access that you set up.

- Notchling doesn't run `cloudflared`; you do. Notchling listens for it on `127.0.0.1:47822`.
- With the tunnel on, everything the bridge sends to a phone through it passes through Cloudflare.
  Cloudflare ends the TLS connection for your hostname, and the bridge adds no encryption of its own,
  so Cloudflare's network can read that traffic.
- When a phone connects through the tunnel, the Mac fetches your Access team's public signing keys
  from `https://<your team>/cdn-cgi/access/certs`: at most once an hour, and for an unknown key at most
  once every 30 seconds.
- The Access client id and secret are stored on the Mac in `bridge/access.json` (mode 0600), and go
  to the phone inside the pairing QR code.
- The hostname and team fields start filled in with the maintainer's own. Replace them with yours.

## Tool versions on this Mac

Settings > Agents shows the newest version of each tool, so it can tell you when the Mac is behind.

- **When:** when Settings > Agents opens, and on Check for Updates, at most once an hour for each tool
  (a failed read is tried again after 5 minutes). Never in the background.
- **What:** `curl` fetches one public file over https, with no cookie, no account, no login and no
  identifier: `https://downloads.claude.ai/claude-code-releases/latest` (Claude Code),
  `https://registry.npmjs.org/@openai/codex/latest` (Codex) and Antigravity's own release manifest, the
  URL its installer reads (`https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests/<platform>.json`).
  Your Mac's operating system and architecture choose the Antigravity file name. Notchling's own update
  check is separate and unchanged.
- **Why:** to show "Latest" next to the installed version, and to offer Update.
- The Update and Go Back buttons run the tool's own update command on this Mac (`claude update`,
  `npm install -g`, `brew upgrade --cask`, Codex's installer, `agy update`), only while **Install** is on,
  and only after you click. The tool then talks to its vendor as it always does.
- The check that follows an update runs the tool's `--version`, `claude doctor` or `codex exec --help` and
  reads transcripts already on this Mac. It starts no session, sends no prompt and writes nothing to the
  tools' folders.

## Machines

Only when `machines.json` lists a machine:

- Notchling runs `ssh` to each listed machine about once a minute to read the worker's status, and runs
  slower checks when you open the window, press Check or run a fix, and every 10 minutes.
- A secret from the vault goes to a machine only as `ssh`'s standard input, written into a mode-600
  file there. It is never in a command line, a log or a check result.
- To check the GitHub issues token, it calls `https://api.github.com` with that token, and nothing
  else: requests and redirects to any other host are refused.
- It runs `gh` with your own GitHub login, to list repositories and issues, act on issues for Morning
  Actions, and manage deploy keys.
- A fix runs on the machine only after you click and see its commands. Some fixes download installers
  there (Homebrew's tools, the GitHub CLI, Claude Code).

## Tools Notchling starts

- **Claude Code.** Replies, answers, New Session and moves run `claude` (`claude attach`,
  `claude --bg` and others). Claude Code sends your prompts to Anthropic as it always does.
- **Codex.** A message in a Codex session runs `codex exec`, which sends the prompt to OpenAI. Commands
  Codex runs in that turn have network access unless you turn off **Allow Network in Codex Turns**.
  Adding a Codex account runs `codex login`.
- **Antigravity CLI.** A message in an Antigravity session goes to `agy acp`, which sends it to Google.
- **Delegation.** When an agent runs `Notchling --delegate`, Notchling runs Codex or Antigravity CLI
  with that prompt.
- **The vault.** `Notchling --secret … -- <command>` runs a command an agent chose with the secret in
  its environment. Where that command sends it is up to the command; Ask First is for entries you
  want to approve each time.

## Instructions in agent tools

Notchling writes its skills and rules into Claude Code's, Codex's and Antigravity's folders on this
computer ([decision 0019](../decisions/0019-agent-instructions-per-tool.md)) and sends them nowhere
itself. Each tool puts them into a session's prompt (a rule in every session, a skill when a task
calls for it), so their text goes to that tool's provider with your prompts, as your own instructions
do. Today these are the vault, pages and delegate skills; the delegation and model-choice rules and
Settings > Agents, which shows every text and turns each off per tool, come with that decision's
phases.

- **The vault skill** lists your saved secrets' names, and the host each one is for, in its
  description, never their values. Those names reach the provider of each tool where the skill is
  installed.
- The other texts hold the path of Notchling's command and model names: no login, account or
  content of your files.
- **Your own rules.** To tell whether you have your own model or delegation rules, Notchling reads
  the names of the files in those folders and the headings of `~/.codex/AGENTS.md` and
  `~/.gemini/GEMINI.md`. Settings shows such a file's text when you open it; it goes into no file,
  log or message Notchling writes.

## Web views

A saved Artifact page or an HTML file you open in Chat renders from the copy on disk. Like a browser,
the web view loads whatever the page links to, such as scripts, fonts and images from other sites.
Links you click open in your browser.

## The phone app

- It connects only to the Mac it paired with: on the LAN to that Mac's pinned certificate, or through
  the tunnel URL from the pairing, with the Access headers. On the LAN it never sends Access headers.
- Voice dictation runs on the phone with a Whisper model bundled in the app.
- None of its direct dependencies is an analytics or crash-reporting package (`apps/flutter/pubspec.yaml`).

## Logs on this Mac

Notchling writes these files, in `~/Library/Application Support/Notchling` unless noted. None leaves the
Mac, and Settings > Activity reads the Activity store.

- `audit/YYYY-MM.jsonl`: the Audit log, written only by the daemon. Each line is a registered kind of event
  (`docs/log-kinds.md` lists them all, with what each records and why) with fixed fields: choices, flags,
  counts, sizes, times, names of ops and errors, ids. No field holds a message, a typed text, a free-text error,
  a path in an error or a secret's value; a vault use records the secret's name. Each line is sealed with a
  keyed hash over the previous line's seal; the key is a Keychain item (`dev.notchling.audit`) and so is the
  newest seal, so an edited or cut line is found. If the key can't be read the lines are written unsealed and
  the Inbox says so. A month's file is kept a year by default (90 days or 2 years are the other choices), the
  folder is capped at 500 MB, and a month is pruned only after a seal that says where the chain went on.
  `audit/legacy/` holds the old `audit.jsonl` files, moved, never rewritten.
- `daemon/events.db`: the Activity store, the same events in a table, with rows that expire by kind.
- `notchlingd.log` (and `.1`, rotated at 5 MB): the daemon's output.
- `AgentConsole/bridge/remote-log.jsonl`: written only when the daemon is down and a phone acts: the phone's
  name, the operation and the length of any text, never the text. The daemon imports it when it starts. While
  the daemon runs, a phone's changes are events with the device's id, not its name or address.
- `account-log.json`: the old record of which account was active, read once into the Activity store and not
  written any more (an account switch is now an `account.switched` event with the account ids and why).
- `logs/crash.log`: a panic's message and place (opened 0600; a message can quote what was on screen).
- `permission-screen.txt`: the last permission screen the daemon could not read.
- `reply-screen.txt`: the last reply the daemon could not type into a session (opened 0600): the job's id, why
  it failed, the message's length, what each step saw and the terminal screen it ended on. The text in the
  session's input box is masked, so neither the message nor a draft is written; the rest of the screen is as
  the terminal showed it.

## What stays on the Mac

Transcripts, conversations and search are read from files on this Mac and stay there, except what a
paired phone is shown. Cost tracking, the search index, the local copies of Artifacts and the vault
list are local files. Vault values are in the Keychain, and agents reach them only through a socket
on this Mac that answers only Notchling's own executable. A use of a secret is allowed from the
Notchling app on the Mac, or from a paired phone through the inbox while the bridge, acting and the
vault are on; the value itself never leaves the Mac.

## Planned

None of these exist yet. Each will change this page when it lands.

- **Crash reports**, opt-in only. Stable updates become automatic only after this privacy policy and
  opt-in, privacy-respecting crash reports exist.
- **Update checks** (Phase 6): the app will read one signed update feed with Beta and Stable channels.
- **Remote computers:** the desktop app and the phone will connect to other computers' Notchling the
  way the phone connects today, with the same pairing. Accounts, lanes and the vault never leave
  their computer.
- **Tool versions** (see decision XXXX, tool versions follow the Mac). The Mac will read what the
  latest version of each tool is, on demand and at most hourly, sending nothing that identifies you:
  Claude Code's from `downloads.claude.ai`, Codex's from `registry.npmjs.org`, and Antigravity's from
  the manifest host its own installer reads, from which it also fetches the binaries it caches. It
  will read Notchling's release manifest from GitHub. Machines' workers will download their updates
  from those same vendor hosts and from GitHub, over https, each checked against a checksum, beside
  the installers Machines already lists.

## Keeping this page true

The capability check (`CAP-NETWORK`, [docs/rules.md](../rules.md)) sends a pull request that adds a
new `URLSession`, `NWConnection` or other network client use to the maintainer, wherever the file is.
That pull request updates this page. The check catches new network code; it doesn't prove this page
is complete, so a reviewer compares the two.
