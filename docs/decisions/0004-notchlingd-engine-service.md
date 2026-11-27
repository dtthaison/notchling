# 0004: The Rust engine runs as the notchlingd service

## Context

Today the Swift app does almost everything in its own process: it reads sessions, types replies, runs the phone bridge, the vault server and limit watching. Only the terminals live apart, in hostd. The hook, the phone and the vault's command line all reach into that one app.

The design rebuilds Notchling for macOS, Linux and Windows. The desktop app, the phone, the hook and the command line all need the same engine.

From the design: [Architecture](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md#architecture).

## Decision

- The engine is written in Rust.
- It runs as **`notchlingd`** on every computer: a LaunchAgent on macOS, a systemd user service on Linux, a startup task on Windows.
- `notchlingd` owns everything that acts or lives long: replies, session control, the phone bridge, the vault, accounts and lanes, limit watching, keep awake.
- The desktop app, the phone, the hook and the command line are its clients. They reach it through the local API ([0007](0007-local-api-tiers.md)).

## Consequences

- **The hook under a daemon** *(review)*. The daemon answers "pass" at once unless a client that can answer (the app or a phone) is attached. The hook keeps its own deadline and passes on any socket error. `claude -p` (`sdk-cli`) sessions stay ignored. Each of the three has a fixture test.
- **Terminals stay apart** *(review)*. They live in a separately restartable pty host: today's hostd, later its Rust port `crates/ptyhost`. Updating the daemon never ends a running terminal session.
- The engine is a library (`crates/engine`); the service is `crates/notchlingd`.
- While the Swift app and the daemon run side by side, each resource has one owner ([0011](0011-one-owner-per-resource.md)).
- Two parts are the designer's calls and not settled:
  - **The app reads transcripts itself**, read-only, with the engine library, for speed. It never writes or acts except through the daemon.
  - **Machines stays a separate tool**, not inside the daemon, and is ported last (part E10).
- Nothing of this exists yet. The daemon starts serving sessions in Milestone 2 and acts in Milestone 3.

## Status

Accepted. The maintainer decided the Rust engine and the always-on service *(decided)*. The hook and pty host rules came from the reviews and are part of the approved revision 2.

The two designer's calls above stay open until the maintainer accepts them or a later record changes them.

## Date

2026-11-07
