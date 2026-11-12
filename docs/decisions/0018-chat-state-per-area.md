# 0018: Chat's state per screen area, the transcript as a log, selection on the model, the conversation as a list

## Context

Chat (`crates/app/src/chat/view/`) is one GPUI entity, `ChatView` (`mod.rs:113-251`, 66 fields), that every part of the window reads. Three parts are cached views (`pane.rs`: the conversation, the Sidebar, the Details Panel), and each one observes `ChatView` (`pane.rs:61`, `cx.observe(chat, |_, _, cx| cx.notify())`). Facts measured in the code on 2026-11-12:

- `ChatView` has 122 `cx.notify()` calls. Every one draws all three parts again, whatever changed. So does every keystroke in the reply box (`tools.rs:207`, `draft_changed`), every terminal frame (`terminal.rs:388`, up to 30 a second while a shell prints), every iOS Simulator frame (`simulator/mod.rs:391`), and the Copy check and hover clock in `tick` (`mod.rs:805-813`).
- A cached view draws all of itself or none of itself. The conversation's part builds every row (up to 500 items, `Parser` keeps `KEEP_ITEMS`) and lays them out on each draw. GPUI also draws it again on every wheel scroll and every hover change inside it, since a `div` notifies the view it is in (`vendor/gpui/src/elements/div.rs`, scroll and group-hover handlers). So scrolling and hovering lay out the whole conversation every frame.
- `window.refresh()` drops every view cache in the window. Text selection calls it on each mouse move of a drag (`select.rs:196, 219, 241`), the kit scroll bar on each drag step and hover change (`kit/src/ui/scrollbar.rs:316, 327, 363, 377`), the terminal's own selection (`terminal.rs:677, 739, 768, 829`), and a Details Panel fold (`inspector.rs:231`).
- A load clones the parser's conversation (`parser.rs:517`), sends it to the main thread, and compares it item by item with the one on show (`mod.rs:671`). The engine already reads only the bytes appended since the last read (`transcript/tail.rs`, `Cursors`); the copies and the comparison are what cost.
- Selection is kept in the order texts were drawn (`select.rs`, a thread-local list of notes made at prepaint). Copy reads the notes, so it only works while every message is drawn. That rules out drawing only the rows on screen.
- The pending question is read twice per load, once by the conversation reader and once by the notch's snapshot reader (`loader.rs:123-126`).

The fixes so far (cached parts, the spinner layer, `redraw_chrome`) are recorded in `crates/AGENTS.md` ("Things that bit before"). What is left comes from the shape of the state, not from one hot spot.

## Decision

1. **Each screen area draws from its own view model.** The Sidebar and the Details Panel stay cached parts but redraw only when their model changes: on each `ChatView` notify a part computes its model (`SidebarModel`: the sections, the row accounts, the selected row, the daemon note; `DetailsModel`: the `Inspector`, the folder, the subagent ids) and draws only when it differs from the last one (`PartialEq`). The draw function receives the model, not `ChatView`, so it cannot read a field the model leaves out. The conversation becomes an entity of its own, `ConversationView`, that owns what only it uses (scroll and follow state, the Markdown cache, open groups, hover, Copy check, selection, the Latest pill) and draws from a `ConversationInput` with the same equality gate. `ChatView` stays the controller: sessions, the selected row, drafts, tools and panels, since about 60 sites across the window read them and moving them would add plumbing and redraw nothing less.
2. **The transcript is a log.** The engine hands out `Arc<Conversation>` from its cursors (`read_shared`; `read` stays for its callers), so an unchanged file costs a pointer copy and a pointer compare. The loader compares each new conversation with the last one on the background thread and sends a change description with it (`chat::log::Change`: same, splices of items by id, or reset). The pending question is found by the conversation reader (`Conversation::pending_question`), so one reader serves both.
3. **Selection is anchored to the model.** A selection end is a row id, a block index within the row and a byte offset. A pure `plain_blocks(row)` gives each row's selectable texts in the order the row draws them, and a render check pins the two together. Copy comes from the model, so it covers rows that are not drawn. The selection belongs to `ConversationView` and a drag notifies that view, never `window.refresh()`.
4. **The conversation is a GPUI `list`** (`ListAlignment::Bottom`), fed by the change description as splices, so a draw lays out only the rows on screen. A spike over a probe view decides go or no-go before any of it lands (follow, anchor drift, rows per frame, resize); the fallback is a windowed column with our own height cache.
5. **Small changes first:** terminal and simulator frames, typing and the slash menu redraw only the window's outer parts (`redraw_chrome`); the notch redraws only when what it draws changed; the scroll bar, the Details Panel fold and selection notify the view they are in instead of refreshing the window.
6. **Measure before and after.** `--render chat` gets bench modes (`NOTCHLING_RENDER_BENCH_MODE`: idle, scroll, hover, select, typing, terminal, reload, menu) over a generated 200-turn conversation, with counters for draws per part, rows drawn and bytes parsed. Each step records its numbers, and count-based tests hold the gains: a terminal frame draws no part, an appended item draws only the rows on screen and parses only the new bytes.

