---
name: render-notch-view
description: Use when you need to see a Notchling view or menu to check a UI change — renders NotchView offscreen to a PNG, or reads NSMenu item frames.
---

To see a view, render it offscreen: write a probe `main.swift` in its own folder under `/tmp`, compile it with every file in `Sources/` except `main.swift` and `AppDelegate.swift`, put `NotchView(model:)` in a borderless window at `NotchController.canvas` size, drive the model (`go(.list)`, `peek(_:)`, `go(.detail(id))`, `requests`, `width`), wait for animations to settle (2–3 s; longer if transcripts are big), then `cacheDisplay` to a PNG and read it. `AgentMonitor().poll()` gives the real agents on this Mac.

NSMenu contents can't be captured as an image (glass is blank), but walking the popped-up menu window's view tree gives exact frames.
