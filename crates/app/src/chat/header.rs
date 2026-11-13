use std::path::Path;

use notchling_engine::catalog::ModelInfo;
use notchling_engine::transcript::model::Conversation;
use notchling_kit::Status;

use super::claude_picker::model_label;
use super::data::Session;
use super::format::{context_text, context_window_source, model_name, short_path};
use super::sidebar::status_of;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub title: String,
    pub folder: String,
    pub status: Status,
    pub status_word: String,
    pub context: Option<String>,
    pub context_note: Option<String>,
    pub model: Option<String>,
    pub host: String,
    pub shells: usize,
}

fn window_note(model: Option<&str>, catalog: &[ModelInfo]) -> Option<String> {
    let from = context_window_source(model, catalog)?;
    Some(format!(
        "Window as {} until Notchling knows this model",
        model_name(&from)
    ))
}

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
        context_note: window_note(conversation.facts.model.as_deref(), catalog),
        model: conversation
            .facts
            .model
            .as_deref()
            .map(|id| model_label(Some(id), catalog)),
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

    fn on(model: &str) -> Conversation {
        Conversation {
            facts: SessionFacts {
                model: Some(model.into()),
                context_tokens: 148_800,
                ..SessionFacts::default()
            },
            ..Conversation::default()
        }
    }

    #[test]
    fn a_borrowed_window_says_so_and_a_known_one_does_not() {
        use notchling_engine::catalog::{Catalog, LiveModel, Tool};
        let live = LiveModel {
            id: "claude-opus-5-6".into(),
            name: Some("Claude Opus 5.6".into()),
            ..LiveModel::default()
        };
        let catalog = Catalog::built_in().models(Tool::Claude, &[live]);
        let home = Path::new("/Users/me");
        let new = header(&session(), &on("claude-opus-5-6"), home, &catalog);
        assert_eq!(new.context.as_deref(), Some("148.8k / 400k · 37%"));
        assert_eq!(
            new.context_note.as_deref(),
            Some("Window as Opus 5.5 until Notchling knows this model")
        );
        assert_eq!(new.model.as_deref(), Some("Opus 5.6"));
        let known = header(&session(), &on("claude-opus-5-5"), home, &catalog);
        assert_eq!(known.context_note, None);
        let none = header(&session(), &on("claude-sonnet-5-5"), home, &catalog);
        assert_eq!(none.context_note, None);
        let early = header(&session(), &on("claude-opus-5-5"), home, &[]);
        assert_eq!(early.context_note, None);
    }

    #[test]
    fn empty_conversation_has_no_context() {
        let h = header(&session(), &Conversation::default(), Path::new("/x"), &[]);
        assert_eq!(h.context, None);
        assert_eq!(h.model, None);
        assert_eq!(h.folder, "/Users/me/work/notchling");
    }
}
