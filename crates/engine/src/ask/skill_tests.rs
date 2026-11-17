use super::skill::{text, DESCRIPTION, NAME};
use crate::delegate::skill::SkillTool;

const TOOLS: [SkillTool; 3] = [SkillTool::Claude, SkillTool::Codex, SkillTool::Antigravity];

#[test]
fn claude_text_says_background_end_turn_and_handback() {
    let text = text("/Apps/N", SkillTool::Claude);
    for part in [
        "run_in_background: true",
        "end your turn",
        "`Answer:`",
        "SubagentHandback",
        "plain text at the end of a turn doesn't reach your caller",
        "Don't call SubagentHandback to wait",
        "deliver your report with SubagentHandback, even if you called it before",
        "`claude -p`",
        "'/Apps/N' ask [--choice",
    ] {
        assert!(text.contains(part), "{part}");
    }
    assert!(text.starts_with("---\nname: notchling-ask\ndescription: Use when you need a decision"));
}

#[test]
fn codex_and_agy_text_say_wait() {
    let codex = text("/Apps/N", SkillTool::Codex);
    assert!(codex.contains("ask --wait"));
    assert!(codex.contains("124"));
    assert!(codex.contains("`--write` delegate jobs"));
    assert!(codex.contains("read-only sandbox blocks Notchling's socket"));
    assert!(codex.contains("report the question to your caller"));
    assert!(!codex.contains("run_in_background"));
    let agy = text("/Apps/N", SkillTool::Antigravity);
    assert!(agy.contains("ask --wait"));
    assert!(agy.contains("124"));
    assert!(agy.contains("run_command"));
    assert!(!agy.contains("run_in_background"));
    assert!(!agy.contains("SubagentHandback"));
}

#[test]
fn text_names_exit_codes_and_choices_rule() {
    for tool in TOOLS {
        let text = text("/Apps/N", tool);
        for part in [
            "- 0:",
            "- 2:",
            "- 3:",
            "- 124:",
            "2 to 6 choices",
            "Never put a secret",
            "One question per command",
            "stands on its own",
            "`--main-may-answer` only when the main session knows enough",
            "Ask your caller the way you would without Notchling",
        ] {
            assert!(text.contains(part), "{tool:?} {part}");
        }
    }
}

#[test]
fn main_session_part() {
    for tool in TOOLS {
        let text = text("/Apps/N", tool);
        assert!(text.contains("'/Apps/N' asks --wait"));
        assert!(text.contains("'/Apps/N' answer <id> --choice <n>"));
        assert!(text.contains("answer <id>"));
        assert!(text.contains("in the background"));
    }
}

#[test]
fn no_model_names_and_no_placeholder_left() {
    for tool in TOOLS {
        let text = text("/Apps/N", tool).to_lowercase();
        for name in ["opus", "sonnet", "haiku", "fable", "gpt", "gemini"] {
            assert!(!text.contains(name), "{tool:?} {name}");
        }
        for left in ["{binary}", "{how}", "{description}"] {
            assert!(!text.contains(left), "{left}");
        }
    }
}

#[test]
fn the_description_is_fixed() {
    assert_eq!(NAME, "notchling-ask");
    assert_eq!(
        DESCRIPTION,
        "Use when you need a decision from the user that you can't make or find out yourself — ask it on its own card in Notchling and get the answer back, instead of stopping or reporting the question to your caller."
    );
}
