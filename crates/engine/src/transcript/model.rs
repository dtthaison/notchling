use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::snapshot::PendingQuestion;

pub type Timestamp = f64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatItem {
    pub id: String,
    pub kind: ItemKind,
    pub date: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ItemKind {
    User { text: String, images: usize },
    Assistant { text: String },
    Thinking { seconds: f64, text: String },
    Tools(Vec<ToolCall>),
    TurnEnd { seconds: f64, model: Option<String> },
    Notice { text: String, symbol: String },
    CommandOutput { text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Shell,
    Read,
    Edit,
    Write,
    Search,
    Web,
    Agent,
    Task,
    Ask,
    Skill,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub kind: ToolKind,
    pub title: String,
    pub input: String,
    pub output: Option<String>,
    pub is_error: bool,
    pub diff: Vec<DiffLine>,
    pub finished: bool,
}

impl ToolCall {
    pub fn new(id: &str, name: &str, kind: ToolKind, title: String, input: String) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            title,
            input,
            output: None,
            is_error: false,
            diff: Vec::new(),
            finished: false,
        }
    }

    pub fn added(&self) -> usize {
        self.diff.iter().filter(|d| d.kind == DiffKind::Add).count()
    }

    pub fn removed(&self) -> usize {
        self.diff
            .iter()
            .filter(|d| d.kind == DiffKind::Remove)
            .count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffKind {
    Add,
    Remove,
    Context,
    Hunk,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
    pub number: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

impl TaskStatus {
    pub fn parse(word: &str) -> Option<TaskStatus> {
        match word {
            "pending" => Some(TaskStatus::Pending),
            "in_progress" => Some(TaskStatus::InProgress),
            "completed" => Some(TaskStatus::Completed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskEntry {
    pub id: String,
    pub subject: String,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionFacts {
    pub branch: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub context_tokens: u64,
    pub turns: u64,
    pub files: Vec<String>,
    pub commands: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackgroundShell {
    pub id: String,
    pub command: String,
    pub title: String,
    pub output_path: String,
    pub started: Option<Timestamp>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Conversation {
    pub items: Vec<ChatItem>,
    pub tasks: Vec<TaskEntry>,
    pub facts: SessionFacts,
    pub activity: Option<String>,
    pub turn_start: Option<Timestamp>,
    pub turn_tokens: u64,
    pub queued: Vec<String>,
    pub finished_agents: HashMap<String, String>,
    pub shells: Vec<BackgroundShell>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_question: Option<PendingQuestion>,
}

impl Conversation {
    pub fn last_user_texts(&self) -> Vec<String> {
        let start = self.items.len().saturating_sub(8);
        self.items[start..]
            .iter()
            .filter_map(|i| match &i.kind {
                ItemKind::User { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }
}

impl ItemKind {
    pub fn user(text: &str) -> ItemKind {
        ItemKind::User {
            text: text.to_string(),
            images: 0,
        }
    }

    pub fn notice(text: &str, symbol: &str) -> ItemKind {
        ItemKind::Notice {
            text: text.to_string(),
            symbol: symbol.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conversation_round_trips_through_json_and_missing_fields_default() {
        let mut call = ToolCall::new(
            "t1",
            "Edit",
            ToolKind::Edit,
            "a.rs".into(),
            "/w/a.rs".into(),
        );
        call.diff.push(DiffLine {
            kind: DiffKind::Add,
            text: "let x = 1;".into(),
            number: Some(3),
        });
        call.output = Some("ok".into());
        call.finished = true;
        let conversation = Conversation {
            items: vec![
                ChatItem {
                    id: "u1".into(),
                    kind: ItemKind::user("hi"),
                    date: Some(1.5),
                },
                ChatItem {
                    id: "t".into(),
                    kind: ItemKind::Tools(vec![call]),
                    date: None,
                },
                ChatItem {
                    id: "e".into(),
                    kind: ItemKind::TurnEnd {
                        seconds: 2.0,
                        model: Some("m".into()),
                    },
                    date: None,
                },
            ],
            tasks: vec![TaskEntry {
                id: "1".into(),
                subject: "s".into(),
                status: TaskStatus::InProgress,
            }],
            activity: Some("Editing".into()),
            queued: vec!["next".into()],
            ..Conversation::default()
        };
        let json = serde_json::to_value(&conversation).unwrap();
        assert_eq!(json["items"][0]["kind"]["type"], "user");
        assert_eq!(json["items"][1]["kind"]["type"], "tools");
        assert_eq!(json["items"][1]["kind"]["data"][0]["kind"], "edit");
        assert_eq!(json["tasks"][0]["status"], "in_progress");
        let back: Conversation = serde_json::from_value(json).unwrap();
        assert_eq!(back, conversation);
        let partial: Conversation =
            serde_json::from_value(serde_json::json!({"items": [], "later": 1})).unwrap();
        assert_eq!(partial, Conversation::default());
    }
}
