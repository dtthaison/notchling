# The phone app (Flutter): notes for agents

Agent Console on the phone: the same product as Notchling's Chat window, talking to the Mac over `agentconsole.v1`.

## Rules

- The look is `DESIGN.md`, taken from the Mac's Chat kit. Read colors and text styles from `Kit.of(context)`; never write a hex, font name or radius in a screen. Build surfaces with `Surface`, bars with `GlassBar`.
- Status is an icon and a word, never color alone. Orange is for the one primary action of a screen.
- Text wraps and grows (Vietnamese is longer than English); targets are at least 48 dp. Strings go in `lib/l10n/*.arb`, both `vi` and `en`.
- `agentconsole.v1` (`lib/core/protocol/`) is shared with the Mac's bridge. Don't change it casually: add optional fields and ops that older Macs and phones ignore, never rename or remove one. Its spec is the repository's `protocol/` folder: read `protocol/README.md`, and change the JSON Schema (`protocol/agentconsole.v1.schema.json`) first. `lib/core/protocol/generated.dart` comes from the schema by `python3 tools/protocol/gen.py`, run from the repository root; never edit it by hand (`GUARD-PROTOCOL-GEN`). The app still reads the wire with the hand-written `models.dart`, and `test/protocol_conformance_test.dart` parses every fixture in `protocol/fixtures/` with both. The `add-protocol-field` skill has the steps. Pairing (`lib/features/pairing/`, `lib/core/pairing/`) and networking (`lib/core/net/`) need the maintainer.
- Existing tests and golden images (`test/goldens/*.png`) change only with the maintainer. Add new tests freely.

## Checks (CI runs the same)

```sh
cd apps/flutter
mkdir -p assets/viz/vendor assets/viz/fonts assets/models   # git-ignored assets; tests only need the paths
touch assets/models/ggml-small-q5_1.bin                     # (tool/sync_viz.sh, tool/fetch_model.sh for a real build)
flutter analyze
flutter test                                                # includes the goldens
flutter test --update-goldens test/goldens                  # only for a look change the maintainer asked for
```

## Why the code is this way

The code carries no comments (see [AGENTS.md](../../AGENTS.md)). What a change could break without knowing it, by file:

- `lib/app/alerts.dart::mutes and lib/features/sessions/archive_store.dart`: Mutes, archive and chat cache all key by storageKey (bare id under the empty scope, otherwise scope/id), and a delete for a scope must wait for the stored data to load or it comes back.
- `lib/app/notifications.dart::channels`: Android fixes a notification channel's sound when it is created, so the channels carry a version in their ids; change the id when the sound changes.
- `lib/app/shell.dart::KeyboardLift`: While the keyboard slides the list is never resized per frame: the composer moves by a paint-only transform and the list gets its real bottom padding once the inset holds still; closing drops the padding first.
- `lib/app/shell.dart::tab stack`: A hidden tab stays mounted but is cut off from MediaQuery changes and has TickerMode off, or its search field relays out on every keyboard frame; bars also go opaque while the keyboard is up (no BackdropFilter blur).
- `lib/app/widgets/kit.dart::Surface and lib/app/theme/tokens.dart::shadows`: Level 1 surfaces use one short contact shadow because a large blur on every bubble is re-rasterised on each list relayout; only the composer and sheets (level 2) keep a soft shadow.
- `lib/app/widgets/motion.dart::AnimatedSize`: An AnimatedSize with a zero duration asserts in layout, so under Reduce Motion the widget must be absent, not given Duration.zero.
- `lib/core/net/bridge_client.dart::ops`: Ops added later (commands.list, session.sendNow, the interrupt) answer `invalid` or "no such op" from an older Mac; the app keeps its built-in list or hides the action instead of failing.
- `lib/core/net/bridge_client.dart::sendImage`: A bridge message is capped at 256 KB, so photos go as base64 pieces between session.image.begin and .end; the phone-stream frame is "PHN1", a big-endian uint32 sequence, then the JPEG.
- `lib/core/net/tunnel.dart::headersFor`: The Cloudflare Access service token is sent only to the tunnel's own host, never to a LAN address or a redirect target, so it cannot leak off the tunnel.
- `lib/core/net/ws_transport.dart::connectPinned`: The LAN connector trusts no CA at all (only the pinned SHA-256 fingerprint makes a certificate acceptable) and never sends Access headers; the tunnel connector uses the system CAs because Cloudflare's certificate cannot be pinned.
- `lib/features/chat/chat_notifier.dart::ChatNotifier`: A chat sends only to the session id it was built with, and a page that returns after the computer changed is dropped, so messages never land in or show under another computer's session.
- `lib/features/chat/chat_pictures.dart::fetch`: Pictures are fetched once through a block-bodied cleanup, since returning the removed future from an arrow body would make the future wait on itself.
- `lib/features/chat/conversation_view.dart::entry`: Only a message that just arrived animates in; a fading bubble is drawn offscreen and costs frames while scrolling or while the keyboard resizes the list.
- `lib/features/connection/subscription_sync.dart`: The Mac has no op to stop the phone stream, so leaving the Phone tab means unsubscribing and subscribing again without it.
- `lib/features/macs/computers.dart::clientForSession`: An op on a session goes to that session's own computer client, never to another computer that happens to have the same session id; a takeover answer is resolved against the host taken when it started.
- `lib/features/macs/macs_notifier.dart::remove`: Removing a computer deletes its stored things (cache, archive, mutes) before its profile, so an interrupted removal can be repeated, and never hands the empty scope to another computer.
- `lib/features/visualize/viz_webview.dart`: A spec reaches the viz WebView as a JS string literal made by a second jsonEncode and read by JSON.parse, never spliced into script text, and navigation away from local bundle files is refused.
- `lib/features/voice/prompt_builder.dart`: Whisper keeps only about the last 224 tokens of its initial prompt, so the prompt is cut by characters on a word boundary with session words first.
- `lib/features/voice/silence_trim.dart and speech_engine.dart`: Silence is trimmed before Whisper because it invents text on silence; the model is copied out of the bundle once (whisper.cpp needs a file path) and freed after an idle period.
