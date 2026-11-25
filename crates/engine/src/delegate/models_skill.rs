use super::skill::SkillTool;

pub const NAME: &str = "notchling-models";

pub const TOOLS: [SkillTool; 3] = [SkillTool::Claude, SkillTool::Codex, SkillTool::Antigravity];

const TEMPLATE: &str = r#"---
name: notchling-models
description: Use when a tool updated its models, or to check models, efforts and prices for Claude Code, Codex and Antigravity. Reads each tool's current model list through Notchling, updates Notchling's copy, and reports what is new or gone, which efforts each model offers, which models have no price, and which role picks to review.
---

# Notchling models

Notchling keeps a copy of each tool's models and efforts (Chat's model picker, New Session, `delegate-models` and the delegate effort checks read it). One command refreshes it for Claude Code, Codex and Antigravity.

```sh
'{binary}' models            # check: read the lists, change nothing of Notchling's, report
'{binary}' models update     # read the lists, update Notchling's copy, report (the app rewrites the skills)
'{binary}' models --json     # the same report as JSON (also: models update --json)
```

Where each list comes from. Notchling never writes a login or token itself:

- Claude Code: Anthropic's model list, asked with the live Claude Code login's access token (never renewed) and only while Notchling's accounts switch is on. It is asked at most once a minute and not again after Anthropic refused the login. When it can't be read the report says "not read" and the list Notchling has stays. Efforts are the data file's: Claude Code offers no listing of them.
- Codex: `codex debug models`, Codex's own catalog. Codex may renew its own login when it asks for models, so Notchling runs it only while no Codex session is live; otherwise the report says so and the cached list stays. Efforts are per model.
- Antigravity: `agy models`. Efforts are per model, so `xhigh` is offered only where the listing has it (for example `gemini-4-neon-xhigh`) and refused elsewhere.

A list that is empty, or that drops most of the models Notchling had, is refused and the old list stays; add `--force` to accept a real shrink.

The report lists, per tool, the models that are new or gone, the efforts a model gained or lost, and the models with no price. Then it lists role picks to review: a marked model that is gone, or has a newer one in its family, or no longer offers the marked effort.

## Rules

- Report what the command printed. Don't change role picks, the override file or any price yourself: role picks are the maintainer's decision, so present the suggestions and stop. `'{binary}' delegate-models --for <role>` shows what each role would use now.
- Never make up a price. A model without a price is counted at $0 until a price is added to the override file, and the report says how.
- The Notchling app rewrites the skills (Settings > Skills & Models > Refresh Models). From the command line `update` leaves them alone and the report says "skills not rewritten (needs the app)"; the app also skips skills it didn't write and does nothing while the Install Hooks and Skills switch is off.
- Exit code 0 when the report was made, 1 when the daemon failed, 4 when it refused, 3 when Notchling isn't running, 2 when the command was wrong.
{tool_note}"#;

fn tool_note(tool: SkillTool) -> &'static str {
    match tool {
        SkillTool::Claude => "",
        SkillTool::Codex => "\n## In Codex's sandbox\n\nThe command talks to the Notchling daemon over a local socket and runs the other tools, which Codex's workspace-write sandbox blocks (\"isn't running\" means the sandbox blocked it). Run it with escalated permissions (outside the sandbox) and give the user a one-line reason.\n",
        SkillTool::Antigravity => "\n## In Antigravity\n\nRun it through the terminal tool. If the session runs with `--sandbox` and the call says \"isn't running\", the sandbox blocked the local socket; ask the user to allow the command.\n",
    }
}

pub fn content(binary: &str, tool: SkillTool) -> String {
    TEMPLATE
        .replace("{tool_note}", tool_note(tool))
        .replace("{binary}", binary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skill_names_the_binary_the_commands_and_the_rules() {
        let text = content("/Apps/N", SkillTool::Claude);
        assert!(text.starts_with("---\nname: notchling-models\n"));
        assert!(text.contains("'/Apps/N' models update"));
        assert!(text.contains("'/Apps/N' models --json"));
        assert!(text.contains("'/Apps/N' delegate-models --for <role>"));
        assert!(text.contains("Use when a tool updated its models"));
        assert!(text.contains("Don't change role picks"));
        assert!(text.contains("Never make up a price"));
        assert!(!text.contains("escalated permissions"));
        assert!(content("/Apps/N", SkillTool::Codex).contains("escalated permissions"));
        assert!(content("/Apps/N", SkillTool::Antigravity).contains("## In Antigravity"));
        assert_eq!(NAME, "notchling-models");
    }

    #[test]
    fn no_placeholder_is_left_and_no_model_is_named_as_a_pick() {
        for tool in TOOLS {
            let text = content("/b", tool);
            for left in ["{binary}", "{tool_note}"] {
                assert!(!text.contains(left), "{left}");
            }
            for name in ["gpt-6", "astra", "hydrogen-high"] {
                assert!(!text.contains(name), "{name}");
            }
        }
    }
}
