use serde::{Deserialize, Serialize};

pub const MAX_QUESTION_CHARS: usize = 2_000;
pub const MIN_CHOICES: usize = 2;
pub const MAX_CHOICES: usize = 6;
pub const MAX_CHOICE_CHARS: usize = 120;
pub const MAX_ANSWER_BYTES: usize = 10 * 1024;
pub const MAX_OPEN_PER_SESSION: usize = 20;
pub const MAX_OPEN: usize = 100;
pub const KEEP_CLOSED_MS: i64 = 86_400_000;
pub const WAIT_MS_MAX: i64 = 50_000;
pub const FIND_MS: i64 = 10_000;
pub const FIND_WINDOW_MS: i64 = 600_000;
pub const FIND_MAX_FILES: usize = 64;
pub const FIND_TAIL_BYTES: u64 = 256 * 1024;
pub const MAIN_FIND_MS: i64 = 5_000;
pub const TITLE_CHARS: usize = 80;
pub const DETAIL_CHARS: usize = 400;
pub const CHOICE_LABEL_CHARS: usize = 60;
pub const WAIT_DEFAULT_S: u64 = 900;
pub const DEADLINE_MARGIN_MS: i64 = 60_000;
pub const DEADLINE_VARIABLE: &str = "NOTCHLING_DELEGATE_DEADLINE_MS";

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

words!(Asker {
    Main => "main",
    Subagent => "subagent",
    Codex => "codex",
    Antigravity => "antigravity",
    Unknown => "unknown",
});

words!(Mode {
    Background => "background",
    Wait => "wait",
});

words!(AskState {
    Open => "open",
    Answered => "answered",
    Delivered => "delivered",
    Dismissed => "dismissed",
    Expired => "expired",
    Gone => "gone",
});

words!(By {
    Mac => "mac",
    Phone => "phone",
    Main => "main",
});

impl AskState {
    pub fn is_closed(self) -> bool {
        matches!(
            self,
            AskState::Delivered | AskState::Dismissed | AskState::Expired | AskState::Gone
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    pub asker: Asker,
    pub agent_id: Option<String>,
    pub label: Option<String>,
    pub agent_type: Option<String>,
    pub model: Option<String>,
    pub tool_use_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct New {
    pub key: String,
    pub session: String,
    pub session_id: Option<String>,
    pub pid: u32,
    pub pid_start: Option<u64>,
    pub folder: Option<String>,
    pub branch: Option<String>,
    pub question: String,
    pub choices: Option<Vec<String>>,
    pub main_may_answer: bool,
    pub mode: Mode,
    pub deadline_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub n: i64,
    pub id: String,
    pub key: String,
    pub session: String,
    pub session_id: Option<String>,
    pub asker: Asker,
    pub agent_id: Option<String>,
    pub label: Option<String>,
    pub agent_type: Option<String>,
    pub model: Option<String>,
    pub tool_use_id: Option<String>,
    pub folder: Option<String>,
    pub branch: Option<String>,
    pub question: String,
    pub choices: Option<Vec<String>>,
    pub main_may_answer: bool,
    pub mode: Mode,
    pub deadline_ms: Option<i64>,
    pub pid: u32,
    pub pid_start: Option<u64>,
    pub state: AskState,
    pub answer: Option<String>,
    pub choice: Option<u8>,
    pub by: Option<By>,
    pub inbox_id: Option<String>,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub closed_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    Empty,
    TooLong,
    Choices,
    ChoiceText,
    WorkerNeedsChoices,
    SessionFull,
    AllFull,
    Db(String),
}

pub fn new_id() -> String {
    crate::vault::random_id("q").unwrap_or_else(fallback_id)
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
    format!("q{hex}")
}
