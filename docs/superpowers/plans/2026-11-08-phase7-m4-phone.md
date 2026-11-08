# Milestone 4: The Phone (built and switched off)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** the daemon can run the phone bridge with the same protocol, the same certificate and the same pairings as the Swift app, so phones don't re-pair. It also serves the phone extras. The desktop app gets the pairing panel, and the phone can follow several computers. All of it ships **switched off**: the Swift app keeps ports 47821/47822 and `AgentConsole/bridge/` until the maintainer hands them over (decision 0011).

**Spec:** `docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md`: Parts E5, E15, U6, P2; Milestone 4; "Remote computers"; "Swift and Rust side by side". Decisions 0006, 0007, 0011. Protocol: `protocol/README.md`, `protocol/agentconsole.v1.schema.json`, `protocol/fixtures/`.

## Global Constraints

### Off by default

The daemon's bridge has a runtime switch, `bridge` (off at every start, the same rule as `acting`). It is turned on only by an app-tier `bridge.set {on}`, and the app re-asserts the user's choice.

While it's off, the daemon:

- binds no port;
- reads `AgentConsole/` only when the app asks for pairing state;
- never writes into `AgentConsole/`.

### Same identity as the Swift app

- The certificate and key are read from `~/Library/Application Support/AgentConsole` byte for byte. They are never regenerated while files exist.
- Device tokens and pairing records stay in the Swift app's file format, read and written compatibly, under a flock.
- The data folder is a parameter, so tests use temp folders with generated certificates.

### Protocol

- The protocol is `agentconsole.v1`, exactly as the schema says.
- The Rust router must pass every fixture in `protocol/fixtures/`. Port `ProtocolFixturesTests`.
- The schema is additive only. A new field or op follows the `add-protocol-field` skill: schema first, then `python3 tools/protocol/gen.py`.
- Generated files are never edited by hand.

### Security

All of this is ported from ConsoleCore and pinned by its tests:

- TLS on :47821.
- The tunnel origin on loopback :47822 behind the Cloudflare Access JWT check (`AccessJWT`, RS256, `aud` and `iss` checked, keys cached).
- The address filter.
- The auth rate limiter.
- One-time pairing codes.
- The audit log.

Device tokens are compared in constant time.

### Crypto dependencies

- Use `rustls` with the `ring` provider (not aws-lc-rs), `rcgen` only in tests, and `tungstenite` for WebSocket, if their licenses pass `cargo deny`.
- If any needed crate fails `cargo deny`, stop and report the crate and its license. Never edit `deny.toml` without the maintainer.

### Tests

- No test binds 47821 or 47822. Ports are parameters, and tests bind port 0.
- No test talks to Cloudflare. JWTs are signed with a test key.
- No test touches the real `AgentConsole/` folder or Keychain.

### Phone (Flutter)

- `flutter analyze` and `flutter test` pass.
- Generated Dart protocol types come only from `gen.py`.
- The phone keeps working against a Swift Mac. New behavior is additive.

### Code

- Files under 600 lines.
- No unwrap or expect outside tests.
- Everything type-checks for Linux and Windows.

## Tasks

1. **E5a, router and hub** (`crates/engine/src/bridge/`). Port `BridgeRouter`, `BridgeHub`, `BridgeProtocol` DTOs (or use the generated `crates/protocol` types where they match), `LiveBridgeBackend.enrich` (activity and facts) and `ForeignSessions` mapping, over a `BridgeBackend` trait.
   - Port `ProtocolFixturesTests` and the router and hub tests.
2. **E5b, transport and the daemon's bridge** (`crates/engine/src/bridge/net/`, notchlingd).
   - TLS server, WebSocket wire, connection, pairing codes, device store, rate limiter, address filter, Access JWT, audit log, tunnel origin.
   - The `bridge` switch and the app-tier ops `bridge.set`, `bridge.status`, `bridge.pair.begin` (returns the QR payload and code), `bridge.devices`, `bridge.revoke {device}` and `bridge.tunnel.status`.
   - Port the ConsoleCore tests for each.
3. **E15, phone extras.**
   - Visualize: port `VizEngine`, `VizSpec` and `VisualizeSuggester`, plus `TranscriptFacts`, which M1 deferred.
   - Photos: `session.image.begin`, `chunk` and `end`, in 96 KB base64 pieces, saved 0600 under the cache folder, pruned after a day, with the path typed in only when acting is on.
   - Web snapshots: the `WebSnapshotter` logic that is pure. Capture itself stays macOS-only behind a trait.
   - Streaming (`PhoneStreamer`).
   - Port their tests.
   - Starts after Task 1 merges.
4. **U6, the pairing panel** (`crates/app`).
   - QR and one-time code, paired phones with Revoke, the bridge switch, and tunnel status.
   - Wired to the `bridge.*` ops, and disabled with a reason while the bridge is off or no daemon is present.
   - Rendered with `--render phone-panel`.
5. **P2, remote computers.**
   - First, a new decision record `docs/decisions/0012-remote-computers.md` with status **Proposed**. It defines identity (each computer's certificate fingerprint and name), transport (LAN, or the tunnel with Access), host-qualified session ids, reconnect cursors, simultaneous clients and revocation.
   - Then the phone side in `apps/flutter`: a list of computers, each with its own pairing and connection; sessions grouped per computer; session ids qualified by host in the phone's state. Reuse the existing `features/macs` if it already does part of this.
   - Desktop-to-desktop connections wait until the maintainer accepts 0012.

## Review Focus

- **Pairing.** A phone paired with the Swift app must still connect to the Rust bridge, with the same certificate fingerprint and the same token check. Test this with a device store written in the Swift format.
- **Hostile input.** A frame of 256 KB plus one byte, a broken WebSocket frame, a slow-loris handshake, or a forged or expired Access JWT must be refused without stalling other phones.
- **Images.** A path traversal in an image upload's name must not escape the cache folder. A huge or never-finished upload must be pruned.
- **Bridge off.** No port may be bound while the bridge is off, and nothing under `AgentConsole/` may be written.
- **Multiple computers.** On a phone with two computers, a session id that is the same on both must never act on the wrong computer.
