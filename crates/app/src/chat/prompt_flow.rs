//! What a card says while an answer is on its way, when it fails, and right after it went through
//! (pure). A permission in a background job is answered by typing a digit into its terminal, which
//! takes seconds; a job with several permissions in a row then shows the next one in the same
//! place, looking the same, so without these words a click read as "nothing happened".

use notchling_kit::sentence;

use crate::acting::Decision;

/// How long "Allowed: Write notes.md" stays above whatever follows, in ms.
pub const CONFIRM_MS: i64 = 6_000;

/// What is being sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sending {
    /// An answer to a question.
    Answer,
    /// A permission's decision. `on_screen`: typed into the job's terminal, not given to a hook.
    Decision { decision: Decision, on_screen: bool },
}

/// The line on a card while its answer is going out.
pub fn sending_text(sending: Sending) -> &'static str {
    match sending {
        Sending::Answer => "Sending…",
        Sending::Decision {
            decision,
            on_screen: false,
        } => match decision {
            Decision::Allow => "Allowing…",
            Decision::Always => "Allowing, and not asking again…",
            Decision::Deny => "Denying…",
        },
        Sending::Decision {
            decision,
            on_screen: true,
        } => match decision {
            Decision::Allow => "Allowing… typing it into the session, a few seconds",
            Decision::Always => "Allowing, and not asking again… a few seconds",
            Decision::Deny => "Denying… typing it into the session, a few seconds",
        },
    }
}

/// The tools whose summary is the path of the file they work on.
const FILE_TOOLS: [&str; 5] = ["Write", "Edit", "MultiEdit", "NotebookEdit", "Read"];

/// How much of a command a confirmation names.
const SUBJECT_CHARS: usize = 48;

/// The file a tool works on, when the summary is its path: the last part of it, for the card's
/// headline (a long path wraps, and two requests in one folder look alike).
pub fn file_name<'a>(tool: &str, summary: &'a str) -> Option<&'a str> {
    if !FILE_TOOLS.contains(&tool.trim()) {
        return None;
    }
    let path = summary.trim();
    // A path cut short ("…") or spread over lines isn't one to take a name from.
    if path.contains('\n') || path.contains('…') || path.ends_with("...") {
        return None;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    (!name.is_empty()).then_some(name)
}

/// What a confirmation names after the tool: the file, or the command's first line, cut short.
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

/// "Allowed: Write notes.md": what the answer that just went through was about.
pub fn confirmation(tool: &str, summary: &str, decision: Decision) -> String {
    let verb = match decision {
        Decision::Allow => "Allowed",
        Decision::Always => "Always allowed",
        Decision::Deny => "Denied",
    };
    let subject = subject(tool, summary);
    if subject.is_empty() {
        format!("{verb}: {}", tool.trim())
    } else {
        format!("{verb}: {} {subject}", tool.trim())
    }
}

/// A decision that went through, shown for a moment.
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

/// The daemon's reason for a refusal as the card shows it. A call that timed out or lost the
/// connection may still have reached the session, which the words say.
pub fn failure_text(message: &str) -> String {
    let said = sentence(message.trim());
    if said.is_empty() {
        return said;
    }
    if message.contains("didn't answer in time") || message.contains("closed the connection") {
        return format!(
            "{}. It may still have reached the session: look before sending again.",
            said.trim_end_matches('.')
        );
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
        // Typed into a terminal: it takes seconds, and the card says so.
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
        // A bare name is its own headline.
        assert_eq!(file_name("Write", "notes.md"), Some("notes.md"));
        // Not a path tool, a path that is cut short, or nothing to name.
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
        // A long command is cut at a character, never mid-way through one, and keeps one line.
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
        // A reply that never came may still have gone through.
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
            // Read from a job's state like a real one, so it is typed into the terminal.
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
