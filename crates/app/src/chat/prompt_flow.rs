use notchling_kit::sentence;
use notchling_locale as t;

use super::markers::{CLOSED_CONNECTION, DIDNT_ANSWER, FILE_TOOLS, GONE};

use crate::acting::Decision;

pub const CONFIRM_MS: i64 = 6_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sending {
    Answer,
    Decision { decision: Decision, on_screen: bool },
}

pub fn sending_text(sending: Sending) -> &'static str {
    match sending {
        Sending::Answer => t::chat::sending_note(),
        Sending::Decision {
            decision,
            on_screen: false,
        } => match decision {
            Decision::Allow => t::chat::allowing(),
            Decision::Always => t::chat::allowing_always(),
            Decision::Deny => t::chat::denying(),
        },
        Sending::Decision {
            decision,
            on_screen: true,
        } => match decision {
            Decision::Allow => t::chat::allowing_typing(),
            Decision::Always => t::chat::allowing_always_typing(),
            Decision::Deny => t::chat::denying_typing(),
        },
    }
}

const SUBJECT_CHARS: usize = 48;

pub fn file_name<'a>(tool: &str, summary: &'a str) -> Option<&'a str> {
    if !FILE_TOOLS.contains(&tool.trim()) {
        return None;
    }
    let path = summary.trim();
    if path.contains('\n') || path.contains('…') || path.ends_with("...") {
        return None;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    (!name.is_empty()).then_some(name)
}

fn subject(tool: &str, summary: &str) -> String {
    if let Some(name) = file_name(tool, summary) {
        return name.to_owned();
    }
    let line = summary.lines().next().unwrap_or("").trim();
    if line.chars().count() <= SUBJECT_CHARS && !summary.trim().contains('\n') {
        return line.to_owned();
    }
    let cut: String = line.chars().take(SUBJECT_CHARS).collect();
    format!("{}…", cut.trim_end())
}