The plan is [`docs/superpowers/plans/2026-11-12-chat-state-split.md`](../superpowers/plans/2026-11-12-chat-state-split.md).

## Consequences

- Typing, terminal output and simulator frames stop laying out the conversation, the Sidebar and the Details Panel. Scrolling, hovering and selecting lay out only the rows on screen. The bench says by how much; the target is that no common action costs in proportion to the conversation's length.
- A view model can miss a field the draw needs, and the part then shows a stale value. Because a draw function takes only its model, the compiler catches a read of `ChatView`; what it cannot catch (a global, the appearance, the fold state) is redrawn explicitly, and the render tests and the bench's counters watch it.
- A list measures a row only when it is drawn. Rows never drawn count as zero height, so the scroll bar's thumb is approximate until they are, and "at the bottom" is decided from the list's following state and the last row's bounds, not from pixel sums.
- Selection works across the conversation only. A drag can no longer run from the conversation into the file panel; the file panel keeps its own selection, and ⌘C copies whichever has one.
- Item ids are renumbered when the engine starts a fresh cursor on a file (replaced, truncated, or grown past 4 MB at once). The change description then says reset, and the list starts over at the bottom.
- `Conversation` gains one field, `pending_question`, serialized only when present, so the daemon's `agy.conversation` answers are unchanged.
- The work is ordered so each step leaves the app building and behaving as before; behavior-preserving moves land before behavior changes.

## Rejected alternatives

- **A cached view per message.** A cached view's box comes only from the style given to `cached()` (`vendor/gpui/src/view.rs:169-175`): on a cache hit GPUI never measures its content. Each message would need its height from somewhere else, which is the height cache a list already keeps, plus one entity per message to create and drop.
- **Keeping the selection in draw order.** Copy would still need every selectable text drawn each frame, which is what a list stops doing, and a drag would still have to redraw everything that notes texts.
- **One Redux-style store.** One store that every view observes is the current shape with a new name: each change still wakes every view, and each view still needs a selector with an equality gate, which is decision 1 without the rewrite of 122 notify sites.
- **A separate entity for the session list.** Sessions, the selected row and drafts are read across the window (about 60 sites), so moving them would thread another context through those calls and redraw nothing less than the per-part view models do.
- **An `Arc` per item in `Conversation`.** It changes the daemon's wire type, needs serde's `rc` feature (a protected `Cargo.toml` change) and touches every parser and many tests, for a gain the `Arc` around the whole conversation and the change description already give.
- **Skipping reloads by which paths the watcher reported.** The Sidebar shows other sessions' state (ages and status come from their files), so a reload can't skip their changes. The `Arc` pointer compare already makes a reload that didn't touch the open session cost nothing in the conversation.
- **`uniform_list`.** Rows differ in height by orders of magnitude.
- **Patching GPUI's list in `vendor/`.** `vendor/` is protected, and the spike shows whether it is needed. It stays the second fallback, the maintainer's call.

## Status

Proposed. The maintainer settles it by merging it. The list spike (Task 2.1 of the plan) can still send the conversation to its fallback, a windowed column with our own height cache; that would amend decision 4 in a new record.

## Date

2026-11-12
