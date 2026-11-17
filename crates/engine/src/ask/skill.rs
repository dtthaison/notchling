use crate::delegate::skill::SkillTool;

pub const NAME: &str = "notchling-ask";

pub const DESCRIPTION: &str = "Use when you need a decision from the user that you can't make or find out yourself — ask it on its own card in Notchling and get the answer back, instead of stopping or reporting the question to your caller.";

pub const TEMPLATE: &str = r#"---
name: notchling-ask
description: {description}
---

# Ask the user

Ask only for a decision you can't make or find out yourself. Notchling saves the question and shows it on its own card, naming you, in Chat, the notch and the Inbox (so also on the phone). The command prints the answer when the user gives it.

```sh
'{binary}' ask [--choice '<option>']... [--main-may-answer] [--wait] [--timeout <seconds>] [--json] -- '<question>'
```

- One question per command.
- Give 2 to 6 choices (`--choice`, repeated) when the answer is one of a few. The phone answers only choices.
- Write a question that stands on its own: name the file, the branch and the trade-off. The user reads it without your conversation.
- Never put a secret in a question: not a token, a password or a key.
- `--main-may-answer` only when the main session knows enough to answer. Without it only the user answers.
{how}
## Exit codes

- 0: answered. `Answer:` is the answer, `From:` says who gave it.
- 2: dismissed, or already answered. Decide yourself, or report the question to your caller.
- 3: Notchling refused or isn't running. Ask your caller the way you would without Notchling.
- 124: no answer in time. Decide yourself, or report the question to your caller.
- 1: anything else; the reason is on stderr.

## If you are the main session

Questions from your own subagents that allow it (`--main-may-answer`) can be answered by you:

```sh
'{binary}' asks --wait
'{binary}' answer <id> --choice <n>
'{binary}' answer <id> -- '<text>'
```

Run `asks --wait` in the background; it prints each open question with its id and numbered choices, and ends when one is waiting. Answer only what you know: leave any other question to the user. `answer` says `Already answered` when the user was first.
"#;

impl SkillTool {
    fn ask_how(self) -> &'static str {
        match self {
            SkillTool::Claude => "\n## How to run it\n\nRun it with Bash `run_in_background: true`, then end your turn with one short line. Don't call SubagentHandback to wait: it ends your run. The command's exit wakes you; read its output: `Answer:` is the answer. A main session, or `claude -p`, is woken by the exit the same way.\n\nWhen your work is done, deliver your report with SubagentHandback, even if you called it before: plain text at the end of a turn doesn't reach your caller.\n",
            SkillTool::Codex => "\n## How to run it\n\nRun `ask --wait` in the foreground with the longest timeout the tool allows. Under `notchling delegate` it stops by itself before the delegate's limit (exit 124).\n\nIt works only in `--write` delegate jobs: the read-only sandbox blocks Notchling's socket, and the command exits 3. Then report the question to your caller.\n",
            SkillTool::Antigravity => "\n## How to run it\n\nRun `ask --wait` with the run_command tool and let the tool wait for it; under `notchling delegate` it stops by itself before the delegate's limit (exit 124). Make the command exactly `'<binary>' ask --wait ...`, with no `;`, `&&`, pipe or redirection around it, or the request is declined.\n",
        }
    }
}

pub fn text(binary: &str, tool: SkillTool) -> String {
    TEMPLATE
        .replace("{description}", DESCRIPTION)
        .replace("{how}", tool.ask_how())
        .replace("{binary}", binary)
}
