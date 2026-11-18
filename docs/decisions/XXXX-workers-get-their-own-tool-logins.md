# XXXX: Workers get their own tool logins

## Context

`XXXX-workers-follow-the-mac-on-click` let the user bring a Worker's Claude Code, Codex and Antigravity to the Mac's versions. It left two gaps in "adding a Worker leaves it working like the Mac". A tool the Worker doesn't have (m1 had no Codex) stayed missing, and a tool it has but isn't signed in to stayed unusable. The first answer to the second was a how-to ("in your own terminal on m1 run codex login --device-auth"), and the login steps in Machines said "Notchling never signs a tool in".

Codex and Antigravity can't share the Mac's login. Their refresh tokens rotate: a copy on a second machine signs one of the two out the next time either refreshes. Root `AGENTS.md` already says never to refresh a token in use, and the vault and `security` rules say what Notchling may read from the Mac's logins.

On 2026-11-18 the maintainer decided that adding a Worker should leave it working like the Mac, with each Worker holding its own login for the same account, approved by the user in their browser on this Mac.

## Decision

- **Each Worker has its own login per tool.** Codex signs in on the Worker with `codex login --device-auth`, run over the existing ssh runner as the worker user. The user opens the link on this Mac and enters the one-time code there. The Worker's login is a separate grant on the same account; it is never the Mac's.
- **Never copied.** No auth file, token, cookie or Keychain item moves between machines, in either direction. Nothing is read back from `~/.codex` or any auth file on the Worker; Notchling learns the result only from the tool's own status command (`codex login status`) and the exit code. This Mac's logins are never touched by this.
- **The link and the code live in one place.** They are read from the command's output, held in memory for the sign-in sheet on this Mac, and shown only there. Every log line, the Fix log, the audit line, events, errors and the card mask them: the output is scrubbed by pattern and by the exact values found before it reaches any sink, and the sheet's data is dropped when the sign-in ends. The link is opened only if it is an `https` address on openai.com or chatgpt.com.
- **Bounded and cancellable.** The wait ends after 10 minutes, on Cancel, or when the command exits. A cancel stops the local ssh; the remote command also stops itself after 10 minutes and when the code expires.
- **Success is what the status check says afterwards.** The command's exit code alone is not success. After it, the same status read as the Codex login step runs, and only a signed-in answer counts. The result shows on the card.
- **Antigravity has no headless sign-in Notchling can start.** `agy` signs in through its own interface with the same Google login as the Antigravity app, and no device flow is documented. The Sign In button on its line shows what to do on the Worker and a Check Again button that reads the login status. Notchling doesn't guess a flow for it.
- **Claude Code keeps the long-lived setup token from the Vault.** Its login on a Worker is the cluster's, set up as before. This record changes nothing for Claude Code.
- **Missing tools are installed on the user's click.** A Worker's card offers Install Codex or Install Antigravity for a tool shown as Not Installed. Codex installs the Mac's usable reference exactly (the standalone installer's `--release x.y.z`, or `npm install -g @openai/codex@x.y.z` when curl is missing), as the worker user with no sudo, with the version validated as three plain numbers. Antigravity's installer takes no exact version, so the newest is installed and the card says so. The install refuses while the Worker runs a job and runs behind the install switch. It succeeds only if the tool is found afterwards, and for a pinned install only if its version equals the target.
- **Sign-in is gated by the accounts switch.** It changes a login, so it runs behind the Switch for accounts, checked again at the click.
- **A tool the Worker doesn't have shows Not Installed.** A tool counts as installed only when its program is there, not when a manifest, package record or empty folder is: the node's own report is read from files (`versions/mac.rs`) and a left-over manifest used to make a removed Codex show its old version. The Cluster card also trusts the Worker's own `command -v` read from Machines when it is fresh (20 minutes) over a node report that disagrees.
- **Recorded like any Machines Fix.** Each install and sign-in is one Fix with its log lines and one `machines.fix` audit line with the machine and the tool's title; neither the link nor the code is in them.

## Consequences

- It amends `XXXX-workers-follow-the-mac-on-click`: installing a missing Codex or Antigravity and signing a Worker in are now actions Notchling takes on the user's click, beyond updating a tool that is already there.
- It amends the Codex how-to in `crates/engine/src/cluster/logins.rs`: the sentence "Notchling never signs a tool in" is gone, since Notchling now signs Codex in on a Worker. "Never renews a login" stays: Notchling doesn't refresh a token.
- It adds one allowed write on a Worker: the tool's own login, made by the tool itself through its device sign-in. `docs/goals.md` and `docs/policies/privacy.md` list it, and that the link and one-time code pass through this Mac's memory and the sign-in sheet; the maintainer changes them.
- A cancelled sign-in may leave the command running on the Worker for up to its own timeout. Approving a stale code after a cancel would sign the Worker in; that is harmless but unplanned.
- Antigravity workers stay a manual step until the CLI documents a headless sign-in.
- The Machines window is unchanged: its login steps still only read, and its how-to now points to the Cluster sheet for Codex.

## Status

Decided by the maintainer on 2026-11-18. The number is a placeholder, assigned at merge, together with the notes changes the maintainer makes.

## Date

2026-11-18
