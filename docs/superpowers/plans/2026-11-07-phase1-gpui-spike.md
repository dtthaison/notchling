# Phase 1: GPUI Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Answer five yes/no questions that decide whether the GPUI rebuild goes ahead as designed, and record the answers in the repo's first decision record.

**Architecture:** A throwaway Cargo workspace in `spike/gpui/` with one small binary per question. Pure logic (notch geometry, hit testing, transcript matching) gets unit tests. Everything else is a manual or scripted check with a written pass rule. The code is never merged. Only the results and the decision record are merged.

**Tech Stack:** Rust (exact stable version pinned in Task 1), `gpui = "=0.2.2"`, `wry = "=0.57.1"`, `portable-pty = "=0.9.0"`, `objc2-app-kit = "=0.3.2"`, `raw-window-handle = "=0.6.2"`, macOS 26.6 (this Mac, built-in notch), Claude Code 2.1.319.

**Spec:** `docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md`, revision 2, sections "Architecture", "Parts" (U2, U4, U10, E3) and "Phases" (Phase 1).

## Global Constraints

- Throwaway code: branch `spike/gpui-phase1`, folder `spike/gpui/`. It is never merged into `main`.
- Only `docs/decisions/0001-gpui-spike.md` and `spike/RESULTS.md` go to `main`, through a PR.
- License: only MIT, Apache-2.0 or BSD crates. No Zed crate other than `gpui`; Zed's `terminal`, `terminal_view` and `markdown` are GPL-3.
- Pin every crate with `=` exactly as listed under Tech Stack.
- Never touch the maintainer's real sessions. S5 uses only a scratch session it creates and removes itself.
- No Keychain, no account logins, no edits to `~/.claude/settings.json`.
- Each question ends in **yes**, **no** or **unverified**, each with evidence: a screenshot path, a log path or a test output. "Probably" is not a result.
- Screenshots go to `~/Desktop/notchling-spike/`. This environment can't take screenshots, so the maintainer saves them there and the agent reads them.

## Review Focus

- **No notch on the display, or an external monitor only.** `notch_rect` must return `None`, and the overlay must fall back to a 24 pt strip under the menu bar, as `NotchController.screensChanged` does today. Pinned by `notch_rect_none_without_notch` in Task 2.
- **Mouse exactly on the shape's edge.** The click-through toggle must be stable, with no flicker on the boundary pixel. Pinned by `hit_test_edges` in Task 2. Today's code adds 4 pt of slack below the collapsed strip.
- **A transcript that is still being written** (partial last line) or is larger than 2 MB. The S5 matcher must read only the tail and skip a partial line, not crash. Pinned by `matcher_ignores_partial_line` in Task 5.
- **The scratch session fails to start**, for example because the folder isn't trusted. S5 must clean up (`claude stop`, `claude rm`) on every exit path and report "unverified", not leave a job behind. Pinned by the cleanup step in Task 5.
- **Vietnamese composition interrupted** (Esc or a click mid-word). The marked text must be cancelled or committed, never duplicated. Checked in Task 4's manual script, line "Esc mid-word".

---

### Task 1: Spike workspace

**Files:**
- Create: `spike/gpui/Cargo.toml` (workspace), `spike/gpui/rust-toolchain.toml`, `spike/gpui/README.md`
- Create: `spike/gpui/spike-core/` (lib crate for pure logic), `spike/gpui/notch/`, `spike/gpui/web/`, `spike/gpui/ime/`, `spike/gpui/render/`, `spike/gpui/attach/` (bin crates)

**Interfaces:**
- Produces: a workspace where `cargo build --workspace` succeeds and `cargo test -p spike-core` runs.

- [ ] **Step 1:** Create the branch: `git switch -c spike/gpui-phase1`.
- [ ] **Step 2:** Run `rustup toolchain install stable --profile minimal` (this Mac has rustup but no default toolchain). Write the exact version it installed into `rust-toolchain.toml` as `channel = "<x.y.z>"`, with `components = ["clippy", "rustfmt"]`.
- [ ] **Step 3:** Create the workspace with the six members. Each bin crate has an empty `main` that prints its name.
  - Add the crate pins from Tech Stack only to the crates that use them: `gpui` in notch, web, ime and render; `wry` in web; `objc2-app-kit` and `raw-window-handle` in notch and web; `portable-pty` in attach.
- [ ] **Step 4:** Run `cargo build --workspace`. Expected: it builds. If `gpui 0.2.2` fails to build on this Mac, stop and record S0 = **no** with the error. The rest of the spike depends on it.
- [ ] **Step 5:** Commit: `git add spike/gpui && git commit -m "spike: GPUI spike workspace"`.

### Task 2: S1, the notch overlay

