---
kind: feature
---
The Details Panel's Git section now shows every worktree of the session's repository, one line each: its branch, how big its work is, and a dot where a session or subagent is working. A last row, "main and 39 branches", opens a new read-only Branches panel with Working, Not Merged and Merged tabs, a filter, and Changes, Open Session and Show in Finder on each row. Every number is against the branch's nearest base, so a lane is shown against the branch it was cut from, not against the whole history of `main`, and Changes opens against that base too. Nothing fetches: remote branches are as fresh as the last fetch, and the panel says when that was.
