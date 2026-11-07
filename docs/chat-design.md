# Chat window: design notes

Made with BAMA's `mac-design` and `ux-design` passes (github.com/daveebbelaar/bama at f0c2cdb, read, not installed).

## Design plan

- **Archetype**: menu bar extra + panels. The notch stays the quick entry point; Chat is the window people keep open beside the work.
- **Information architecture**: one level. Sidebar of sessions grouped Needs Input / Working / Completed; the detail is the conversation.
- **Command map**: menu bar menu > Chat… opens it on the session that most needs you. Return sends, ⌥Return adds a line, Tab takes the suggested reply or the highlighted slash command, ↑↓ move through commands.
- **Feeling**: calm. Nothing moves unless an agent does.

## Glossary

| Term | Means | Not |
| --- | --- | --- |
| Session | One Claude Code conversation, in a terminal or in the background | agent, job, chat |
| Background session | Runs with no terminal (`claude --bg`); can be replied to here | job |
| Terminal session | Lives in a terminal window; read here, reply there | |
| Reply | Text sent into a background session | message, prompt |
| Question | A choice the session asked with AskUserQuestion | |
| Permission | A tool call waiting for Allow or Deny | |

## UX

- Flow: menu bar > Chat… -> the session that needs input is selected -> type a reply -> Return sends, the reply shows as "Sending…" -> it becomes a normal message when the session has it. Escape leaves the field; the draft stays per session.
- Surface: new window ("a long or self-contained task people keep open beside the main window"). The Accounts window is fixed size; Chat resizes like Artifacts.
- States:
  - no sessions: "No Sessions" with how to start one
  - no selection: "Select a Session"
  - empty transcript: "No messages yet"
  - working: the activity line ("Editing main.swift") under the last message
  - sending: the reply in the list with "Sending…"
  - failed: the reason under the field, and the draft comes back
  - terminal session: "Replies to this session go to its terminal." with Open in <App>
  - ended: "This session has ended." with Resume in Terminal
- Words: "Chat…", "Reply…", "Open in Ghostty", "Resume in Terminal", "Allow", "Always Allow", "Deny".
- Access: keyboard-only flow works (sidebar arrows, Tab into the field, Return). Status is an icon plus a word, never color alone. Every icon-only button has `.help`.

## Security

Replies use the same local path as the notch: `claude attach <id>` in a pseudo-terminal. No network listener, no web view, nothing another app or web page can reach. Terminal sessions are read-only, so Chat can never type into a terminal.
