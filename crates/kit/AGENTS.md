# kit: notes for agents

## Why the code is this way

The code carries no comments (see [AGENTS.md](../../AGENTS.md)). What a change could break without knowing it, by file:

- `crates/kit/src/icon.rs::Icon`: Icons are Lucide SVGs vendored (only those used, ISC license in `assets/icons/LICENSE`) because SF Symbols may not ship in apps for Windows or Linux; the SF name is kept only as data for the Swift app.
- `crates/kit/src/ink.rs::avatar gradient`: A session's avatar gradient is picked by FNV-1a of its name using the high bits (`hash >> 40 % 8`), the same hash and bucket as the Swift app, so a session looks alike in both; low bits spread poorly.
- `crates/kit/src/render_map.rs::RenderMap`: The map paints from the window's own draw, never asking a cached part to draw for its sake, and draws the map itself causes (its flash-clearing timer) are marked ignored so they aren't counted.
- `crates/kit/src/scrollbar.rs::scrolled`: Kit scroll positions are "scrolled down", 0 at the top and never negative, the opposite sign of GPUI's offset; a list that follows its end is at `max`, and an offset is clamped to `0..=max` since later measured rows can leave it past.
- `crates/kit/src/text_edit/mod.rs::replace_text`: When the owner clears the box mid-composition (a send), the input method may still commit its marked text afterwards; that one commit is dropped, or the sent words come back.
- `crates/kit/src/ui/icons.rs::KitAssets`: An app must hand `KitAssets` to `Application::with_assets` or every `svg()` icon draws nothing, and an SVG takes its color from its own style, so an icon inside a button needs `icon_colored`.
- `crates/kit/src/ui/parts.rs::part`: `.part("Name")` goes last in an element's chain so the dashed outline and badge paint over its children; they take no layout space and have no hitbox, so they never take a click.
- `crates/kit/src/ui/resize.rs::resize_edges`: The border zones are the last child of the window root, carry no mouse handlers and don't occlude, so the press still reaches AppKit's live resize; a titled window has no top zones.
- `crates/kit/src/ui/scrollbar.rs::Scrollable`: The bar shows only for a scroll the user makes, never for app-made ones, and its fade frames run on a timer (30 a second) not every vsync, since each frame redraws the whole view; a hidden bar's strip passes presses through.
- `crates/kit/src/ui/scrollbar_track.rs::Track`: While the thumb is dragged a list holds the height it reports to the bar, so rows measured during the drag don't move the thumb from under the pointer.
- `crates/kit/src/ui/tool_images.rs::tool_images`: The kit never reads an app bundle and no OpenAI or Google logo is stored in the repository; the app hands over installed apps' icons, and the kit falls back to its own mark.
- `crates/kit/src/ui/widgets.rs::elide mark`: The mark in a margin is laid over the text's own box, not beside it: `elide` needs the text to be a direct child of the lines column, and in any other wrapper it measures narrow and shows one word.
