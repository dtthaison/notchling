use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_ROWS: usize = 200;
pub const MAX_TEXT: usize = 100 * 1024;

macro_rules! words {
    ($name:ident { $($variant:ident => $word:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $word),+
                }
            }

            pub fn parse(word: &str) -> Option<Self> {
                match word {
                    $($word => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

words!(Tool {
    Claude => "claude",
    Codex => "codex",
    Antigravity => "antigravity",
});

words!(Kind {
    Prompt => "prompt",
    Command => "command",
});

words!(Via {
    Chat => "chat",
    Notch => "notch",
    Phone => "phone",
    Agent => "agent",
});

words!(State {
    Queued => "queued",
    Waiting => "waiting",
    Sending => "sending",
    InQueue => "in_queue",
    Unsure => "unsure",
    Box => "box",
    Stopped => "stopped",
    Gone => "gone",
});

words!(Why {
    Dialog => "dialog",
    Queue => "queue",
    Turn => "turn",
    Retry => "retry",
    NotFound => "not_found",
    Restart => "restart",
    Dropped => "dropped",
    NotCleared => "not_cleared",
    Mixed => "mixed",
    Answer => "answer",
    Checking => "checking",
    Unreadable => "unreadable",
});

impl Why {
    pub fn stored(self) -> Option<Why> {
        match self {
            Why::Checking | Why::Unreadable => None,
            other => Some(other),
        }
    }
}

impl Kind {
    pub fn of(tool: Tool, text: &str) -> Kind {
        if tool == Tool::Claude && text.trim_start().starts_with('/') {
            Kind::Command
        } else {
            Kind::Prompt
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub file: Option<String>,
    pub slack_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub n: i64,
    pub id: String,
    pub session: String,
    pub tool: Tool,
    pub kind: Kind,
    pub text: String,
    pub options: Option<Value>,
    pub via: Via,
    pub state: State,
    pub why: Option<Why>,
    pub clear_box: bool,
    pub queued_ms: i64,
    pub updated_ms: i64,
    pub attempt_ms: Option<i64>,
    pub seen_before: Option<i64>,
    pub queued_before: Option<i64>,
    pub tries: u32,
    pub next_ms: Option<i64>,
    pub noticed_ms: Option<i64>,
    pub anchor: Option<Anchor>,
    pub aside_at_ms: Option<i64>,
    pub aside_ms: Option<i64>,
    pub hold_ms: Option<i64>,
    pub joined: u32,
    #[serde(default)]
    pub awaited: bool,
    #[serde(default)]
    pub done_from: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct New {
    pub session: String,
    pub tool: Tool,
    pub text: String,
    pub options: Option<Value>,
    pub via: Via,
    pub clear_box: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertError {
    Empty,
    TooLong,
    Full,
    Db(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Done,
    Busy,
    Missing,
    Stale,
}

pub fn new_id() -> String {
    crate::vault::random_id("o").unwrap_or_else(fallback_id)
}

fn fallback_id() -> String {
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    let digest = hasher.finalize();
    let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("o{hex}")
}