pub fn confirmation(tool: &str, summary: &str, decision: Decision) -> String {
    let verb = match decision {
        Decision::Allow => t::chat::allowed(),
        Decision::Always => t::chat::allowed_always(),
        Decision::Deny => t::chat::denied(),
    };
    let subject = subject(tool, summary);
    if subject.is_empty() {
        t::chat::decision_tool(verb, tool.trim())
    } else {
        t::chat::decision_tool_subject(verb, tool.trim(), &subject)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Told {
    pub went_through: String,
    pub already_answered: String,
}

impl Told {
    pub fn new(tool: &str, summary: &str, decision: Decision) -> Told {
        Told {
            went_through: confirmation(tool, summary, decision),
            already_answered: already_answered(tool, summary),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirmed {
    pub text: String,
    pub until_ms: i64,
}

impl Confirmed {
    pub fn new(text: String, now_ms: i64) -> Confirmed {
        Confirmed {
            text,
            until_ms: now_ms + CONFIRM_MS,
        }
    }

    pub fn visible(&self, now_ms: i64) -> bool {
        now_ms < self.until_ms
    }
}

pub fn is_gone(message: &str) -> bool {
    message.contains(GONE)
}

pub fn already_answered(tool: &str, summary: &str) -> String {
    let subject = subject(tool, summary);
    if subject.is_empty() {
        t::chat::already_answered(tool.trim())
    } else {
        t::chat::already_answered_subject(tool.trim(), &subject)
    }
}

pub fn failure_text(message: &str) -> String {
    let said = sentence(message.trim());
    if said.is_empty() {
        return said;
    }
    if message.contains(DIDNT_ANSWER) || message.contains(CLOSED_CONNECTION) {
        return t::chat::may_have_reached(said.trim_end_matches('.'));
    }
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: &str = "/Users/me/work/notchling/.claude/projects/-Users-me-work-notchling/memory/autopilot-needs-claude-on-path.md";

    #[test]
    fn a_decision_on_its_way_says_what_is_being_decided() {
        let on_screen = |decision| Sending::Decision {
            decision,
            on_screen: true,
        };
        assert!(sending_text(on_screen(Decision::Allow)).starts_with("Allowing"));
        assert!(sending_text(on_screen(Decision::Always)).starts_with("Allowing"));
        assert!(sending_text(on_screen(Decision::Always)).contains("not asking again"));
        assert!(sending_text(on_screen(Decision::Deny)).starts_with("Denying"));
        assert!(sending_text(on_screen(Decision::Allow)).contains("few seconds"));
        let held = Sending::Decision {
            decision: Decision::Allow,
            on_screen: false,
        };
        assert_eq!(sending_text(held), "Allowing…");
        assert_eq!(sending_text(Sending::Answer), "Sending…");
    }

    #[test]
    fn a_file_tool_headlines_its_file_not_its_whole_path() {
        assert_eq!(
            file_name("Write", LONG),
            Some("autopilot-needs-claude-on-path.md")
        );
        assert_eq!(file_name("Edit", "/w/a b/notes v2.md"), Some("notes v2.md"));
        assert_eq!(file_name("MultiEdit", "/w/src/main.rs"), Some("main.rs"));
        assert_eq!(file_name("NotebookEdit", "/w/x.ipynb"), Some("x.ipynb"));
        assert_eq!(file_name("Read", "~/notes/today.md"), Some("today.md"));
        assert_eq!(file_name("Write", "notes.md"), Some("notes.md"));
        assert_eq!(file_name("Bash", "ls /tmp/a.txt"), None);
        assert_eq!(file_name("Write", "/Users/me/…/memory/a.md"), None);
        assert_eq!(file_name("Write", "/Users/me/memory/…"), None);
        assert_eq!(file_name("Write", "/Users/me/memory/"), None);
        assert_eq!(file_name("Write", ""), None);
        assert_eq!(file_name("Write", "line one\nline two"), None);
    }

    #[test]
    fn two_writes_in_one_folder_are_told_apart_by_their_names() {
        let a = file_name("Write", "/w/memory/autopilot-needs-claude-on-path.md");
        let b = file_name("Write", "/w/memory/glass-kit-feature-state.md");
        assert_eq!(a, Some("autopilot-needs-claude-on-path.md"));
        assert_eq!(b, Some("glass-kit-feature-state.md"));
    }

    #[test]
    fn what_went_through_is_named_by_decision_and_subject() {
        assert_eq!(
            confirmation("Write", LONG, Decision::Allow),
            "Allowed: Write autopilot-needs-claude-on-path.md"
        );
        assert_eq!(
            confirmation("Edit", "/w/MEMORY.md", Decision::Always),
            "Always allowed: Edit MEMORY.md"
        );
        assert_eq!(
            confirmation("Bash", "cargo test --workspace", Decision::Deny),
            "Denied: Bash cargo test --workspace"
        );
        let long = format!("echo {}é{}\nsecond line", "a".repeat(40), "b".repeat(40));
        let text = confirmation("Bash", &long, Decision::Allow);
        assert!(text.starts_with("Allowed: Bash echo aaaa"), "{text}");
        assert!(text.ends_with('…') && !text.contains('\n'), "{text}");
        assert!(text.chars().count() <= 70, "{text}");
        assert_eq!(confirmation("Bash", "", Decision::Allow), "Allowed: Bash");
    }

    #[test]
    fn a_confirmation_shows_for_a_moment_only() {
        let c = Confirmed::new("Allowed: Write a.md".into(), 1_000);
        assert!(c.visible(1_000));
        assert!(c.visible(1_000 + CONFIRM_MS - 1));
        assert!(!c.visible(1_000 + CONFIRM_MS));
    }

    #[test]
    fn a_refusal_is_shown_in_the_daemons_words() {
        let said = "The terminal asks about something else than Chat shows, so nothing was typed. Answer it in the terminal.";
        assert_eq!(failure_text(said), said);
        let late = failure_text("the daemon didn't answer in time");
        assert!(
            late.starts_with("The daemon didn't answer in time."),
            "{late}"
        );
        assert!(late.contains("may still have"), "{late}");
        assert!(failure_text("the daemon closed the connection").contains("may still have"));
        assert_eq!(failure_text(""), "");
    }

    #[test]
    fn the_renders_name_each_state_of_the_card() {
        use crate::chat::prompts::{preview_prompt, Preview, Prompt, NEEDS_PREFIX, PREVIEW_PATH};
        for (word, preview) in [
            ("permission-file", Preview::PermissionFile),
            ("permission-sending", Preview::PermissionSending),
            ("permission-failed", Preview::PermissionFailed),
            ("permission-next", Preview::PermissionNext),
        ] {
            assert_eq!(Preview::parse(word), Some(preview), "{word}");
            let Prompt::Permission(r) = preview_prompt(preview) else {
                panic!("a permission")
            };
            assert!(r.id.starts_with(NEEDS_PREFIX), "{word}");
            assert_eq!(
                file_name(&r.tool_name, &r.summary),
                PREVIEW_PATH.rsplit('/').next()
            );
        }
    }
}