The question: can a GPUI window be a borderless, transparent, non-activating panel around the camera that ignores clicks outside a shape and takes them inside it, like `NotchController` does today?

**Files:**
- Create: `spike/gpui/spike-core/src/notch.rs`, `spike/gpui/notch/src/main.rs`
- Test: `spike/gpui/spike-core/src/notch.rs` (unit tests in the same file)

**Interfaces:**
- Produces:
  - `pub struct ScreenInfo { pub frame: Rect, pub safe_top: f64, pub aux_left_width: Option<f64>, pub aux_right_width: Option<f64> }`
  - `pub struct Rect { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }`, with y measured from the bottom as in AppKit
  - `pub fn notch_rect(s: &ScreenInfo) -> Option<Rect>`
  - `pub fn hit_test(shape: Rect, mouse: (f64, f64), slack_below: f64) -> bool`

- [ ] **Step 1: Write the failing tests.** They are taken from `NotchController.screensChanged` and `shapeRect`:

```rust
#[test] fn notch_rect_on_macbook() {
    let s = ScreenInfo { frame: Rect{x:0.,y:0.,w:1512.,h:982.}, safe_top: 32.,
                         aux_left_width: Some(662.), aux_right_width: Some(662.) };
    assert_eq!(notch_rect(&s), Some(Rect{x:662.,y:950.,w:188.,h:32.}));
}
#[test] fn notch_rect_none_without_notch() {
    let s = ScreenInfo { frame: Rect{x:0.,y:0.,w:2560.,h:1440.}, safe_top: 0., aux_left_width: None, aux_right_width: None };
    assert_eq!(notch_rect(&s), None);
}
#[test] fn hit_test_edges() {
    let shape = Rect{x:100.,y:900.,w:200.,h:32.};
    assert!(hit_test(shape, (100., 900.), 0.));        // corner inside
    assert!(!hit_test(shape, (300., 916.), 0.));       // right edge is outside (half-open)
    assert!(hit_test(shape, (150., 897.), 4.));        // inside the 4 pt slack below
    assert!(!hit_test(shape, (150., 895.), 4.));
}
```

- [ ] **Step 2:** Run `cargo test -p spike-core notch`. Expected: FAIL, functions not defined.
- [ ] **Step 3:** Implement `notch_rect` and `hit_test` in `spike-core/src/notch.rs`.
  - `notch_rect`: the notch width is the frame width minus both aux widths, and it sits at the top of the frame.
  - `hit_test`: half-open on the right and top edges, and the shape grows downward by `slack_below`.
- [ ] **Step 4:** Run `cargo test -p spike-core notch`. Expected: PASS.
- [ ] **Step 5:** Implement `notch/src/main.rs`:
  - Open a GPUI window with:
    - `titlebar: None`
    - `kind: WindowKind::PopUp`
    - `window_background: WindowBackgroundAppearance::Transparent`
    - `focus: false`
    - `is_movable: false`
    - bounds `notch_rect` widened to 560 × 640 pt
  - Draw a black rounded shape the size of the notch, growing to 400 × 120 pt while hovered.
  - Through `raw-window-handle` get the `NSView`, then its `NSWindow` (with `objc2-app-kit`), and set:
    - `level = NSStatusWindowLevel`
    - collection behavior `canJoinAllSpaces | stationary | fullScreenAuxiliary | ignoresCycle`
    - `hasShadow = false`
  - Every 50 ms, read `NSEvent::mouseLocation`, call `hit_test`, and set `ignoresMouseEvents(!inside)`.
  - If `gpui::Window` doesn't expose a raw window handle, find the window through `NSApp.windows()` by its title `"notchling-spike-notch"`.
- [ ] **Step 6: Manual check.** Run `cargo run -p notch`, then save `~/Desktop/notchling-spike/s1-*.png`.
  - **Pass (yes)** needs all of these:
    - the shape sits exactly around the camera;
    - a click on the desktop beside it reaches Finder;
    - hovering grows the shape and a click inside it is received (log it);
    - it stays visible over a full-screen app and on every Space;
    - it never takes focus from the front app.
  - Any of these failing is **no**, with the failing item named.
- [ ] **Step 7:** Commit: `git commit -am "spike: S1 notch overlay"`.

### Task 3: S2, a web view beside GPUI

The question: can a GPUI app open pages in a `wry` web view in the same process without fighting over the run loop?

**Files:**
- Create: `spike/gpui/web/src/main.rs`

**Interfaces:**
- Consumes: the NSWindow lookup from Task 2, copied (throwaway code, no shared crate needed).

