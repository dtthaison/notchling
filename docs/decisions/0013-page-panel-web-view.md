# 0013: A web view in Chat's page panel, and the live claude.ai page

## Context

[0005](0005-gpui-desktop-app.md) put web content in its own `wry` window, not inside a GPUI view, because GPUI has no web view. The Swift app opens an Artifact clicked in a conversation in a panel beside the chat (`PagePanel`), and the maintainer asked on 2026-11-10 for the GPUI Chat window to do the same: the saved local copy when Notchling has one, otherwise the live claude.ai page, in the panel. He approved the network use that the live page needs with that request.

## Decision

- A clicked Artifact link (`claude.ai/artifact/<id>`, `claude.ai/code/artifact/<uuid>`) opens in a page panel in the Details Panel's place. Any other link still goes to the browser. `crates/app/src/chat/page_panel.rs` decides this and is tested.
- The page is a `wry` web view built as a child of the Chat window's content view, above GPUI's own view. Each frame a canvas in the panel reports the panel's bounds, and the web view's frame follows them, turned to AppKit's coordinates and put on device pixels. It hides while a sheet, menu or the part names are up, because GPUI draws those under it. Page windows (0005) stay as they are, and both share one builder (`pages/web.rs`).
- A local copy keeps 0005's rules: read access to its own folder only, no navigation out of it, no new windows.
- A live page loads only claude.ai and the hosts its pages need (`*.claude.ai`, `*.claudeusercontent.com`, `challenges.cloudflare.com`), over https and in every frame. A link it opens in a new window goes to the browser. Its sign-in is kept in a website data store of its own (a fixed id, macOS 14 and later), apart from the store local copies use.

## Consequences

- A live claude.ai page now loads inside the app: network use from the app process, which 0005 didn't have. The first live page may ask the user to sign in to claude.ai in the panel. Google sign-in usually refuses embedded web views and leaves for accounts.google.com, which the panel refuses, so sign-in there is by email.
- Navigation within a frame to any other site is refused silently. WebKit's navigation callback as `wry` gives it doesn't say which frame asked or whether there was a click, so sending refused navigations to the browser would also open every third-party frame a page embeds.
- GPUI's tooltips and overlays can't draw over the web view, so they hide it or show beside it.
- A render can't show the web view; `NOTCHLING_RENDER_PAGE` draws the panel's chrome.

## Status

Proposed. It amends the Web content point of 0005 for Chat's page panel only. The maintainer asked for the feature on 2026-11-10; accepting this record settles it.

## Date

2026-11-10
