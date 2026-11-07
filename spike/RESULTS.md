# GPUI spike results (Phase 1)

Run on 2026-11-07. The code is on branch `spike/gpui-phase1` (`spike/gpui/`) and is never merged.

- **GPUI:** `gpui = "=0.2.2"`, with the `runtime_shaders` feature
- **Rust:** 1.99.1 (pinned in `spike/gpui/rust-toolchain.toml`)
- **Other crates:** `wry =0.57.1`, `portable-pty =0.9.0`, `objc2-app-kit =0.3.2`, `raw-window-handle =0.6.2`
- **Machines:**
  - this MacBook: macOS 26.6, built-in notch, Claude Code 2.1.319
  - `m1`: macOS 26.3, Claude Code 2.1.318

Evidence files are in `~/Desktop/notchling-spike/` on the maintainer's Mac.

## Answers

| # | Question | Result | Evidence |
|---|---|---|---|
| S0 | Does GPUI build here? | **yes** | `cargo build --workspace` passes |
| S1 | Notch overlay: a borderless, non-activating panel around the camera that lets clicks outside its shape through | **yes** (automatic check) | `s1-auto.log`, `s1-auto-open.png`, `s1-auto-collapsed.png`; unit tests `notch_rect_*`, `hit_test_edges` |
| S2 | A `wry` web view beside GPUI, in the same process | **yes** (automatic check; keys inside a page window unverified) | `s2-auto.log`; the maintainer's manual run (GPUI window took every key; example.com loaded) |
| S3 | Vietnamese Telex and VNI in a GPUI text field | **yes** through GPUI's input handler (6 of 6); with the real input source, **unverified** | `s3-auto.log` |
| S4 | A PNG of a GPUI view with no person | **yes, capture**; offscreen **no** | `s4-notch.png` |
| S5 | `claude attach` over a pty | **yes** on macOS (two Macs); Linux and Windows **unverified** | `s5-macos.log`, `s5-m1.log`; 7 unit tests in `transcript.rs` |

## What each check did, and what surprised us

### S0

**What it did:**
- Built the workspace on this Mac.

**Surprises:**
- GPUI compiles its Metal shaders at build time, and this Mac has no Metal Toolchain: *"cannot execute tool 'metal' due to missing Metal Toolchain"*.
- The spike uses GPUI's `runtime_shaders` feature instead, which compiles the shaders at launch.
- A release build should install the toolchain on its macOS runner (`xcodebuild -downloadComponent MetalToolchain`) and compile the shaders ahead of time.

### S1

**What it did** (`notch --auto` reads back the real panel, with no person needed):
- **Placement:** read the panel's frame on this Mac:
  - notch at x 665, 185 × 32 pt;
  - panel 560 × 640, centred on the notch to within the 0.5 pt AppKit rounds to;
  - top edge at the top of the screen.
- **Window settings:**
  - window level 25 (status);
  - collection behaviour includes all Spaces, stationary, full screen auxiliary and ignores-cycle;
  - not the key window, and the front app never changed.
- **Mouse beside the notch:** the panel ignores mouse events, so clicks fall through to the app below.
- **Mouse on the notch:**
  - the panel takes mouse events;
  - the shape grows to 400 × 120 (see the capture).
- **A click inside the grown shape:** a real `NSEvent` mouse-down and mouse-up sent to the window, which GPUI received once.

**Surprises:**
- **Titled windows.** GPUI makes every window *Titled*, even with `titlebar: None`. macOS keeps a titled window below the menu bar (the panel sat 33 pt too low), and such a window can become key.
- **The fix:**
  - set the style to `Borderless | NonactivatingPanel` after opening, like `NotchController`'s panel;
  - set the frame on the first timer tick, because GPUI sets the frame again after opening.
- The rebuild will need a small platform patch or wrapper for this.

**Not observed by a person:**
- **A real click reaching Finder.** Inferred from `ignoresMouseEvents`, which is how AppKit decides that.
- **Visibility over a full-screen app and on every Space.** Inferred from the collection behaviour flags, the same ones the Swift app uses.

