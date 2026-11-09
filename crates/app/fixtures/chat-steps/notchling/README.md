# Chat steps fixture

A second Chat fixture root (`crates/app/fixtures/chat-steps`), laid out like `fixtures/chat`:
one working session whose transcript has a group of four commands (one failed) and a Read, a
thought, a Markdown reply (heading, bold, italics, inline code, lists, a code block, a table, a
link), a turn end, then an edit and a command still running. Render it with
`notchling-app --render chat --out <file.png> --data-root crates/app/fixtures/chat-steps`;
`NOTCHLING_RENDER_EXPAND=1` draws every step group, step and thought open. This folder stands for
Notchling's data folder and holds only this note.
