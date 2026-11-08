//! The inspector beside the conversation: tasks, the session's facts, subagents, background shells.

use std::path::Path;

use notchling_engine::transcript::model::{Conversation, TaskStatus};
use notchling_engine::transcript::shells::{self, ShellState};
use notchling_engine::transcript::subagents::{Status as SubagentStatus, Subagent};
use notchling_kit::{Icon, Status};

use super::format::{model_name, short_path, tokens};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRow {
    pub subject: String,
    pub status: Status,
    pub icon: Icon,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubagentRow {
    pub description: String,
    /// "Explore · sonnet · high".
    pub detail: String,
    pub status: Status,
    pub status_word: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellRow {
    pub title: String,
    pub command: String,
    pub status: Status,
    pub status_word: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inspector {
    pub tasks: Vec<TaskRow>,
    /// Label and value, in display order.
    pub facts: Vec<(&'static str, String)>,
    pub subagents: Vec<SubagentRow>,
    pub shells: Vec<ShellRow>,
}

fn task_row(subject: &str, status: TaskStatus) -> TaskRow {
    let (status, icon) = match status {
        TaskStatus::Pending => (Status::Needs, Icon::More),
        TaskStatus::InProgress => (Status::Working, Icon::Working),
        TaskStatus::Completed => (Status::Done, Icon::Check),
    };
    TaskRow {
        subject: subject.to_owned(),
        status,
        icon,
    }
}

fn subagent_row(agent: &Subagent) -> SubagentRow {
    let mut detail = vec![agent.agent_type.clone()];
    detail.extend(agent.model.clone());
    detail.extend(agent.effort.clone());
    if agent.background {
        detail.push("background".into());
    }
    let (status, word) = match agent.status {
        SubagentStatus::Running => (Status::Working, "Running"),
        SubagentStatus::Completed => (Status::Done, "Done"),
        SubagentStatus::Failed => (Status::Needs, "Failed"),
        SubagentStatus::Stopped => (Status::Needs, "Stopped"),
        SubagentStatus::Quiet => (Status::Done, "Quiet"),
    };
    SubagentRow {
        description: agent.description.clone(),
        detail: detail.join(" · "),
        status,
        status_word: word.to_owned(),
    }
}

/// Facts as the inspector lists them: only what the transcript says.
pub fn facts(conversation: &Conversation, home: &Path) -> Vec<(&'static str, String)> {
    let f = &conversation.facts;
    let mut out: Vec<(&'static str, String)> = Vec::new();
    if let Some(branch) = &f.branch {
        out.push(("Branch", branch.clone()));
    }
    if let Some(model) = &f.model {
        out.push(("Model", model_name(model)));
    }
    if let Some(effort) = &f.effort {
        out.push(("Effort", effort.clone()));
    }
    if f.turns > 0 {
        out.push(("Turns", f.turns.to_string()));
    }
    if f.input_tokens + f.output_tokens > 0 {
        out.push((
            "Tokens",
            format!(
                "{} in · {} out",
                tokens(f.input_tokens),
                tokens(f.output_tokens)
            ),
        ));
    }
    if f.commands > 0 {
        out.push(("Commands", f.commands.to_string()));
    }
    if !f.files.is_empty() {
        let names: Vec<String> = f.files.iter().map(|p| short_path(p, home)).collect();
        out.push(("Files", names.join("\n")));
    }
    out
}

/// The inspector for a conversation. Shells are read-only here: running means a process still has
/// the output file open, which only the live app can ask (`writers`); a reader passes none, so a
/// shell with no notification shows as ended.
pub fn inspector(
    conversation: &Conversation,
    subagents: &[Subagent],
    home: &Path,
    writers: &dyn Fn(&str) -> Vec<i32>,
) -> Inspector {
    Inspector {
        tasks: conversation
            .tasks
            .iter()
            .map(|t| task_row(&t.subject, t.status))
            .collect(),
        facts: facts(conversation, home),
        subagents: subagents.iter().map(subagent_row).collect(),
        shells: conversation
            .shells
            .iter()
            .map(|shell| {
                let state = shells::state(shell, &conversation.finished_agents, writers);
                let (status, word) = match state {
                    ShellState::Running => (Status::Working, "Running".to_owned()),
                    ShellState::Ended(how) => (
                        if how == "completed" {
                            Status::Done
                        } else {
                            Status::Needs
                        },
                        capitalized(&how),
                    ),
                };
                ShellRow {
                    title: if shell.title.is_empty() {
                        shell.command.clone()
                    } else {
                        shell.title.clone()
                    },
                    command: shell.command.clone(),
                    status,
                    status_word: word,
                }
            })
            .collect(),
    }
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// No process can be asked about: nothing writes.
pub fn no_writers(_: &str) -> Vec<i32> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::transcript::model::{BackgroundShell, SessionFacts, TaskEntry};

    #[test]
    fn tasks_facts_and_shells() {
        let mut c = Conversation {
            tasks: vec![
                TaskEntry {
                    id: "1".into(),
                    subject: "Fix".into(),
                    status: TaskStatus::Completed,
                },
                TaskEntry {
                    id: "2".into(),
                    subject: "Port".into(),
                    status: TaskStatus::InProgress,
                },
            ],
            facts: SessionFacts {
                branch: Some("main".into()),
                model: Some("claude-opus-5-5".into()),
                effort: Some("high".into()),
                input_tokens: 15_400,
                output_tokens: 900,
                turns: 2,
                files: vec!["/Users/me/w/a.rs".into()],
                commands: 3,
                ..SessionFacts::default()
            },
            shells: vec![
                BackgroundShell {
                    id: "b1".into(),
                    command: "cargo test".into(),
                    title: "Run tests".into(),
                    output_path: "/tmp/b1.output".into(),
                    started: None,
                },
                BackgroundShell {
                    id: "b2".into(),
                    command: "sleep 9".into(),
                    title: String::new(),
                    output_path: "/tmp/b2.output".into(),
                    started: None,
                },
            ],
            ..Conversation::default()
        };
        c.finished_agents.insert("b1".into(), "completed".into());
        let i = inspector(&c, &[], Path::new("/Users/me"), &no_writers);
        assert_eq!(i.tasks[0].status, Status::Done);
        assert_eq!(i.tasks[1].icon, Icon::Working);
        assert_eq!(
            i.facts,
            [
                ("Branch", "main".to_owned()),
                ("Model", "Opus 5.5".to_owned()),
                ("Effort", "high".to_owned()),
                ("Turns", "2".to_owned()),
                ("Tokens", "15.4k in · 900 out".to_owned()),
                ("Commands", "3".to_owned()),
                ("Files", "~/w/a.rs".to_owned()),
            ]
        );
        assert_eq!(i.shells[0].title, "Run tests");
        assert_eq!(
            (i.shells[0].status, i.shells[0].status_word.as_str()),
            (Status::Done, "Completed")
        );
        assert_eq!(i.shells[1].title, "sleep 9", "no description: the command");
        assert_eq!(i.shells[1].status_word, "Ended");
    }

    #[test]
    fn subagent_rows() {
        let agent = Subagent {
            id: "abg".into(),
            path: String::new(),
            description: "Survey the repo".into(),
            agent_type: "Explore".into(),
            model: Some("sonnet".into()),
            effort: Some("high".into()),
            background: true,
            tool_use_id: None,
            started: 0.0,
            updated: 0.0,
            status: SubagentStatus::Completed,
        };
        let i = inspector(
            &Conversation::default(),
            &[agent],
            Path::new("/x"),
            &no_writers,
        );
        assert_eq!(
            i.subagents[0],
            SubagentRow {
                description: "Survey the repo".into(),
                detail: "Explore · sonnet · high · background".into(),
                status: Status::Done,
                status_word: "Done".into()
            }
        );
        assert!(i.facts.is_empty() && i.tasks.is_empty() && i.shells.is_empty());
    }
}
