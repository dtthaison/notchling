# XXXX: Workers follow the Mac's tool versions, on the user's click

## Context

`XXXX-tool-versions-follow-the-mac` made the Mac the reference for Claude Code, Codex and Antigravity but kept phase 1 and 3 read-only on workers: the Cluster sheet warned ("m1 runs Claude Code 2.1.318, the Mac 2.1.327") and nothing installed there. Its notes say "Phase 3 keeps workers where they are" and "Nothing installs or changes on a worker". A warning with no way to act on it sent the maintainer to ssh by hand, and a worker on another tool version is exactly the case the cluster review's issue 10 worried about: a dispatched job meets a transcript line, a flag or a dialog the Mac's readers were checked against only on the Mac's version.

On 2026-11-18 the maintainer decided that once a machine is in the cluster, Notchling does what it needs to there, including bringing a Worker's Claude Code, Codex and Antigravity to the Mac's versions.

## Decision

- **A worker follows the Mac's usable reference on the user's click.** The target is the Mac's reference version for that tool, usable only for the binary it checked (`XXXX-tool-versions-follow-the-mac`). With no usable reference there is no target and no button. A version that failed its check is never a target. A worker that is ahead of the reference moves down only by the same click, and the button says "down".
- **Never automatic.** Each tool update starts from a click in the Cluster sheet: the Update button on a drift warning, or Update Tools in the machine's menu for every drifted tool on it. Nothing starts at a daemon start, on a drift notice, after a Mac update or on a timer. Automatic stays unavailable, as the first record already says.
- **Exact version only.** Claude Code runs `claude install <x.y.z>` (native) or `npm install -g @anthropic-ai/claude-code@<x.y.z>`; Codex runs its standalone installer with `--release <x.y.z>` or `npm install -g @openai/codex@<x.y.z>`. A version reaches a command only as three plain numbers; a pre-release suffix is refused. A tool installed a way that can't install an exact version (Homebrew, apt, an unknown path, and Antigravity, whose installer takes none) gets a how-to text and runs nothing.
- **The worker user, no sudo.** The command runs over the existing ssh runner as the account Machines already uses, by the method the tool was installed with. A global npm folder that account can't write fails with that reason shown; Notchling doesn't escalate.
- **A turn is never stopped.** The update refuses while the worker runs a job (the cluster's own job list) or any process of that tool (a working session, whoever started it), and says so. An unreadable process list refuses too.
- **Success is what the worker says afterwards.** The version is read again with the same Tool Versions read, and the update succeeded only when it equals the target. Anything else is a failure with both versions shown.
- **Self-update stays off on workers.** Updating a tool never turns its own updater back on, and the Self-Update Off steps stay as they are. The two never fight over a version.
- **Gated by the install switch.** The update runs behind the Switch for installs, as Update and Go Back on the Mac do (`CAP-PROCESS`), checked again when the button is clicked.
- **Recorded like any Machines Fix.** A run is one Fix with its own log lines (the command first) and one `machines.fix` line in `audit.jsonl`, with the machine and the tool's title and version.
- **The Mac's own tools are untouched by this.** Update and Go Back on the Mac keep their own path.

## Consequences

- It amends `XXXX-tool-versions-follow-the-mac`. Its lines "phase 3 keeps workers where they are" and "Nothing installs or changes on a worker" end for tool updates the user starts. Rollouts, Roll Out, Automatic and the rollout lock described there remain unbuilt and are not made easier or harder by this.
- It adds one allowed write on a worker beyond the self-update key: the installed copy of Claude Code or Codex, by the tool's own installer. `docs/goals.md` and `docs/policies/privacy.md` list it; the maintainer changes them.
- Tools Notchling can't pin stay a how-to. Antigravity, Homebrew and apt installs are never updated from here.
- A node reports its own tool versions on its own schedule, so the cluster warning may outlive a successful update until the node reports again. The Cluster sheet hides the Update button as soon as the worker's re-read equals the target and shows the result on the card.
- The Machines window's Tool Versions step is unchanged: it still only reads.

## Status

Decided by the maintainer on 2026-11-18. The number is a placeholder, assigned at merge, together with the notes changes the maintainer makes.

## Date

2026-11-18
