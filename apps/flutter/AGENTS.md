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
