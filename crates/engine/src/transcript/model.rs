//! The conversation model every reader produces: what Chat shows for a session.

use std::collections::HashMap;

/// Seconds since the Unix epoch.
pub type Timestamp = f64;

/// One step of a session as Chat shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatItem {
    pub id: String,
    pub kind: ItemKind,
    pub date: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    User {
        text: String,
        images: usize,
    },
    Assistant {
        text: String,
    },
    /// Claude Code keeps only how long it thought; the text is usually redacted.
    Thinking {
        seconds: f64,
        text: String,
    },
    Tools(Vec<ToolCall>),
    /// End of a turn: how long it took and with which model.
    TurnEnd {
        seconds: f64,
        model: Option<String>,
    },
    /// `symbol` is an SF Symbol name the Swift app drew; a Rust view maps it to its own icon.
    Notice {
        text: String,
        symbol: String,
    },
    /// What a slash command printed, such as /cost or /context.
    CommandOutput {
        text: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub kind: ToolKind,
    pub title: String,
    /// The command, path, query or input, as plain text.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Add,
    Remove,
    Context,
    Hunk,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
    pub number: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

impl TaskStatus {
    /// Claude Code's wire names: `pending`, `in_progress`, `completed`.
    pub fn parse(word: &str) -> Option<TaskStatus> {
        match word {
            "pending" => Some(TaskStatus::Pending),
            "in_progress" => Some(TaskStatus::InProgress),
            "completed" => Some(TaskStatus::Completed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskEntry {
    pub id: String,
    pub subject: String,
    pub status: TaskStatus,
}

/// The receipt for a session: where it runs and what it did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionFacts {
    pub branch: Option<String>,
    pub model: Option<String>,
    /// The reasoning effort the latest turn ran at, when the transcript says (None is the default).
    pub effort: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// What the model holds after the latest reply: its last request's input and output.
    pub context_tokens: u64,
    pub turns: u64,
    pub files: Vec<String>,
    pub commands: u64,
}

/// A Bash call Claude Code ran with `run_in_background`.
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundShell {
    pub id: String,
    pub command: String,
    /// The Bash call's description, when it gave one.
    pub title: String,
    pub output_path: String,
    pub started: Option<Timestamp>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Conversation {
    pub items: Vec<ChatItem>,
    pub tasks: Vec<TaskEntry>,
    pub facts: SessionFacts,
    /// What it's doing right now: the tool still waiting for a result.
    pub activity: Option<String>,
    /// When your latest prompt was sent.
    pub turn_start: Option<Timestamp>,
    /// Tokens written since (Claude Code's "47s · down 2.3k tokens" while it works).
    pub turn_tokens: u64,
    /// Your messages typed while it worked, waiting for the turn to end.
    pub queued: Vec<String>,
    /// How subagents ended, by agent id (a background agent's task notification) or by the Agent
    /// call's tool use id (a foreground agent's result).
    pub finished_agents: HashMap<String, String>,
    /// Bash calls run in the background, oldest first.
    pub shells: Vec<BackgroundShell>,
}

impl Conversation {
    /// The text of your last few messages.
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
