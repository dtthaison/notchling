# The bridge protocol: `agentconsole.v1`

How the Mac and the phone talk. This folder is the written spec; the code on both sides is checked against it.

| File | What it is |
| --- | --- |
| `agentconsole.v1.schema.json` | The JSON Schema (draft 2020-12): every request, answer and pushed event. The source of truth for the generated types. |
| `fixtures/*.json` | One real message per file. The Swift bridge writes the ones the Mac sends; the ones the phone sends are written by hand from what the phone's client sends. |
| `README.md` | This file: what the schema can't say (order of frames, binary frames, the rules for changing it). |

Generated from the schema by `python3 tools/protocol/gen.py`: `crates/protocol/src/generated.rs` (serde structs) and `apps/flutter/lib/core/protocol/generated.dart` (plain classes with `fromJson` and `toJson`). The phone app still reads the wire with its hand-written `apps/flutter/lib/core/protocol/models.dart`; a test pins the two to the same wire, and switching the app to the generated types is a separate change.

The protocol's name is `agentconsole.v1` because it began as the Agent Console app. Its Mac side is `ConsoleCore`'s `BridgeRouter`, `BridgeHub` and `BridgeProtocol` (under `legacy/macos/Sources/ConsoleCore/Bridge/`), and its phone side is `apps/flutter/lib/core/net/bridge_client.dart`.

## Transport

