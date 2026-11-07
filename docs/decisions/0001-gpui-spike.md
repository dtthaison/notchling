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

S0 asks whether GPUI builds at all. The rule: the rebuild goes ahead only if S0 to S4 are all yes. A no sends the decision back to the maintainer.

The results, with evidence, are in [`spike/RESULTS.md`](../../spike/RESULTS.md). The spike code is on branch `spike/gpui-phase1` and is never merged.

## Decision

**The GPUI rebuild proceeds.** S0 through S4 are yes:

| Check | Result |
|---|---|
| S0 GPUI builds | yes |
| S1 notch overlay | yes, by an automatic check of the real panel |
| S2 web view | yes |
| S3 Vietnamese input | yes, through GPUI's input handler |
| S4 PNG of a view | yes, by capturing a window; GPUI can't render offscreen |
| S5 attach | yes on macOS, on two Macs |

S5 is unverified on Linux and Windows. Per the design, those apps ship **able to show sessions only**, without sending replies, until S5 passes there.

## Consequences

**The rebuild must carry these fixes:**

- Build GPUI's Metal shaders ahead of time on a macOS runner that has the Metal Toolchain. The spike used `runtime_shaders`.
- Make the notch panel borderless and non-activating after GPUI opens it, and set its frame afterwards. GPUI makes every window Titled, which macOS keeps below the menu bar.
- Code that calls GPUI's `NSView` through objc2 uses GPUI's `NSRange` encoding.
- Agents get screenshots of a view by capturing the app's own window. Moving from `CGWindowListCreateImage` to ScreenCaptureKit is planned work.

**Before Milestone 1 ships, a person checks what the spike checked only in code:**

- typing with the real Telex and VNI input sources;
- a real click passing through the notch to the app below;
- the notch staying visible over a full-screen app.

If any of these fails, it becomes a fix in Milestone 1, not a reason to reopen this decision. The exception is a failure that GPUI can't fix without a fork; that comes back to the maintainer.

**Still unverified:**

- Linux and Windows input methods.
- Linux and Windows attach (S5).

## Date

2026-11-07
