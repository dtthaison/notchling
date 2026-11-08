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

## Date

2026-11-07
