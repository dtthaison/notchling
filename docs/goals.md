# Goals

What Notchling is for, who it's for, and what it will never do. The source is the design,
[revision 2](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md),
which the maintainer approved on 2026-11-07. Changing this file is a protected change: it waits for
the maintainer.

## What Notchling is

Notchling is for people who run several coding agents at once. It shows every session in the Mac's
camera notch and in a Chat window, and keeps the Mac awake while agents work. From the notch, Chat or
a paired phone you can read a session, answer its questions and permission prompts, reply, and switch
accounts when a limit runs out. It works with Claude Code first, and with Codex and Antigravity CLI.

Today it is:

- a macOS app in Swift (macOS 13 or later);
- `hostd`, the session host that keeps terminals alive when the app quits;
- a Flutter phone app, which people build themselves for now.

It is being rebuilt: a Rust engine that runs as a service (`notchlingd`) on every computer, one GPUI
desktop app for macOS, Linux and Windows, and the same Flutter phone app.
[ARCHITECTURE.md](../ARCHITECTURE.md) describes both.

The project is meant to run with agents doing most of the work. They take issues, fix bugs, add
features and open pull requests, and checks decide what may land without the maintainer.
[GOVERNANCE.md](../GOVERNANCE.md) says who decides. [policies/automation.md](policies/automation.md)
says what agents may do.

### In scope

- Watching and driving agent sessions (Claude Code, Codex, Antigravity CLI) from the notch, Chat and
  the phone.
- Keeping the computer awake while agents work.
- The accounts those tools use: saving them, switching, limits and cost.
- Tools agents use alongside a session: the Secret Vault, local copies of Artifacts, handing a job to
  another tool, and Machines, which sets up the computers that run the overnight worker.
- Setting up the agent tools themselves: the hooks, skills and rules Notchling adds to each, and
  which model each kind of work gets ([decision 0019](decisions/0019-agent-instructions-per-tool.md)).

An idea outside this list starts as a feature request. The maintainer decides.

## Goals

1. An issue can go from filed to merged with no human step when it only touches the auto-merge zone.
2. Quality holds or improves as agent pull requests grow. Every rule fails a check instead of living
   in a doc ([rules.md](rules.md)).
3. Another developer or an agent can work correctly from what the repository says, without asking.
4. A mistake found after merge turns into a check, a test or a skill, so it can't land twice.
5. Users get updates on their own, from Beta to Stable, and no release ever touches a signing key
   from code an agent wrote.

Goal 1 needs the agent setup (Phase 5) and goal 5 the release pipeline (Phase 6). The release pipeline is built and off until the maintainer turns it on, and the app has no updater yet ([automation.md](policies/automation.md#releases-so-far)); the agent setup is described in the same policy.

## Non-goals

- App Store or Google Play releases. For now people build the phone app themselves.
- A website.
- Replacing the sd worker. The design changes how it authenticates and labels work, not what it does.

## Anti-goals

These count as failures even if everything works:

- A stranger's issue or pull request steers an agent into logins, Keychain, the vault, lanes,
  Machines, the hook, process launching or the release path, and that change lands without the
  maintainer.
- Agents loosen their own guard rails, by any path: workflows, settings, CODEOWNERS, rulesets, the
  lint baseline, tests, goldens, `AGENTS.md`, skills, `.claude/`, the kill switch, the App's
  permissions.
- A secret, token or real login is reachable from a CI job that runs repository code, or from a test.
- A check is green and means nothing, because it would pass on its own bad example.
- An agent acts with the maintainer's GitHub identity.

## What Notchling will never do

The app:

- Send anything to a server of its own. There is none. There is no telemetry today; crash reports,
  if they come, are opt-in only. [policies/privacy.md](policies/privacy.md) lists every call the app
  makes.
- Send an account's login anywhere except the service it belongs to, or a machine you set up in
  Machines.
- Renew the login a tool is using. Refresh tokens rotate, so renewing Claude Code's or Codex's live
  login would sign it out.
- Change more of your config than its own part: in `~/.claude/settings.json` only Notchling's hook
  entries, and the `agent` key only while you have Plan-Only Main Chat on (with a backup, your other
  hooks and keys kept); on a worker set up in Machines, only the one key per tool that turns its
  self-update off (with a backup, everything else kept); only `claudeAiOauth` in the
  `Claude Code-credentials` Keychain item; only `oauthAccount` in `~/.claude.json`.
- Write anything into an agent tool's folders but its own instructions: the `notchling-*` files and
  Antigravity's `notchling` plugin that [decision 0019](decisions/0019-agent-instructions-per-tool.md)
  lists, and `~/.codex/AGENTS.md` only when you have none. It never changes or removes a file it
  didn't write, or one you edited unless you choose Use Notchling's (your copy is kept as a backup),
  and when you have your own rules for the same purpose it writes none.
- Answer a session's permission prompt for you. The prompt waits for your answer. When the app isn't
  there to ask you, the hook passes and Claude Code asks in its own terminal. The one exception is a
  job one agent hands to another with `--delegate --write`: the other tool may then edit files in the
  folder it was given.
- Copy accounts, lanes or the vault to another computer. Each computer keeps its own.

The project:

- Let an agent merge a change outside the auto-merge zone without the maintainer.
- Put a signing key or the model review key in a CI job that runs repository code.
- Give the agent App the `workflows` permission or admin rights.
- Let a label decide anything. Anyone with pull-request write access can remove a label.
- Leave the MIT license, or take a dependency that isn't MIT, Apache or BSD licensed.

## Weaker on Linux and Windows

Notchling runs only on macOS today. The rebuild brings Linux and Windows, and some protections don't
carry over.

- **Secrets.** On macOS, the vault's values are one Keychain item that trusts only the Notchling
  build that wrote it. Any other program, `security` included, gets macOS's prompt first. Linux's
  Secret Service and Windows' Credential Manager have no per-app access control. Any program running
  as you can read what Notchling keeps there, vault values included. The vault's Ask First prompt
  still guards uses that go through Notchling, but not a program that reads the store directly. On
  those systems, the vault is only as safe as your user account. The rebuild's vault has a store
  only on macOS so far: on Linux and Windows it answers that there is none.
- **Saved logins** are the same on every system. On macOS they go through `/usr/bin/security` on
  purpose, so a rebuilt app keeps access, which also means any program running as you can read them.
- **Replies.** Typing into a session (`claude attach` over a pty) was verified only on macOS
  ([decision 0001](decisions/0001-gpui-spike.md)). Until it passes on Linux and Windows, the apps
  there show sessions but don't send replies.
- **Input methods.** Vietnamese input was checked only with macOS's own Telex and VNI. Linux and
  Windows input methods are unverified (decision 0001).
- **No notch.** Linux and Windows get a tray icon and a panel at the top of the screen instead of the
  notch and the menu bar. This is the designer's call and open to change.
