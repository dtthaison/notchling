//! The header over the conversation: title, folder, context, status.

use std::path::Path;

use notchling_engine::catalog::ModelInfo;
use notchling_engine::transcript::model::Conversation;
use notchling_kit::Status;

use super::data::Session;
use super::format::{context_text, model_name, short_path};
use super::sidebar::status_of;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub title: String,
    /// The working folder, with `~` for the home folder.
    pub folder: String,
    pub status: Status,
    /// The status word beside the pill's icon.
    pub status_word: String,
    /// "148.8k / 400k · 37%", or the count alone, or nothing yet.
    pub context: Option<String>,
    /// "Opus 5.5".
    pub model: Option<String>,
    /// "Claude Code" or "Codex", plus where it runs.
    pub host: String,
    /// Background shells the transcript launched.
    pub shells: usize,
}

/// The header for a session and its conversation.
pub fn header(
    session: &Session,
    conversation: &Conversation,
    home: &Path,
    catalog: &[ModelInfo],
) -> Header {
    let agent = &session.agent;
    let tool = session.tool.name();
    let host = if agent.host.is_empty() {
        tool.to_owned()
    } else {
        format!("{tool} · {}", agent.host)
    };
    Header {
        title: agent.name.clone(),
        folder: short_path(&agent.cwd, home),
        status: status_of(agent.group),
        status_word: agent.label.clone(),
        context: context_text(
            conversation.facts.context_tokens,
            conversation.facts.model.as_deref(),
            catalog,
        ),
        model: conversation.facts.model.as_deref().map(model_name),
        host,
        shells: conversation.shells.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
    use notchling_engine::transcript::model::{BackgroundShell, SessionFacts};

    fn session() -> Session {
        Session {
            agent: Agent {
                id: "s1".into(),
                name: "fix the bug".into(),
                project: "notchling".into(),
                cwd: "/Users/me/work/notchling".into(),
                group: AgentGroup::Working,
                label: "Working".into(),
                detail: None,
                since_ms: 0,
                target: AgentTarget::Job("aa11".into()),
                suggested_reply: None,
                transcript_path: None,
                host: "Background".into(),
                headless: false,
                config_dir: None,
                command: None,
                resume_session_id: None,
            },
            tool: crate::chat::data::Tool::Claude,
            transcript: None,
        }
    }

    #[test]
    fn header_shows_folder_context_and_model() {
        let c = Conversation {
            facts: SessionFacts {
                model: Some("claude-opus-5-5".into()),
                context_tokens: 148_800,
                ..SessionFacts::default()
            },
            shells: vec![BackgroundShell {
                id: "b1".into(),
                command: "sleep".into(),
                title: String::new(),
                output_path: "/tmp/b1.output".into(),
                started: None,
            }],
            ..Conversation::default()
        };
        let h = header(&session(), &c, Path::new("/Users/me"), &[]);
        assert_eq!(h.title, "fix the bug");
        assert_eq!(h.folder, "~/work/notchling");
        assert_eq!(h.status, Status::Working);
        assert_eq!(h.status_word, "Working");
        assert_eq!(h.context.as_deref(), Some("148.8k / 400k · 37%"));
        assert_eq!(h.model.as_deref(), Some("Opus 5.5"));
        assert_eq!(h.host, "Claude Code · Background");
        assert_eq!(h.shells, 1);
    }

    #[test]
    fn empty_conversation_has_no_context() {
        let h = header(&session(), &Conversation::default(), Path::new("/x"), &[]);
        assert_eq!(h.context, None);
        assert_eq!(h.model, None);
        assert_eq!(h.folder, "/Users/me/work/notchling");
    }
}
