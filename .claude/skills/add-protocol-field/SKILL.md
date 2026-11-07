---
name: add-protocol-field
description: Use when the phone and the Mac need to exchange something new over the bridge protocol (agentconsole.v1), such as a new field on a session or chat message, or a new op. Additive only, schema first, then regenerated types and a conformance fixture.
---

Needs Phase 4's `protocol/` folder (the written spec, JSON Schema and fixtures) and its type generator. Until then, use "Before Phase 4" at the end. Every file here is protected: the bridge, `mobile/lib/core/protocol/` and `protocol/` all wait for the maintainer.

**Additive only.** Add an optional field or a new op. Older phones and Macs ignore what they don't know. Never rename or remove a field, change its type, or make an optional one required. A breaking change needs the maintainer and a protocol version bump; it is not this skill.

1. **Name it and say what it means.** One line: the field, its type, who sends it, what an older peer does without it. Look for an existing field that already carries it (`SessionDTO`, `ChatMessageDTO` in `Sources/ConsoleCore/Bridge/BridgeProtocol.swift`).

2. **Schema first.** Add the field to the JSON Schema under `protocol/`, optional, with a description. Add a new op with its request and reply shapes. Don't touch generated code yet.

3. **Regenerate the types.** Run the generator Phase 4 sets up, for Dart (`mobile/lib/core/protocol/`) and Rust. Commit the generated files as they come out; never edit them by hand.

4. **Conformance fixtures.** Add new fixture messages under `protocol/`'s fixtures: one with the field and one without it (an older peer). Both the Dart and Rust conformance tests read every fixture; a message without the field must still parse.

5. **Send it and read it.** Fill it on the sending side (on the Mac today: `LiveBridgeBackend.enrich` in `Sources/Notchling/Phone/LiveBridgeBackend.swift`). Read it on the receiving side with a fallback for when it's missing.

6. **Document it.** Add it to the additive fields list in the Phone section of `Sources/AGENTS.md`.

7. **Checks.**
   ```sh
   swift test
   cd mobile && flutter analyze && flutter test
   cargo test                                   # once Rust code exists
   PYTHONPATH=tools/guard python3 -m guard lint
   ```

## Before Phase 4

The protocol is hand-written on both sides today, so the field is added twice:

- Mac: an optional property on the DTO in `Sources/ConsoleCore/Bridge/BridgeProtocol.swift`, with a doc comment saying older phones ignore it (see `activity` and `facts` on `SessionDTO`). `nil` is left out of the JSON. `ConsoleCore` stays free of Notchling types and macOS 14 APIs.
- Phone: a nullable field on the model in `mobile/lib/core/protocol/models.dart`, read with `strOrNull` or an `is Map` check, never `str` (which expects it to be there).
- Tests in new files: a Swift test in `Tests/ConsoleCoreTests/` that encodes the DTO with and without the field, and a Dart test in `mobile/test/` that parses both JSON shapes.

Tried: not yet
