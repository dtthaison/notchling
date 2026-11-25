# 0005: GPUI for every desktop app

## Context

Today's desktop app is Swift (AppKit and SwiftUI), for macOS only, about 41.7k lines. The design wants one desktop app for macOS, Linux and Windows. GPUI is a Rust UI framework, the one Zed is built on.

GPUI has gaps. It has no web view, and it can't render a view offscreen. The Phase 1 spike tested the open questions before anything moved ([0001](0001-gpui-spike.md)).

From the design: [Architecture](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md#architecture), "Desktop app" and "Web content".

## Decision

- One GPUI app, on a pinned GPUI revision, for macOS, Linux and Windows. It replaces the Swift app.
- Our own widget layer over GPUI: `crates/kit`.
- Platform pieces sit behind small traits (`crates/platform`): the notch overlay and menu bar on macOS, a tray and a top panel on Linux and Windows.
- **Web content** *(review)*: pages, artifacts and HTML files open in their own `wry` window, not inside a GPUI view, because GPUI has no web view.

## Consequences

- The spike passed S0 to S4 ([0001](0001-gpui-spike.md)). Its findings carry into the rebuild: shaders built ahead of time, the notch panel made borderless and non-activating after GPUI opens it, the panel's focus, the hit test.
- `claude attach` is proven on macOS only. Until it passes on Linux and Windows, those apps ship able to show sessions only, without sending replies (0001).
- Agents get a PNG of a view by capturing the app's own window, since GPUI can't render offscreen (part U10).
- Zed's terminal and markdown crates are GPL-3, so we write our own terminal view and markdown view ([0008](0008-mit-permissive-crates.md)).
- Before the Swift app retires, the GPUI app also needs: Vietnamese input (Telex, VNI), selection across a reply, drag and drop, file previews per OS, the keyboard, the `?` part names, pointer cursors, and screen reader support at least equal to the Swift app's.
- The Swift app moves to `legacy/macos/` and takes fixes only, until it retires in one release.

## Status

Accepted. The maintainer decided GPUI for every desktop app *(decided)*, and the spike confirmed it ([0001](0001-gpui-spike.md)). The `wry` window came from the reviews and is part of the approved revision 2.

The top panel on Linux and Windows is the designer's call and stays open until the maintainer accepts it or a later record changes it.

## Date

2026-11-07
