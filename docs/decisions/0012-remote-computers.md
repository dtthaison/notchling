# 0012: Remote computers: one phone, several computers, each session on one

## Context

The design lets the phone and the desktop app follow sessions on other computers, the way the phone follows the Mac today, with the same pairing. Each session belongs to exactly one computer, which alone acts on it, and session ids are host-qualified. Nothing is replicated: accounts, lanes and the vault never leave their computer. A record must define identity, transport, reconnect cursors, simultaneous clients and revocation before any milestone uses it.

From the design: [Remote computers](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md) (the bullet under the parts list, and P2 in the phone's parts). Milestone 4, Task 5 builds the phone side.

What exists today:

- The phone already keeps a list of paired Macs (`apps/flutter/lib/features/macs/`), each with its own hosts, port, pinned certificate fingerprint, device token and optional tunnel. Only one is "in use" at a time, and only that one has a connection.
- A phone's id for a Mac is the first 16 hex characters of the certificate fingerprint (`MacProfile.id`).
- Session ids are whatever the computer gives: Claude Code's UUIDs, `pty:<n>`, `codex:<id>`, `agy:<id>`. Two computers can give the same id (`pty:3`, or a transcript copied between them).
- `agentconsole.v1` (`protocol/`) has no idea of a computer: every id on a connection belongs to the computer at the other end.

## Decision

**Identity.** A computer is its TLS certificate. Its identity is the SHA-256 fingerprint of that certificate, 64 lowercase hex characters, pinned by the client at pairing. Its host id is the first 16 of those characters. Its name is the `name` in the pairing QR: shown to people, never used to tell computers apart. A new certificate is a new computer, so it is paired again. The daemon carries the Swift app's certificate over byte for byte (Milestone 4), so the Mac keeps its identity through the handover.

**Transport.** The same two routes as today, per computer:

- LAN: TLS to the computer's own addresses on port 47821, with the certificate checked against the pinned fingerprint.
- Tunnel: the computer's own Cloudflare tunnel, with the Access service token from its QR, to its loopback origin on 47822 behind the Access JWT check.

Pairing is today's: QR or one-time code, approved on the computer, giving a device token for that computer only. A client holds one connection per computer it follows, each with its own token and its own retry and backoff. Computers never relay each other's sessions: a client always talks to the computer that owns the session.

**Host-qualified session ids.** In a client's state a session is `<host>/<session id>`:

- `<host>` is the 16-character host id, lowercase hex. It never contains `/`.
- `<session id>` is the id exactly as the owning computer sent it, and may contain `/` or `:`.
- A qualified id is split at the first `/`. It matches `^[0-9a-f]{16}/.+$`. No id a computer sends starts with 16 hex characters and a slash (UUIDs have a `-` at position 9, the others start with a letter and `:`).
- On the wire nothing changes: a connection only ever carries its own computer's bare ids, so the protocol needs no new field.

Everything a client keeps about a session is keyed by the qualified id: the picked session, open rows, the chat cache and the archive. Every op on a session goes to the client of the session's own computer, never to whichever computer is in use. The phone's stores keep one exception so an existing phone works unchanged: the Mac in use when the phone first runs this version keeps the empty storage scope, so its cache and archive keep their bare keys. Every other computer, and every computer paired later, uses its host id as the scope. The empty scope is never handed to another computer.

**Reconnect cursors.** A cursor belongs to one computer's connection:

- The session list has no cursor: `subscribe {sessions: true}` sends the whole list first, then changes.
- A chat's cursor is the newest message id the client has for that session on that computer. It is sent as `subscribe.lastMessageId` and `chat.get.after` after a reconnect. When the computer no longer has that message it answers `reset: true` and the newest page replaces what the client shows.
- A client never sends one computer's cursor to another. Two computers' `s1` have two cursors.

**Simultaneous clients.** A computer serves any number of paired clients at once: phones, and desktop apps once desktop-to-desktop connections exist. Each connection has its own subscription and cursors. The owning computer alone acts on a session, so it orders what clients send: replies go through its own outbox one at a time, and a prompt is answered once. The first answer wins, and the others see the prompt withdrawn (`prompt` with `resolved: true`) or get an error. Every client sees the others' effects through the usual pushes.

**Revocation, from either side.**

- From the computer: revoking a device (the Swift app's Paired Phones, the daemon's `bridge.revoke {device}`) deletes its token. The device's next `hello` gets `unauthorized`, and the client marks that computer revoked and stops retrying it. Only that computer is affected; the client's other computers keep working.
- From the client: removing a computer deletes its profile, token and storage scope on the client and closes its connection. Today the computer keeps the token until it is revoked there too. An additive op for a client to revoke its own token on the computer (`device.forget`) is proposed, and waits for this record to be accepted.

**Nothing is replicated.** Accounts, lanes, the vault, settings and transcripts stay on their computer. A client gets only what `agentconsole.v1` already sends about each session. No computer learns about another, and no client copies data between computers.

## Consequences

- One phone can follow a Mac and a Linux or Windows computer at once, each with its own pairing, token and connection. Sessions are grouped per computer, and a session id shared by two computers can't act on the wrong one.
- A phone with one paired Mac looks and behaves as before: no headings, the same cache and archive.
- Each followed computer costs the phone one connection. Computers that aren't in use only push their session list. Chat, Visualize and the phone stream follow the computer in use, which becomes whichever computer the picked session is on.
- A 16-character host id leaves a 1 in 2^64 chance that two computers share one. Pairing should refuse a computer whose id matches a paired one with a different fingerprint; today the newer pairing replaces the older one.
- No protocol change was needed for the phone side.
- Built in Milestone 4: the phone side (`apps/flutter/lib/features/macs/`: `session_ref.dart`, `computers.dart`, `computer_heading.dart`; per-computer clients in `features/connection/connection.dart`; tests in `test/computers_test.dart` and `test/computers_view_test.dart`).
- Waiting for this record to be accepted: desktop-to-desktop connections in the desktop app, the `device.forget` op, the pairing check on host id collisions, and notifications from computers other than the one in use.

## Status

Proposed. The design decided that computers see each other and that each session belongs to one computer; the details here (the id format, the storage scope, the cursor and revocation rules) are new. The maintainer accepting this record settles it.

## Date

2026-11-08
