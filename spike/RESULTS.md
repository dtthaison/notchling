# GPUI spike results (Phase 1)

Run on 2026-11-07. The code is on branch `spike/gpui-phase1` (`spike/gpui/`) and is never merged.

- **GPUI:** `gpui = "=0.2.2"`, with the `runtime_shaders` feature
- **Rust:** 1.99.1 (pinned in `spike/gpui/rust-toolchain.toml`)
- **Other crates:** `wry =0.57.1`, `portable-pty =0.9.0`, `objc2-app-kit =0.3.2`, `raw-window-handle =0.6.2`
- **Machines:**
  - this MacBook: macOS 26.6, built-in notch, Claude Code 2.1.319
  - `m1`: macOS 26.3, Claude Code 2.1.318

Evidence files are in `~/Desktop/notchling-spike/` on the maintainer's Mac.

S1, S2 and S3 were checked two ways:
- **By hand:** the maintainer ran the plan's manual checks on 2026-11-07 and reported "S1 ok, S2 ok, S3 ok".
- **In code:** each check also has an `--auto` mode that reads back what the code can see. The limits of the automatic checks are listed with each one below.

## Answers

| # | Question | Result | Evidence |
|---|---|---|---|
| S0 | Does GPUI build here? | **yes**, with `runtime_shaders` | `cargo build --workspace`; the build that compiles shaders ahead of time wasn't tried |
| S1 | Notch overlay: a borderless, non-activating panel around the camera that lets clicks outside its shape through | **yes** | maintainer's manual check; `s1-auto.log`, `s1-auto-open.png`, `s1-auto-collapsed.png`; unit tests `notch_rect_*`, `hit_test_*` |
| S2 | A `wry` web view beside GPUI, in the same process | **yes** | maintainer's manual check; `s2-auto.log` |
| S3 | Vietnamese Telex and VNI in a GPUI text field | **yes** on macOS's own input source; Linux and Windows **unverified** | maintainer's manual check; `s3-auto.log` |
| S4 | A PNG of a GPUI view with no person | **yes, by capture**; offscreen **no** | `s4-notch.png` |
| S5 | `claude attach` over a pty | **yes** on macOS (one run on each of two Macs); Linux and Windows **unverified** | `s5-macos.log`, `s5-m1.log`; 8 unit tests in `transcript.rs` |

## What each check did, and what surprised us

### S0

**What it did:**
- Built the workspace on this Mac.

**Surprises:**
- GPUI compiles its Metal shaders at build time, and this Mac has no Metal Toolchain: *"cannot execute tool 'metal' due to missing Metal Toolchain"*.
- The spike uses GPUI's `runtime_shaders` feature instead, which compiles the shaders at launch.
- A release build should install the toolchain on its macOS runner (`xcodebuild -downloadComponent MetalToolchain`) and compile the shaders ahead of time. That build wasn't tried here.

### S1

**What the automatic check (`notch --auto`) read back:**
- **Placement:**
  - notch at x 665, 185 × 32 pt;
  - panel 560 × 640, centred on it to within AppKit's 0.5 pt rounding;
  - top edge at the top of the screen.
- **Window settings:**
  - level 25 (status);
  - collection behaviour includes all Spaces, stationary, full screen auxiliary and ignores-cycle.
- **Mouse beside the notch:** the hit test set `ignoresMouseEvents`, which is what AppKit uses for click-through. The check sent no real click there.
- **Mouse on the notch:** the hit test cleared it, and the shape grew to 400 × 120 (see the captures).
- **A click inside:** a mouse-down and mouse-up posted inside the process with `sendEvent:` reached GPUI's handler once. That path skips the window server, so it doesn't show that a real click is routed to the panel.
- **Focus:** the panel reported itself as **key** at launch, while Chrome stayed the front app.

**What the maintainer checked by hand:**
- the plan's S1 list: the shape around the camera, a click beside it reaching Finder, hover and click inside, full-screen apps and Spaces, and focus never leaving the front app;
- all passed.

**Surprises:**
- **Titled windows.** GPUI makes every window *Titled*, even with `titlebar: None`. macOS keeps a titled window below the menu bar, so the panel sat 33 pt too low.
- **The fix:**
  - set the style to `Borderless | NonactivatingPanel` after opening, like `NotchController`'s panel;
  - set the frame on the first timer tick, because GPUI sets the frame again after opening.
