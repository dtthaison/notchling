# 0007: Local API tiers and peer identity

## Context

With a daemon ([0004](0004-notchlingd-engine-service.md)), the desktop app, the hook and the command line all call it on the same computer. Agents run that command line (`--secret`, `--delegate`, `--page`). So the API faces code an agent controls, as well as the signed app.

Today `VaultServer` already works this way for the vault: it listens on `vault.sock` (0600) and answers only a peer that runs as this user from the app's own executable (`LOCAL_PEERPID` and `proc_pidpath`).

From the design: [Architecture](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md#architecture), "Local API".

## Decision

- The local API is a Unix socket or a Windows named pipe, for this user only.
- The daemon identifies the caller by its executable path: `proc_pidpath` on macOS, `/proc/<pid>/exe` on Linux, `GetNamedPipeClientProcessId` on Windows. This is what `VaultServer` does today.
- Two tiers:
  - the **app tier**, the signed desktop app, may do everything;
  - the **agent tier**, the command line agents run, gets only its narrow operations. Vault use still goes through the approval prompt.
- Account switching, vault values, Machines fixes and settings are app tier only.
- The vault is a separate service inside the daemon, with its own checks.
- Privileged calls are audit-logged.
- Approval replies are authenticated. An action that needs approval is denied when no UI is attached.

## Consequences

- An agent that can run a shell can use the command line, but can't switch accounts, read vault values without the prompt, run Machines fixes or change settings.
- A copy of the app binary in another place doesn't get the app tier, as `VaultServer` refuses a build in `build/` today.
- On a computer with no UI attached, anything that needs approval fails closed.
- Other computers and phones don't use this socket. They connect the way the phone does today, with pairing (see "Remote computers" in [ARCHITECTURE.md](../../ARCHITECTURE.md)).
- Part E17, in `crates/local-api`. It arrives in Milestone 2. Nothing of it exists yet besides `VaultServer`.

## Status

Accepted. The tiers came from the reviews and are part of revision 2 of the design, which the maintainer approved on 2026-11-07.

## Note, 2026-11-08: the exec race, and what stays best effort

The Milestone 2 review found that a path read once, after accept, can be beaten: a process connects, forks, and execs the app binary while its fork, which shares the socket, does the talking. The path then names the app. How `crates/local-api` answers it (`peer.rs`, `tier.rs`, `conn.rs`):

- The peer is read twice, at accept and when `hello` arrives, and the app tier needs both reads to be the same process running the same allowlisted file. Each app-tier request reads it a third time and is refused (`forbidden`, audited `peer_changed`) unless it is still the process that said hello.
- **macOS.** The peer is the audit token (`LOCAL_PEERTOKEN`), and its file comes from `proc_pidpath_audittoken`, in libproc since macOS 11, so always there on Notchling's macOS 13+. Checked on macOS 26: the kernel answers `LOCAL_PEERTOKEN` (and `LOCAL_PEERPID`) for the process that last sent or received on the peer's socket, not the one that connected, and exec gives a process a new pid version, which `proc_pidpath_audittoken` checks. A hello written by the fork therefore names the fork. A test reproduces the race (`server_exec_tests.rs`: a child connects, execs an allowlisted `/bin/sleep`, and another process says hello) and asserts the agent tier over many rounds. What is left: the exec'd app itself writing on the shared socket just after the fork's line, which the real app has no reason to do.
- **Linux: best effort.** Nothing is exec-proof there. `SO_PEERCRED` keeps the pid that connected, and the pid survives exec, so the start time in `/proc/<pid>/stat` and the file in `/proc/<pid>/exe` are compared between the reads: an exec between accept and hello is caught, an exec before accept is not.
- **Follow-up for Phase 6.** Builds are signed ad hoc today, so the code signature can't be checked. When Developer ID signing lands, the app tier should also require the app's signing identity (on macOS, the audit token's code signature through the Security framework), which no exec trick can fake.
- `NOTCHLING_APP_TIER_PATHS` is honored only by a daemon that itself runs from a cargo target folder (a dev build; the same rule `ensure_daemon` uses for what it may start), and `ensure_daemon` never passes it, or any other `NOTCHLING_*` variable, to the daemon it starts. An installed daemon's app tier is the installed app alone.

## Note, 2026-11-08: files at rest are not a tier boundary

The Milestone 3 review found that the daemon read its `acting` switch from `notchlingd-settings.json` at start. An agent that can run a shell can write any file this user can, then restart the daemon: the app tier was a file write away. The rule from here on: **nothing an agent can write turns on what only the app tier may**. A file this user owns says what the user (or anyone running as them) wrote, not what the app decided.

- Every daemon run starts with acting off, whatever any file says. Only an app-tier `settings.set_acting` during that run turns it on.
- The daemon no longer writes `notchlingd-settings.json`. One that still says on is ignored and the start is audit-logged (`file_said_on_ignored`).
- The app keeps the user's choice in its own settings (`actingOn`) and sends `settings.set_acting` again whenever the daemon says off after a (re)connect. That record is a file too, so it is only as strong as the app tier itself; it moves the decision to the app, which is the process the tier trusts, and the Phase 6 signing follow-up above is what makes that trust hold.
- The same goes for identity: the hook's process is read from the socket's peer credentials, never from its arguments, and it must descend from the Claude Code process whose `sessions/<pid>.json` names the event's session.

## Note, 2026-11-13: a paired phone may allow a secret

The maintainer decided that a paired phone, with the bridge, acting and vault switches on, may answer vault approvals through the inbox, Allow included (this reverses the inbox plan's ruling R32, which let only the Mac allow).

- The phone's `inbox.answer` goes through the bridge to `InboxOps::decide` with `answered_by` `phone`. The gates stay: the bridge on, acting on, the vault switch on (a wait it drained takes no answer), the first answer wins, and the producer's responder must take the answer. The phone never opens the form that takes a secret's value.
- Who may allow is decided in `InboxOps::decide`, under the inbox lock: an allow on a `secret_approval` is taken from `mac` or `phone` and refused from anyone else.
- Every phone `inbox.answer` is audited in `bridge/remote-log.jsonl`: one that is taken as `inbox.answer:<action>`, a refused one as `inbox.answer:refused:<reason>`.
- **Known risk, accepted by the maintainer.** The paired-devices list (`devices.json`) is a file an agent with a shell can write, so a forged device could answer. It is the same class of risk as the Ask First flag in `notchling.db`: files at rest are not a tier boundary (the note above). Pairing needs an anchor an agent can't write before this is closed.

## Note, 2026-11-14: a headless cluster member rejoins its home at start

The maintainer decided one exception to "off at every daemon start" for the `cluster` switch (decision XXXX-cluster-v1-nodes, its note of the same date): a node that is an active member of a cluster, recorded by its home as headless (it joined through an agent with no app attached) and not a client, turns `cluster` on at daemon start when no app is attached, and dials its home.

- Why it's safe enough: dialing home grants nothing by itself. The peer must present the certificate pinned in `cluster.db`, and every other switch the cluster may set (`acting`, `bridge`, `install`) still comes only from the home's `Members`/`HelloOk`, applied as the app tier (`ClientKind::Node`) only while no app is attached, never from a local file. `vault` and `accounts` never follow the cluster.
- "Never had an app attached" is the home's record of the join (`Member.headless`), replicated to the node, not a flag the node keeps for itself. The node's copy is still a file: an agent that flips it gains only a dial to the pinned home. A member joined from the app, or marked a client, stays off until the app turns the switch on, as before.
- The start is audited `cluster_set:cluster:on`, with the home as `peer`, and published as `settings.switch`.
- The risk this note accepts is the one the 2026-11-08 note names: `cluster.db` is a file at rest. An agent that rewrites the home's whole record and runs a node of its own there can pose as the home; the same agent can already do it through `cluster.join` while no app is attached. It closes when the pin has an anchor an agent can't write.

## Date

2026-11-07
