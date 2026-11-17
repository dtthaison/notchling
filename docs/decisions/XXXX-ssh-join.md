# XXXX: Add Machine over SSH, a headless node joined by an invite

## Context

Before this, a machine reached the cluster in two places with many hand steps between them: the Machines window (an agent-tier dev binary, not in the bundle) checked and fixed sd's worker machines, and the Cluster sheet made a join token that the new machine used and the maintainer then approved with Add in the Inbox. Between them sat the overview plan's manual steps M1 to M15: Tailscale, a Linux build copied by hand, the node user and linger, installs on each Mac, disk encryption. The maintainer asked to type an SSH address and nothing else, and decided the same day that machines added this way get no app, Macs included ([the design](../superpowers/specs/2026-11-16-ssh-join-design.md), "Source").

On 2026-11-16 the maintainer accepted the design's answers 1 to 9 as recommended, and lane E made the join token app-only (`notchling cluster token` is gone; a terminal or agent gets no token).

## Decision

- Machines added over SSH run only the headless `notchling` daemon as a service; no app, on Macs too.
- The add flow runs in the app on the engine's Machines library (app tier, 0007); no local API op runs ssh; the `notchling-machines` binary stays the agent-tier dev tool.
- A Mac node's daemon lives in a bundle-shaped folder (`~/.local/share/notchling/Notchling.app/Contents/MacOS/notchling`) so its own clients trust it, and runs from a LaunchDaemon `dev.notchling.node` as the node user.
- Invites: join tokens bound to one node id, made only by the app tier (`cluster.invite`), one use, 10 minutes; the primary approves them on arrival with an Inbox notice; ordinary tokens keep the Inbox's Add.
- A build id (`NOTCHLING_BUILD`) in every binary and `Versions.build`; Update N Machines compares it.
- Linux builds are static musl builds made on the Mac by `tools/build-node.sh` until signed release assets (version sync §16).

## Consequences

- It amends `XXXX-cluster-v1-nodes`: a join from an invite needs no Add. A join with an ordinary token still raises the Inbox's Add, and since lane E such a token too comes only from the app.
- It amends the B15 Machines node steps: a Mac with the app no longer skips the binary and service steps; the service is installed anyway and the join waits for the app to be closed there.
- The overview plan's manual steps M1 to M15 become Fixes where they can be done safely over ssh (linger, installs, Stays awake, Git, the build) and Checks with how-tos where they can't (Tailscale on a Mac, logins, disk encryption, host keys).
- The shipped app runs ssh for the first time, for node steps only, each Fix showing its commands before the one Run click.
- A Mac node under a LaunchDaemon has no login Keychain and no privacy prompts; Claude Code under it is proven by the maintainer's live check, and the LaunchAgent is the fallback if it fails.
- The plan is [2026-11-16-ssh-join](../superpowers/plans/2026-11-16-ssh-join.md), built on branch `agent/ssh-join`.

## Status

Accepted by the maintainer on 2026-11-16 (the design's question 9, with answers 1 to 8 as recommended). The number is a placeholder, assigned at merge.

## Date

2026-11-16
