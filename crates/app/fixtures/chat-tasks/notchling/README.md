# Chat tasks fixture

A Chat fixture root (`crates/app/fixtures/chat-tasks`), laid out like `fixtures/chat`: one working
session whose transcript writes a three-item task list (one done, one under way, one waiting)
between a few messages, so the pinned Tasks bar shows. Render it with
`notchling-app --render chat --out <file.png> --data-root crates/app/fixtures/chat-tasks`;
`NOTCHLING_RENDER_TASKS_OPEN=1` draws the bar open, its list under it, and
`NOTCHLING_RENDER_APPEARANCE=light` draws it light. This folder stands for Notchling's data
folder and holds only this note.
