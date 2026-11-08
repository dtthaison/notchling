# 0011: One owner per resource while Swift and Rust run side by side

## Context

The Swift app and the Rust daemon both run on the maintainer's Mac during Milestones 1 to 5. Some resources can have only one owner: the hook entries in Claude Code's settings, the terminal host, the phone bridge's ports, the vault socket, limit watching and account switching. Two owners would fight over them.

From the design: [Swift and Rust side by side](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md#swift-and-rust-side-by-side-review).

## Decision

One owner per resource per milestone. The Swift app reads a stand-down setting per area; the phone bridge switch already works this way.

| Resource | Swift owns it until | Then |
| --- | --- | --- |
| Hook entries in `~/.claude/settings.json` | M2 | the daemon; retirement rewrites the hook command to the new binary |
| hostd LaunchAgent `local.agentconsole.hostd`, `hostd.sock` | Swift retirement | the ported pty host, same socket contract |
| Phone bridge ports 47821/47822, `AgentConsole/bridge/` | M3 | the daemon, carrying `cert.pem`/`key.pem` byte for byte so phones don't re-pair |
| `vault.sock`, the vault Keychain item | M5 | the daemon (one Keychain prompt on handover) |
| Limit watching and auto-switch | M5 | the daemon; never both |
| Lanes, account switching | M5 | the daemon; lane Keychain names `Claude Code-credentials-<sha256(path)[:8]>` reproduced exactly |

Retirement unloads the Swift LaunchAgents, rewrites the hook commands and leaves a backup.

## Consequences

- Each area needs its stand-down setting in the Swift app before the daemon takes it over.
- Handovers keep what users already have: paired phones, the vault's values, the lanes' logins.
- Limit watching and auto-switch never run in both at once.
- Today the Swift app owns every one of these.

## Status

Accepted. The rule came from the reviews and is part of revision 2 of the design, which the maintainer approved on 2026-11-07.

## Date

2026-11-07

## Notes

- 2026-11-08 (Milestone 3): the daemon now holds the code for its M2 and M3 resources (the hook, `notchling hook` with `hook.request`; replies, answers, permissions, interrupt and take-back on live sessions; session control as it lands), but none of it is in use. Its `acting` setting (`<data dir>/notchlingd-settings.json`) is off by default: every acting op answers `forbidden` ("acting is off: the Swift app owns sessions until the maintainer turns this on") and the hook passes. The Swift app keeps the hook entries and every live session until the maintainer records the reply fixtures (docs/recording.md) and turns acting on with `settings.set_acting`. Ownership in the table above doesn't move until then.
