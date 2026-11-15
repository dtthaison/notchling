---
kind: fix
---
Refresh in Accounts now fixes "Couldn't read limits. Its login has expired" on a Claude Code account in a lane or the main login. When no Claude Code session is running there, Refresh asks Claude Code itself to renew the login (`claude auth status`, no prompt and no model use), shows "Renewing login…" on the card, then reads the limits again. If Claude Code can't renew it, the card says to sign in again from the ⋯ menu. A cost of zero no longer reads "$-0.00".