JSON text frames over a WebSocket, one message per frame, at most 256 KB each. The phone reaches the Mac either directly (TLS on port 47821, the certificate's fingerprint is in the pairing QR code) or through a Cloudflare tunnel. Nothing is served before `hello` or `pair`; a frame that is not JSON closes an unauthenticated connection, and five bad hellos lock the sender's address for a while.

Three kinds of frame, told apart by their keys:

- **Request**, phone to Mac: `{"id": 4, "op": "chat.get", ...fields}`. `id` is an integer from the phone (any JSON value is echoed back).
- **Answer**, Mac to phone: `{"id": 4, "ok": true, "data": {...}}` or `{"id": 4, "ok": false, "error": {"code": "not_found", "message": "..."}}`. An answer has the request's `id`; one for a frame the Mac couldn't read has none.
- **Event**, Mac to phone, unasked: `{"event": "sessions", ...}`. Only after `subscribe` (and `open`, to any logged-in phone).

The answer's `data` depends on the op, so the schema names it `<Op>Response`. The fixtures are named after the schema definition they are an example of: `ChatGetResponse.json`, and `ChatGetResponse.before.json` for a variant.

## Ops

| Op | Request (`<Name>Request`) | Answer `data` (`<Name>Response`) |
| --- | --- | --- |
| `hello` | `token` | `device {id, name}`, `version` |
| `pair` | `code`, `device {name, platform?}` | `token`, `deviceId`, `version` |
| `ping` | | `time` |
| `sessions.list` | | `sessions`, `models?` |
| `chat.get` | `sessionId`, `subagentId?`, `limit?`, `before?`, `after?` | `messages`, `hasMore`, `reset?` |
| `viz.get` | `sessionId`, `entity?` | `suggested`, `webURL?`, `webCandidates?`, `flow?`, `diagram?`, `chart?`, `entity?` |
| `web.snapshot` | `sessionId`, `width?`, `url?` | `binary`, `bytes`, `format`, `url`; and `sent` |
| `session.message` | `sessionId`, `text` | `sent` |
| `session.sendNow` | `sessionId` | `sent` |
| `session.image.begin` | `sessionId`, `bytes`, `caption?` | `uploadId`, `chunkBytes` |
| `session.image.chunk` | `uploadId`, `seq`, `data` | `received` |
| `session.image.end` | `uploadId` | `sent` |
| `commands.list` | `sessionId` | `commands` |
| `prompt.answer` | `requestId`, `allow?`, `choice?`, `text?` | `answered` |
| `session.new` | `cwd`, `model?`, `modelId?` | `started` |
| `chat.image` | `sessionId`, `imageId`, `maxWidth?` | `binary`, `bytes`, `format`, `imageId`; and `sent` |
| `session.takeover` | `sessionId`, `stopOriginal?` | `sessionId`, `resumed`, `stoppedPid?` |
| `projects.recent` | | `projects` |
| `subscribe` | `sessions?`, `sessionId?`, `subagentId?`, `lastMessageId?`, `phone? {deviceWindowId?, fps?, width?}` | `subscribed: true` |
| `unsubscribe` | | `subscribed: false` |

A `?` marks an optional field. The schema has the types, limits and descriptions.

### Order of frames

- `hello` or `pair` first. `pair` waits for the person to approve on the Mac, so it can take minutes.
- `subscribe` with `sessions: true` sends one `sessions` event **before** its answer, then pushes changes about every 300 ms. With `sessionId` it pushes the messages newer than `lastMessageId` (all current ones count as known when it is absent).
- `web.snapshot` and `chat.image` answer twice with the same `id`: first the JSON that announces a binary frame, then the binary frame and `{"ok": true, "data": {"sent": true}}` (the Mac writes both without waiting for the first; they may arrive in either order). A client takes the first answer as the reply and the binary frame as the picture.
- A photo goes up as `session.image.begin`, any number of `session.image.chunk` (`seq` 0, 1, 2, in order, at most `chunkBytes` of data each, base64), then `session.image.end`.
- A `prompt` event with a `prompt` is new; one with `resolved: true` and a `requestId` withdraws it.

## Binary frames

WebSocket binary frames, always 4 bytes of magic, a big-endian `uint32`, then a JPEG:

| Magic | Number | Sent |
| --- | --- | --- |
| `PHN1` | sequence, counting up | While `subscribe` has `phone`: the Mac's device window, a frame at a time (a frame offered while the last is still going out is dropped). |
| `WEB1` | the `web.snapshot` request id (0 when it is not a number) | After the JSON answer to `web.snapshot`. |
| `IMG1` | the `chat.image` request id (0 when it is not a number) | After the JSON answer to `chat.image`. |

## Errors

`error.code` is one of `unauthorized`, `not_found`, `view_only`, `invalid`, `busy`, `not_reachable`, `internal`. `message` is text for the person, in Vietnamese; do not parse it. A code the receiver doesn't know is read as an unknown error, not a crash (the generated types have an `unknown` case).

## Rules for changing it

`agentconsole.v1` is **additive only**. The schema's `version` stays `agentconsole.v1`.

- A new field is optional, and an older peer ignores it. A new op is a new request definition and a new `Response`.
- Never rename or remove a field, change its type, or make an optional field required. That needs a new protocol version and the maintainer.
- Every object in the schema allows extra fields (`additionalProperties: true`): a receiver ignores what it doesn't know.
- **Required** means the Mac always sends it and every phone relies on it. A field that is optional in Swift, or that the phone reads with a fallback (`queued`, `images`, `subagents`, `tool`, and the like), is optional here, with a `default` where the phone has one.
- Fixtures use made-up ids, names and paths. The only random values in the Mac's answers (a device id, a token, an upload id) are replaced by fixed ones before a fixture is compared or written.

## Keeping it true

Three things hold the schema to the real wire:

1. `legacy/macos/Tests/ConsoleCoreTests/ProtocolFixturesTests.swift` runs the real router, hub and encoders against a canned backend and compares every answer and event with its fixture. It feeds every phone-sent fixture to the router, which must accept it. To rewrite the fixtures after a change on the Mac: `PROTOCOL_FIXTURES_WRITE=1 swift test --filter ProtocolFixturesTests`, then read the diff.
2. `tools/protocol/tests/` validates every fixture against the schema with a small validator, checks that every op the router handles and every event the hub sends is in the schema, and tests the generator.
3. The generated types are tested on every fixture: `cargo test -p notchling-protocol` (parse and write back the same JSON) and `flutter test test/protocol_conformance_test.dart` (the same, and also through `models.dart`).

`python3 tools/protocol/gen.py --check` fails (rule `GUARD-PROTOCOL-GEN`) when the committed generated files differ from what the schema produces. After a schema change run `python3 tools/protocol/gen.py` and commit the result; never edit the generated files by hand.

The steps for adding a field or an op are in the `add-protocol-field` skill.
