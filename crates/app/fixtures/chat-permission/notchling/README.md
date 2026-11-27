# Chat permission fixture

A Chat fixture root (`crates/app/fixtures/chat-permission`), laid out like `fixtures/chat`: one
background job blocked on a Bash permission that no hook holds, known only from its state
(`needs: "approve Bash: …"`), so Chat shows the Permission card read from it. Render it with
`notchling-app --render chat --out <file.png> --data-root crates/app/fixtures/chat-permission`.
A render has no daemon, so the card's buttons are drawn off with the reason under them; with the
daemon attached and acting on they answer in the job's terminal (`session.permission` with
`expect`). This folder stands for Notchling's data folder and holds only this note.