### S2

**What it did:**
- `web --auto` opened example.com and a local page, each in its own `NSWindow` with a wry view built into it through its raw handle.
- Both rendered. The local page's script set the title, and the window's title became `loaded`.
- GPUI kept painting while they loaded: the longest gap between 20 ms ticks was 42 ms.
- Closing one page window left the app and the other page running.
- In the maintainer's manual run, the GPUI window received every key typed.

**Surprises:**
- None. wry builds into a plain `NSView` with no `tao` and doesn't fight GPUI's run loop.

**Unverified:**
- Keys typed into a page window. The window takes focus; nobody typed into it.

### S3

**What it did:**
- `ime --auto` drove GPUI 0.2.2's `examples/input.rs` text field through the same calls a macOS input method makes: `setMarkedText`, `insertText`, `unmarkText`.
- The calls followed the word-by-word marked text that Telex and VNI produce.
- Results:
  - `Việt Nam` ✓
  - `đây là tiếng Việt` ✓
  - VNI `Việt Nam` ✓
  - Esc mid-word, when the input method unmarks: `Viê` once ✓
  - Esc mid-word, when the input method clears: empty ✓
  - a click mid-word: `Viê` once ✓

**Surprises:**
- GPUI registers its `NSTextInputClient` methods with the encoding `{NSRange=QQ}`, while objc2's `NSRange` is `{_NSRange=QQ}`. objc2's debug check refuses the call.
- Anything calling GPUI's view through objc2 needs its own range type with GPUI's encoding.

**Unverified:**
- The real Vietnamese input source typed by a person. This checks GPUI's side of composition, not Apple's input method.
- A multi-line field: the example is single-line.
- Linux and Windows input methods: there's no desktop session for them.

### S4

**What it did:**
- Looked for an offscreen path. GPUI 0.2.2 has none: `TestWindow::draw` does nothing, so `VisualTestContext` produces no pixels.
- Fallback: opened the view in its own non-activating window and captured that window with `CGWindowListCreateImage`, with no permission prompt. The result is an 880 × 240 PNG at 2×.

**Surprises:**
- `CGWindowListCreateImage` is deprecated in favour of ScreenCaptureKit. The renderer should move there before Apple removes it.
- A capture needs the window on screen for a moment.

### S5

**What it did:**
- `attach --scratch` started a scratch `claude --bg` job in a folder of its own and waited for it to finish its turn.
- It attached in a `portable-pty` pty (120 × 40, `NO_COLOR` removed, no answers to terminal queries).
- It typed `ReplySender`'s clear keys, the bracketed text and Enter, with 0.15 s between them, then detached.
- It found the message in the job's transcript, then stopped and removed the job and its folder.

| Machine | Result | Clean up |
|---|---|---|
| This Mac (2.1.319) | `S5 yes` | done |
| m1 (2.1.318) | `S5 yes` | done |

**Surprises:**
- A new temporary folder would be refused as untrusted. The scratch folder sits inside a folder Claude Code already trusts (the spike folder here, `~/go` on m1), so the check never edits `~/.claude.json`.
- On m1 the first turn ended `blocked` instead of `done`. The reply still landed.

**Unverified:**
- Linux and Windows: no machine was available.

## For the rebuild

1. **Metal shaders:** install the Metal Toolchain in CI and compile the shaders at build time.
2. **Notch panel:** needs a borderless, non-activating style and its frame set after GPUI opens the window (S1).
3. **Calling GPUI's NSView through objc2:** needs GPUI's `NSRange` encoding (S3).
4. **Screenshots for agents:** come from capturing our own window, with no offscreen path. Plan the move to ScreenCaptureKit (S4).
5. **Linux and Windows:**
   - ship showing sessions only, until S5 passes on each;
   - input methods unverified there.
6. **Before Milestone 1 ships, a person checks by hand:**
   - real Telex and VNI in the app's own field;
   - a click through the notch to the app below;
   - the notch over a full-screen app.