- **Key window.** GPUI's pop-up panel always answers yes to `canBecomeKeyWindow`, and it became key at launch. A key non-activating panel can take typing without changing the front app. The maintainer saw no stolen typing. Even so, the rebuild should make the panel key only while the user replies, as `NotchController` calls `makeKey` only in its detail state.
- **Hit test top edge.** The top edge of the hit test was half-open: a pointer pushed to the top of the screen reports y equal to the shape's top and was counted outside. Fixed to count it inside, as `NSMouseInRect` does (test `hit_test_top_edge_is_inside`).

**Not run:**
- A display without a notch. The spike's fallback is a fixed 200 × 24 strip, while the Swift app uses `max(24, menu bar height)`. Only `notch_rect` returning `None` is tested.

### S2

**What the automatic check (`web --auto`) showed:**
- It opened example.com and a local page, each in its own `NSWindow` with a wry view built into it through its raw handle.
- Both loaded and ran their script: the titles changed, and the local page's became `loaded`.
- After the views were created, the main thread answered a 20 ms timer within 42 ms. The time spent creating the two web views wasn't measured.
- Closing one page window left the app and the other page running.

**What the maintainer checked by hand:**
- both pages render;
- closing a page window doesn't quit the app;
- the GPUI window stays responsive;
- keys go to whichever window is focused.

**Surprises:**
- None. wry builds into a plain `NSView` with no `tao` and doesn't fight GPUI's run loop.

### S3

**What the automatic check (`ime --auto`) showed:**
- It called GPUI 0.2.2's `examples/input.rs` text field directly with `setMarkedText`, `insertText` and `unmarkText`, in sequences modelled on what Telex produces (the VNI case is the same sequence).
- `Việt Nam`, `đây là tiếng Việt`, and `unmarkText` mid-word (what an input method calls on Esc or a click) giving `Viê` once all worked.
- It doesn't exercise the real path (`keyDown` → GPUI's key handling → the input context), Esc keys, or clicks. The sequences are modelled, not recorded.

**What the maintainer checked by hand:**
- the plan's five lines with macOS's Vietnamese Telex and VNI;
- all passed.

**Surprises:**
- GPUI registers its `NSTextInputClient` methods with the encoding `{NSRange=QQ}`, while objc2's `NSRange` is `{_NSRange=QQ}`. objc2's debug check refuses the call.
- Anything calling GPUI's view through objc2 needs its own range type with GPUI's encoding.

**Unverified:**
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
- `attach --scratch` started a scratch `claude --bg` job in a folder of its own and waited for it to leave `working`.
- It attached in a `portable-pty` pty (120 × 40, `NO_COLOR` removed, no answers to terminal queries).
- It typed `ReplySender`'s clear keys, the bracketed text and Enter, with 0.15 s between them, then detached.
- It found the message in the job's transcript, then stopped and removed the job and its folder.
- Cleanup stops or removes a job only when that job's `cwd` is the scratch folder.

| Machine | Result | Clean up |
|---|---|---|
| This Mac (2.1.319) | `S5 yes`, run twice | done each time |
| m1 (2.1.318) | `S5 yes`, run once | done |

**Surprises:**
- A new temporary folder would be refused as untrusted. The scratch folder sits inside a folder Claude Code already trusts (the spike folder here, `~/go` on m1), so the check never edits `~/.claude.json`.
- On m1 the first turn ended `blocked`, not `done`, and the spike typed into it anyway. It has no dialog check like `ReplySender`'s, and the reply still landed. The rebuild must keep `ReplySender`'s check that no dialog is open before typing.

**Gaps:**
- Ctrl+C during the waits skips the cleanup. Run it to the end, or remove the `spike-attach` job by hand.
- Linux and Windows: no machine was available.

## For the rebuild

1. **Metal shaders:** install the Metal Toolchain in CI and compile the shaders at build time.
2. **Notch panel:**
   - borderless and non-activating, with its frame set after GPUI opens it;
   - key only while the user replies;
   - hit test top edge counted inside (S1).
3. **Calling GPUI's NSView through objc2:** needs GPUI's `NSRange` encoding (S3).
4. **Screenshots for agents:** come from capturing our own window, with no offscreen path. Plan the move to ScreenCaptureKit (S4).
5. **Replies:** keep `ReplySender`'s dialog check before typing into a session (S5).
6. **Linux and Windows:**
   - ship showing sessions only, until S5 passes on each;
   - input methods unverified there.
