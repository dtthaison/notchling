//! The vault approval prompt, pure (the Swift `VaultPrompt.approve`): an agent waits to use
//! secrets marked ask-first, and only the app can say yes. The words are the Swift panel's: the
//! names, the folder, the command, what Deny is for, and Always Allow / Deny / Allow Once.
//! Closing the window counts as Deny. The daemon denies on its own after `APPROVAL_WAIT` (300 s).

use serde_json::Value;

use super::wire::Approval;

pub const WINDOW_TITLE: &str = "Use a Secret";
pub const EXPLANATION: &str = "The command gets the values as environment variables; its output comes back with them hidden. Deny it if the command would print, send or save a value somewhere you don't expect.";
pub const ALWAYS: &str = "Always Allow";
pub const DENY: &str = "Deny";
pub const ONCE: &str = "Allow Once";
pub const FOLDER: &str = "Folder";
pub const COMMAND: &str = "Command";

/// The prompt window's size in points.
pub const PROMPT_SIZE: (f32, f32) = (500.0, 400.0);

/// What the prompt draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    /// The id the answer names.
    pub id: String,
    /// "Allow an agent to use github, npm?"
    pub title: String,
    pub folder: String,
    pub command: String,
    pub explanation: &'static str,
    /// "Stop asking for github, npm; you can turn it back on in the vault".
    pub always_tip: String,
    /// "1 more waiting" while others queue behind this one.
    pub more: Option<String>,
}

impl Prompt {
    pub fn new(a: &Approval, more: usize) -> Prompt {
        let names = a.names.join(", ");
        Prompt {
            id: a.id.clone(),
            title: format!("Allow an agent to use {names}?"),
            folder: if a.folder.is_empty() {
                "unknown folder".to_owned()
            } else {
                a.folder.clone()
            },
            command: a.command.clone(),
            explanation: EXPLANATION,
            always_tip: format!("Stop asking for {names}; you can turn it back on in the vault"),
            more: match more {
                0 => None,
                n => Some(format!("{n} more waiting")),
            },
        }
    }
}

/// The prompt for the oldest approval still pending, out of the feed's events.
pub fn prompt(pending: &[Value]) -> Option<Prompt> {
    let approvals: Vec<Approval> = pending.iter().filter_map(Approval::from_event).collect();
    let first = approvals.first()?;
    Some(Prompt::new(first, approvals.len() - 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_prompt_says_the_swift_panels_words() {
        let pending = vec![
            json!({"id": "ap-1", "names": ["github", "npm"], "command": "gh api /user",
                   "folder": "/Users/me/work"}),
            json!({"id": "ap-2", "names": ["aws"], "command": "aws s3 ls", "folder": ""}),
            json!({"names": ["no id"]}),
        ];
        let p = prompt(&pending).unwrap();
        assert_eq!(p.id, "ap-1");
        assert_eq!(p.title, "Allow an agent to use github, npm?");
        assert_eq!(
            (p.folder.as_str(), p.command.as_str()),
            ("/Users/me/work", "gh api /user")
        );
        assert_eq!(p.explanation, EXPLANATION);
        assert_eq!(
            p.always_tip,
            "Stop asking for github, npm; you can turn it back on in the vault"
        );
        assert_eq!(p.more.as_deref(), Some("1 more waiting"));
        let second = prompt(&pending[1..]).unwrap();
        assert_eq!(
            (second.folder.as_str(), second.more),
            ("unknown folder", None)
        );
        assert_eq!(prompt(&pending[2..]), None);
        assert_eq!(prompt(&[]), None);
    }
}
