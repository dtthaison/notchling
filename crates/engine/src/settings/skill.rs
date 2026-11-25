use crate::delegate::skill::SkillTool;

pub const NAME: &str = "notchling-settings";

pub const TOOLS: [SkillTool; 2] = [SkillTool::Claude, SkillTool::Codex];

pub const WAIT_SECS: u64 = 900;

const TEMPLATE: &str = r#"---
name: notchling-settings
description: Use when the user asks to check or change a Notchling setting — the theme or appearance, how much Chat history to load, a shortcut, the notch (width, pet, last reply), keeping the Mac awake, limit watching — or asks what Notchling's settings are. Reads and changes them through Notchling, and the app shows a change at once.
---

# Notchling settings

Notchling (the Mac menu bar app that shows agent sessions in the notch) keeps its settings in its own store. Read and change them with its command line, never by editing its files.

## 1. Find the key

```sh
'{binary}' settings list
```

Lists every setting: its key, title, value, what it takes, and who may change it:

- `free`: you may set it.
- `ask`: you may ask for it; the user allows or denies the change in Notchling's Inbox, on the Mac or a paired phone.
- `never`: only the user changes it, in Notchling. That covers the daemon's switches (`'{binary}' switches` lists them), logins, and anything that would give an agent more power. Don't look for a way around it: tell the user where to change it.

`'{binary}' settings get <key>` shows one setting. Add `--json` to either for machine-readable output.

## 2. Change it

```sh
'{binary}' settings set <key> <value>
```

Give a value the list shows: `true` or `false`, a number, or one of the choices. Exit codes:

- 0: set. The app shows it at once.
- 2: an unknown key or a value it doesn't take, or the command was wrong. The message says which, and what the key takes.
- 3: Notchling isn't running.
- 4: refused: a key agents can't change.
- 5: the user denied the change.
- 6: the request still waits for the user (with `--json`, the reply holds the request).
- 124: the user didn't answer in time.

For an `ask` key it says it waits for the user's approval in Notchling's Inbox and waits up to {wait}, longer than a foreground command may run, so run it in the background (Claude Code's Bash: `run_in_background: true`) and tell the user to allow it there. If the command stops waiting first, the change still applies once the user allows it; check with `settings get`.

Change only what the user asked for, and never a setting on your own initiative.
{sandbox_note}"#;

fn sandbox_note(tool: SkillTool) -> &'static str {
    match tool {
        SkillTool::Claude => "",
        SkillTool::Codex => "\n## In Codex's sandbox\n\nNotchling's command talks to the Notchling daemon over a local socket, which Codex's workspace-write sandbox blocks (\"isn't running\" means the sandbox blocked it). Run `settings` with escalated permissions (outside the sandbox) and give the user a one-line reason; if this session can't escalate, tell the user it needs to run outside the sandbox.\n",
        SkillTool::Antigravity => "\n## In Antigravity\n\nRun these through the terminal tool. If the session runs with `--sandbox` and the call says \"isn't running\", the sandbox blocked the local socket; ask the user to allow the command.\n",
    }
}

pub fn content(binary: &str, tool: SkillTool) -> String {
    TEMPLATE
        .replace("{binary}", binary)
        .replace("{sandbox_note}", sandbox_note(tool))
        .replace("{wait}", &wait_text())
}

fn wait_text() -> String {
    let minutes = WAIT_SECS / 60;
    format!("{minutes} minute{}", if minutes == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skill_names_the_binary_the_commands_and_the_rules() {
        let text = content("/Apps/N", SkillTool::Claude);
        assert!(text.starts_with("---\nname: notchling-settings\n"));
        for line in [
            "'/Apps/N' settings list",
            "'/Apps/N' settings get <key>",
            "'/Apps/N' settings set <key> <value>",
            "Inbox",
            "'/Apps/N' switches",
        ] {
            assert!(text.contains(line), "{line}");
        }
        assert!(!text.contains("{binary}") && !text.contains("{sandbox_note}"));
        assert!(!text.contains("{wait}"));
        assert!(!text.contains("Codex's sandbox"));
        assert!(content("/Apps/N", SkillTool::Codex).contains("escalated permissions"));
        assert_eq!(TOOLS, [SkillTool::Claude, SkillTool::Codex]);
    }

    #[test]
    fn settings_skill_names_no_switch() {
        use super::super::agents::SWITCHES;
        for tool in [SkillTool::Claude, SkillTool::Codex, SkillTool::Antigravity] {
            let text = content("/Apps/N", tool);
            for switch in SWITCHES {
                assert!(!text.contains(switch.title), "{}", switch.title);
            }
            assert!(!text.contains("five switches"));
        }
    }

    #[test]
    fn the_wait_is_the_one_the_command_waits() {
        let text = content("/Apps/N", SkillTool::Claude);
        assert!(text.contains("waits up to 15 minutes"));
        assert!(!text.contains("five minutes"));
        assert_eq!(WAIT_SECS, 900);
    }
}
