# 0006: Flutter phone, generated protocol types, no Rust

## Context

The phone app is Flutter today (`mobile/`, about 15.6k lines). It speaks `agentconsole.v1` to the phone bridge. The protocol is written twice by hand: in Swift (`Sources/ConsoleCore/Bridge/BridgeProtocol.swift`) and in Dart (`mobile/lib/core/protocol/models.dart`). There is no written spec.

The engine moves to Rust. Rust code could also be built into the phone.

From the design: [Architecture](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md#architecture), "Protocol", and [Phone app (Flutter)](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md#phone-app-flutter), part P1.

## Decision

- The phone stays Flutter *(decided)*. Its users build it themselves for now; there's no App Store or Google Play release *(decided)*.
- No Rust in the phone.
- The protocol gets a written spec in JSON Schema, with conformance and behavior fixtures, version negotiation, and additive changes only. Dart and Rust types are generated from it.

## Consequences

- The phone builds with the Flutter toolchain alone.
- The phone gets the engine's work through the protocol, not by linking the engine.
- A new field goes into the schema first, then the types are regenerated, then a conformance fixture is added.
- Dart and Rust types come from one schema, so the phone and the engine can't drift apart on a field.
- None of this exists yet. The protocol folder and the generated types come in Phase 4 (part P1).

## Status

Proposed. Flutter for the phone is Accepted. Keeping Rust out of the phone and generating its types is the designer's call ("Calls I made" in the design).

It settles when the maintainer accepts it. The natural point is after Phase 4, once the generated Dart types are in the phone and it passes `flutter analyze` and `flutter test` with them.

## Date

2026-11-07
