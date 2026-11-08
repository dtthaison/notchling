//! The text of the `notchling-delegate` skill (Swift `DelegateSkill`): how an agent hands a job (an
//! image, a video, a second-opinion review) to another tool with `notchling delegate`.
//!
//! This is only the text. Installing it into Claude Code's, Codex's and Antigravity's skills
//! folders belongs to E13 (milestone 6), and nothing here writes a file.

/// The skill's name and its folder name.
pub const NAME: &str = "notchling-delegate";

/// Whose skills folder the text is for; each gets its own notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillTool {
    Claude,
    Codex,
    Antigravity,
}

/// The skill, with `{binary}` (the path of the `notchling` program), `{wait_note}`, `{codex_model}`
/// (a model Codex offers, for the example) and `{tool_note}` to fill in.
pub const TEMPLATE: &str = r#"---
name: notchling-delegate
description: Use to hand a job to another AI tool on this Mac and get its result back — generate an image (Codex or Antigravity), get a second-opinion code review from a different model (Antigravity CLI with Gemini, or Codex with GPT), or any task the user wants another agent to do — with a chosen model and effort. Runs through Notchling, waits for the other tool to finish, prints its reply and the files it made.
---

# Delegate to another agent

```sh
'{binary}' delegate <codex|agy> [--model <m>] [--effort <e>] [--folder <dir>] [--out <dir>] [--write] [--session <id>] [--timeout <s>] [--json] -- '<prompt>'
```

- `codex`: OpenAI Codex (GPT models). Its image tool makes images.
- `agy`: Antigravity CLI (Gemini, also Claude and GPT-OSS models). Its image subagent makes images.
- `--model`, `--effort`: pick both. `'{binary}' delegate-models` lists each tool's models and efforts. For agy, effort is low/medium/high (`xhigh`/`max` become high), and `gemini-4-argon-high` is read as model plus effort.
- `--folder`: the workspace the other agent works in (default: the current folder). Name it for a review.
- `--write`: let it change files in that folder. Without it, it only reads (Codex read-only sandbox; agy's permission requests are declined and listed). Use `--write` only when the job is to change files.
- `--out <dir>`: copy the images, videos or audio it generated there. Without it, they are listed where the tool saved them.
- `--session <id>`: continue that session (the id is in the output), for a follow-up on the same job.
- Exit code 0 when it finished, 1 when it failed, 124 on timeout (default 1800 s). Nested delegation stops at two levels.

It blocks until the other tool's turn ends; nothing polls. Run it in the background and carry on: {wait_note}

Output: a header line (tool, model, effort, time, tokens), `Session:`, `Generated:` and `Copied to:` file paths, any `Declined` permissions, then the other agent's reply. Read files it names before relying on them, and tell the user which tool and model did the work.

## Examples

```sh
'{binary}' delegate codex --out ./assets -- 'Generate a 1024x1024 app icon: a paper plane, flat, blue on white.'
'{binary}' delegate agy --model gemini-4-argon --effort high --folder . -- 'Review the uncommitted changes (git diff HEAD) for bugs. List each with file:line and why. Do not change files.'
'{binary}' delegate codex --model {codex_model} --effort high --write --folder ./site -- 'Add a dark mode to styles.css.'
```

Write the prompt so the other agent needs nothing from this conversation: the goal, the files, what to return. It sees the folder, not your context.
{tool_note}"#;

impl SkillTool {
    /// How to wait for the command in this tool.
    pub fn wait_note(self) -> &'static str {
        match self {
            SkillTool::Claude => "with Bash `run_in_background: true` you are told when it exits, then read its output. In the foreground give it the longest timeout (600000 ms); an image takes about a minute, a review several.",
            SkillTool::Codex => "give it a long timeout; an image takes about a minute, a review several.",
            SkillTool::Antigravity => "give the terminal tool a long timeout; an image takes about a minute, a review several.",
        }
    }

    /// The closing section for this tool (empty for Claude Code).
    pub fn tool_note(self) -> &'static str {
        match self {
            SkillTool::Claude => "",
            SkillTool::Codex => "\n## In Codex's sandbox\n\nThe other tool needs the network and its own folders, so run `delegate` with escalated permissions (outside the sandbox) and give the user a one-line reason.\n",
            SkillTool::Antigravity => "\n## In Antigravity\n\nRun it through the terminal tool; with `--sandbox` ask the user to allow the command, since the other tool needs the network.\n",
        }
    }
}

/// The skill for `tool`. `codex_model` is a model slug Codex offers, for the third example (any
/// placeholder when Codex has not cached its list).
pub fn content(binary: &str, tool: SkillTool, codex_model: Option<&str>) -> String {
    TEMPLATE
        .replace("{wait_note}", tool.wait_note())
        .replace("{tool_note}", tool.tool_note())
        .replace("{codex_model}", codex_model.unwrap_or("<model>"))
        .replace("{binary}", binary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skill_names_the_binary_and_the_tools_notes() {
        let claude = content("/Apps/N", SkillTool::Claude, None);
        assert!(claude.starts_with("---\nname: notchling-delegate\n"));
        assert!(claude.contains("'/Apps/N' delegate <codex|agy>"));
        assert!(claude.contains("'/Apps/N' delegate-models"));
        assert!(claude.contains("run_in_background"));
        assert!(claude.contains("--model <model> --effort high --write"));
        assert!(!claude.contains("escalated permissions"));
        assert!(claude.ends_with("not your context.\n"));
        assert!(
            content("/Apps/N", SkillTool::Codex, Some("gpt-x")).contains("escalated permissions")
        );
        assert!(content("/Apps/N", SkillTool::Codex, Some("gpt-x"))
            .contains("--model gpt-x --effort high --write"));
        assert!(content("/Apps/N", SkillTool::Antigravity, None).contains("## In Antigravity"));
    }

    #[test]
    fn no_placeholder_is_left_and_the_text_is_the_one_the_commands_accept() {
        for tool in [SkillTool::Claude, SkillTool::Codex, SkillTool::Antigravity] {
            let text = content("/b", tool, None);
            for left in ["{binary}", "{wait_note}", "{tool_note}", "{codex_model}"] {
                assert!(!text.contains(left), "{left}");
            }
        }
        // The flags it lists are the ones the command parses.
        for flag in [
            "--model",
            "--effort",
            "--folder",
            "--out",
            "--write",
            "--session",
            "--timeout",
            "--json",
        ] {
            assert!(TEMPLATE.contains(flag), "{flag}");
        }
        assert_eq!(NAME, "notchling-delegate");
    }

    #[test]
    fn it_matches_the_swift_text_apart_from_the_command_name() {
        // `Notchling --delegate` became `notchling delegate`; the rest of the prose is the same.
        assert!(TEMPLATE.contains("Exit code 0 when it finished, 1 when it failed, 124 on timeout (default 1800 s). Nested delegation stops at two levels."));
        assert!(TEMPLATE.contains("It sees the folder, not your context."));
    }
}
