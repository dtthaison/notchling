# Chat Code Mode fixture

A Chat fixture root (`crates/app/fixtures/chat-code-mode`), laid out like `fixtures/chat-steps`: one working session from Claude Code's Code Mode. Its transcript has a `REPL` script that finished with two inner calls nested under it, a script that failed, and a script still running. Every path and name in it is made up. Render it with `notchling-app --render chat --out <file.png> --data-root crates/app/fixtures/chat-code-mode`; `NOTCHLING_RENDER_EXPAND=1` draws every step group and step open. This folder stands for Notchling's data folder and holds only this note.