- [ ] **Step 1:** Implement `web/src/main.rs`:
  - Open a normal GPUI window with a button "Open Page".
  - On click, create a separate `NSWindow` through `objc2-app-kit` (800 × 600, titled `"Page"`).
  - Build a `wry::WebViewBuilder` into it with `build(&window_handle)`, using the raw-window-handle form, not `tao`.
  - Load `https://example.com` and then a local file, `spike/gpui/web/test.html`. The file's script sets `document.title` to `"loaded"`.
- [ ] **Step 2: Manual check.** Run `cargo run -p web`, click Open Page twice, close one page window, and type in the GPUI window.
  - **Pass (yes)** needs all of these:
    - both pages render, and the window title becomes "loaded";
    - closing a page window doesn't quit the app;
    - the GPUI window stays responsive while a page loads;
    - keyboard input goes to whichever window is focused.
  - Save `s2-*.png`.
- [ ] **Step 3:** Commit: `git commit -am "spike: S2 wry window beside GPUI"`.

### Task 4: S3, Vietnamese input

The question: does a GPUI text field take macOS's built-in Vietnamese Telex and VNI input correctly?

**Files:**
- Create: `spike/gpui/ime/src/main.rs`, adapted from GPUI 0.2.2's `examples/input.rs`, which implements `EntityInputHandler`

- [ ] **Step 1:** Implement a window with one multi-line text field and a label below it that shows the committed text and its UTF-8 bytes.
- [ ] **Step 2: Manual script.** Turn on System Settings › Keyboard › Input Sources › Vietnamese (Telex and VNI). Run `cargo run -p ime` and type each line below. The label must show exactly the expected text:

| Input source | Keys | Expected |
| --- | --- | --- |
| Telex | `Vieejt Nam` | `Việt Nam` |
| Telex | `ddaay laf tieesng Vieejt` | `đây là tiếng Việt` |
| VNI | `Vie65t Nam` | `Việt Nam` |
| Telex | `Vieej` then Esc mid-word | either `Viê` or nothing, never doubled |
| Telex | `Vieej`, then click elsewhere in the field | `Viê` committed once |

  - **Pass (yes)** needs all five lines to match, with the candidate underline shown while composing.
  - Linux and Windows input methods are **unverified** in this spike: there's no desktop session for them. Record that.
- [ ] **Step 3:** Commit: `git commit -am "spike: S3 Vietnamese input"`.

### Task 5: S5, `claude attach` over a pty

The question: can Rust drive a background Claude Code session the way `ReplySender` does? On macOS and Linux that means a Unix pty, and on Windows it means ConPTY.

**Files:**
- Create: `spike/gpui/spike-core/src/transcript.rs`, `spike/gpui/attach/src/main.rs`
- Test: `spike/gpui/spike-core/src/transcript.rs`

