---
name: add-protocol-field
description: Use when the phone and the Mac need to exchange something new over the bridge protocol (agentconsole.v1), such as a new field on a session or chat message, or a new op. Additive only, schema first, then regenerated types and a conformance fixture.
---

The protocol is written down in `protocol/` (read `protocol/README.md` first): the JSON Schema, one fixture per message, and types generated from the schema for Rust and Dart. Every file here is protected: the bridge, `apps/flutter/lib/core/protocol/`, `crates/protocol/`, `protocol/` and `tools/protocol/` all wait for the maintainer.

**Additive only.** Add an optional field or a new op. Older phones and Macs ignore what they don't know. Never rename or remove a field, change its type, or make an optional one required. A breaking change needs the maintainer and a protocol version bump; it is not this skill.

1. **Name it and say what it means.** One line: the field, its type, who sends it, what an older peer does without it. Look for an existing field that already carries it (`SessionDTO`, `ChatMessageDTO` in `legacy/macos/Sources/ConsoleCore/Bridge/BridgeProtocol.swift`).

2. **Schema first.** In `protocol/agentconsole.v1.schema.json` add the field to its definition (`Session`, `ChatMessage`, ...) as optional (not in `required`), with a `description`, and a `default` where a reader has one. A new op is a `<Name>Request` (with `"op": {"const": ...}`), a `<Name>Response`, and an entry in the `Request` union. Use only what `tools/protocol/gen.py` reads (its docstring lists it); anything else it refuses.

3. **Fixtures.** Add the field to the Mac's canned data in `legacy/macos/Tests/ConsoleCoreTests/ProtocolFixturesTests.swift`, keeping one example of the message without it (an older peer), then rewrite the captured files and read the diff: `PROTOCOL_FIXTURES_WRITE=1 swift test --filter ProtocolFixturesTests`. A new op also needs a request fixture in `protocol/fixtures/` (`<Name>Request.json`, written by hand from what the phone sends) that the test sends. Fixture files are named after the schema definition they exemplify, plus an optional `.variant`.

4. **Regenerate the types.** `python3 tools/protocol/gen.py` rewrites `crates/protocol/src/generated.rs` and `apps/flutter/lib/core/protocol/generated.dart`. Commit them as they come out; never edit them by hand (`python3 tools/protocol/gen.py --check` fails, rule `GUARD-PROTOCOL-GEN`).

5. **Send it and read it.** The Mac encodes with the hand-written DTOs and fills the field in (today: `LiveBridgeBackend.enrich` in `legacy/macos/Sources/Notchling/Phone/LiveBridgeBackend.swift`): an optional property on the DTO in `BridgeProtocol.swift`, `nil` left out of the JSON, with a doc comment saying older phones ignore it. `ConsoleCore` stays free of Notchling types and macOS 14 APIs. The phone reads it with a fallback for when it's missing, in `apps/flutter/lib/core/protocol/models.dart` (`strOrNull` or an `is Map` check, never `str`, which expects it to be there).

6. **Document it.** Add it to the additive fields list in the Phone section of `legacy/macos/Sources/AGENTS.md`.

7. **Checks.**
   ```sh
   swift test
   python3 tools/protocol/gen.py --check
   python3 -m unittest discover -s tools/protocol/tests -t tools/protocol
   cargo test --workspace --locked
   cd apps/flutter && flutter analyze && flutter test
   PYTHONPATH=tools/guard python3 -m guard lint
   ```

Tried: not yet
