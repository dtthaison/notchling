# Chat Claude in Chrome fixture

A Chat fixture root (`crates/app/fixtures/chat-chrome`), laid out like `fixtures/chat-steps`: one
session whose transcript is made up: the `mcp__claude-in-chrome__*` calls Claude Code makes through
the Claude in Chrome extension (tabs, navigate, screenshots, clicks, a batch, the console), with
placeholder pages (example.com, a drawn PNG) and two failures (the extension not connected, a site
waiting for permission). Render it with
`notchling-app --render chat --out <file.png> --data-root crates/app/fixtures/chat-chrome`;
`NOTCHLING_RENDER_EXPAND=1` draws every step group and step open. This folder stands for
Notchling's data folder and holds only this note.
