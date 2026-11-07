# 0001: GPUI spike

## Context

[Revision 2 of the design](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md), approved on 2026-11-07, makes these choices:

- rebuild Notchling's engine in Rust;
- rebuild every desktop app on GPUI, replacing the Swift Mac app;
- keep the Flutter phone app.

Phase 1 of that design is a throwaway spike. It answers five questions before any of the rebuild starts:

- S1: notch overlay
- S2: web view beside GPUI
- S3: Vietnamese input
- S4: a PNG of a view for agents
- S5: `claude attach` over a pty

S0 asks whether GPUI builds at all. The rule: the rebuild goes ahead only if S0 to S4 are all yes. Any no sends the decision back to the maintainer.

The results, with evidence, are in [`spike/RESULTS.md`](../../spike/RESULTS.md). The spike code is on branch `spike/gpui-phase1` and is never merged.

## Decision

**The GPUI rebuild proceeds.** S0 through S4 are yes:

| Check | Result |
|---|---|
| S0 GPUI builds | yes, with GPUI's `runtime_shaders` |
| S1 notch overlay | yes, checked by hand by the maintainer on 2026-11-07 and read back by an automatic check |
| S2 web view | yes, checked by hand and by an automatic check |
| S3 Vietnamese input | yes with macOS's own Telex and VNI, checked by hand; the automatic check covers only GPUI's input handler |
| S4 PNG of a view | yes, by capturing a window; GPUI can't render offscreen |
| S5 attach | yes on macOS, one run on each of two Macs |

S5 is unverified on Linux and Windows. The design says Linux and Windows control their own sessions; the Phase 1 plan narrows that until S5 passes there. So until then, those apps ship **able to show sessions only**, without sending replies.

## Consequences

**The rebuild carries these findings:**

- **Shaders.** Build GPUI's Metal shaders ahead of time on a macOS runner that has the Metal Toolchain. The spike used `runtime_shaders` and didn't try the ahead-of-time build.
- **Notch panel position.** Make the panel borderless and non-activating after GPUI opens it, and set its frame afterwards. GPUI makes every window Titled, which macOS keeps below the menu bar.
- **Notch panel focus.** Let the panel become key only while the user replies. GPUI's pop-up panel can always become key, and did at launch.
- **Hit test.** Count the top edge of the notch's hit test as inside, as `NSMouseInRect` does.
- **objc2 calls.** Code that calls GPUI's `NSView` through objc2 uses GPUI's `NSRange` encoding.
- **Screenshots for agents.** Agents get screenshots of a view by capturing the app's own window. Moving from `CGWindowListCreateImage` to ScreenCaptureKit is planned work.
- **Replies.** Keep `ReplySender`'s check that no dialog is open before typing into a session.

**Still unverified:**

- a display without a notch;
- a multi-line text field with Vietnamese input;
- Linux and Windows input methods;
- Linux and Windows attach (S5).

A failure found in any of these later goes back to the maintainer, as the spec says.

## Date

2026-11-07
