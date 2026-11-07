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

## Date

2026-11-07