**Interfaces:**
- Produces:
  - `pub fn tail_has_user_text(path: &Path, needle: &str, max_bytes: u64) -> io::Result<bool>`: true when a complete JSON line of `"type":"user"` in the last `max_bytes` contains `needle` in its message content.
  - `pub const CLEAR_INPUT: &str`: exactly `ReplySender.clearInput`, which is 20 × `"\u{05}\u{15}\u{7F}"` followed by `"\u{05}\u{15}"`.
  - `pub fn bracketed(text: &str) -> String`, which returns `"\u{1b}[200~" + text + "\u{1b}[201~"`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn matcher_finds_user_line() {
    let f = tmp_with("{\"type\":\"user\",\"message\":{\"content\":\"spike ping abc123\"}}\n");
    assert!(tail_has_user_text(f.path(), "abc123", 2_000_000).unwrap());
}
#[test] fn matcher_ignores_partial_line() {
    let f = tmp_with("{\"type\":\"user\",\"message\":{\"content\":\"spike ping abc123\"");
    assert!(!tail_has_user_text(f.path(), "abc123", 2_000_000).unwrap());
}
#[test] fn matcher_ignores_assistant_echo() {
    let f = tmp_with("{\"type\":\"assistant\",\"message\":{\"content\":\"spike ping abc123\"}}\n");
    assert!(!tail_has_user_text(f.path(), "abc123", 2_000_000).unwrap());
}
#[test] fn clear_input_matches_swift() {
    assert_eq!(CLEAR_INPUT.len(), 20 * 3 + 2);
    assert!(CLEAR_INPUT.starts_with("\u{05}\u{15}\u{7F}") && CLEAR_INPUT.ends_with("\u{05}\u{15}"));
}
```

- [ ] **Step 2:** Run `cargo test -p spike-core transcript`. Expected: FAIL.
- [ ] **Step 3:** Implement `tail_has_user_text`, `CLEAR_INPUT` and `bracketed` in `spike-core/src/transcript.rs`.
  - Read only the last `max_bytes`, drop the first line (it may start mid-line) and drop the last line if it doesn't end in `\n`.
  - The message content may be a string or an array of `{type:"text", text}` blocks.
- [ ] **Step 4:** Run `cargo test -p spike-core transcript`. Expected: PASS.
- [ ] **Step 5:** Implement `attach/src/main.rs`. Usage: `attach --scratch`.
  1. Make a temp folder and run `claude --bg --name spike-attach "say ok"` there. If the folder is refused as untrusted, report **unverified** with that reason.
  2. Read the new job id from `claude --bg`'s output.
  3. Wait until the job's `state.json` says it is idle.
  4. Spawn `claude attach <id>` in a `portable-pty` pty, 120 × 40, without `NO_COLOR`, never answering device-attribute queries.
  5. Wait 1.2 s for the screen to settle, then write `CLEAR_INPUT`, `bracketed("spike ping <uuid>")` and `"\r"`, with a 0.15 s pause between the three.
  6. Detach, then poll the job's transcript with `tail_has_user_text` for 60 s.
  7. On every exit path, run `claude stop <id>` and `claude rm <id>`, and delete the temp folder.
  - Print `S5 yes`, `S5 no: <reason>` or `S5 unverified: <reason>`.
- [ ] **Step 6: Run on macOS.** Run `cargo run -p attach -- --scratch`. Expected: `S5 yes`. Save the log to `~/Desktop/notchling-spike/s5-macos.log`.
- [ ] **Step 7: Run on `m1`** *(maintainer: use m1, not vps-sing-1)*. `m1` is a macOS worker, so this checks a second Mac with its own Claude login. Copy the workspace there, run `cargo run -p attach -- --scratch` and save `s5-m1.log`. Ask the maintainer before running it, because it uses that machine's Claude login for one short turn. Linux stays **unverified** in this spike, since there's no Linux machine to use; per the design, Linux then ships showing sessions only until this check passes there.
- [ ] **Step 8: Run on Windows.** On a Windows machine the maintainer provides, with Claude Code signed in, run the same command and save `s5-windows.log`. If there's no Windows machine before the spike ends, record Windows as **unverified**. Per the design, Windows then ships able to show sessions only, until this check passes.
- [ ] **Step 9:** Commit: `git commit -am "spike: S5 attach over pty"`.

### Task 6: S4, rendering a view to PNG for agents

The question: can an agent get a PNG of a GPUI view without a person, the way `render-notch-view` works today?

**Files:**
- Create: `spike/gpui/render/src/main.rs`

- [ ] **Step 1:** Try offscreen first. Look for an offscreen or test render path in GPUI 0.2.2: grep its source for `capture`, `screenshot`, `TestAppContext` and `VisualTestContext`. If one exists, render the Task 2 shape at its expanded size to `out/notch.png` with no window shown.
- [ ] **Step 2:** If there's no offscreen path, use the fallback. Open the window off-screen-edge but on screen, then capture it with `CGWindowListCreateImage` for its window number, through `objc2`. Capturing other apps' windows needs Screen Recording permission; capturing our own doesn't. Write the image to `out/notch.png`.
- [ ] **Step 3: Check.** Run `cargo run -p render`, then read `out/notch.png`.
  - **Pass (yes, offscreen)**: Step 1 works with no window shown.
  - **Pass (yes, capture)**: Step 2 works with no permission prompt.
  - **No**: neither works. Record which one passed.
- [ ] **Step 4:** Commit: `git commit -am "spike: S4 render to PNG"`.

### Task 7: Record the results and the decision

**Files:**
- Create: `spike/RESULTS.md`, `docs/decisions/0001-gpui-spike.md`

- [ ] **Step 1:** Write `spike/RESULTS.md`:
  - A table with S0 (GPUI builds) and S1 through S5.
  - For each: the result (yes, no or unverified), the evidence paths, and what surprised you.
  - Note the GPUI version and the Rust version used.
- [ ] **Step 2:** Write `docs/decisions/0001-gpui-spike.md` with four sections:
  - **Context:** link the design's revision 2.
  - **Decision:** "GPUI rebuild proceeds" only if S0–S4 are all yes. Any **no** says "maintainer decides again" and names the failed item. S5 Windows unverified means "Windows ships showing sessions only".
  - **Consequences.**
  - **Date.**
- [ ] **Step 3:** Commit both on a new branch from `main` (not the spike branch):
  - `git switch main && git switch -c docs/gpui-spike-results`
  - check out the two files from `spike/gpui-phase1`
  - commit them, then open a PR titled "GPUI spike results".
- [ ] **Step 4:** Push the spike branch too, so the code survives, but don't open a PR for it: `git push -u origin spike/gpui-phase1`.
