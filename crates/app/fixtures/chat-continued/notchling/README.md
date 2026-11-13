# Chat continued-in fixture

A Chat fixture root (`crates/app/fixtures/chat-continued`), laid out like `fixtures/chat`: one
background job whose transcript holds only a title and a name, because Claude Code moved the
interactive session into it. The earlier transcript in the same project folder holds the whole
conversation and ends with a `continued-in` record naming the job's session. Chat, the notch and
the engine follow that record back, so the job shows the earlier messages. Render it with
`notchling-app --render chat --out <file.png> --data-root crates/app/fixtures/chat-continued`
(`NOTCHLING_RENDER_FORCE_FRAMES=1` on a locked screen). This folder stands for Notchling's data
folder and holds only this note.
