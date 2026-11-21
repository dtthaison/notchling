use serde_json::Value;

pub const DEFAULT_EXPIRY_MS: i64 = 15 * 60_000;
pub const MIN_EXPIRY_MS: i64 = 60_000;
pub const MAX_EXPIRY_MS: i64 = 12 * 3_600_000;
pub const LONG_EXPIRY_MS: i64 = 3_600_000;
pub const OPENED_MIN_MS: i64 = 10 * 60_000;
pub const REASON_CHARS: usize = 300;
pub const KEEP_CLOSED_MS: i64 = 7 * 24 * 3_600_000;
pub const IDEM_MS: i64 = 24 * 3_600_000;
pub const WAITING_PER_SESSION: usize = 3;
pub const WAITING_ALL: usize = 20;
pub const OPENED_PER_HOUR: usize = 10;
pub const LONG_PER_SESSION: usize = 1;
pub const LONG_ALL: usize = 5;
pub const COOLDOWN_MS: i64 = 30 * 60_000;
pub const ROOT_WAITING: usize = 1;
pub const ROOT_PER_HOUR: usize = 3;
pub const ROOT_MAX_EXPIRY_MS: i64 = 15 * 60_000;
pub const HOUR_MS: i64 = 3_600_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Waiting,
    Opened,
    Applying,
    Applied,
    Failed,
    Denied,
    Expired,
    Cancelled,
    Withdrawn,
}

impl State {
    pub const ALL: [State; 9] = [
        State::Waiting,
        State::Opened,
        State::Applying,
        State::Applied,
        State::Failed,
        State::Denied,
        State::Expired,
        State::Cancelled,
        State::Withdrawn,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            State::Waiting => "waiting",
            State::Opened => "opened",
            State::Applying => "applying",
            State::Applied => "applied",
            State::Failed => "failed",
            State::Denied => "denied",
            State::Expired => "expired",
            State::Cancelled => "cancelled",
            State::Withdrawn => "withdrawn",
        }
    }

    pub fn parse(text: &str) -> Option<State> {
        State::ALL.into_iter().find(|s| s.as_str() == text)
    }

    pub fn closed(self) -> bool {
        !matches!(self, State::Waiting | State::Opened | State::Applying)
    }

    pub fn open(self) -> bool {
        matches!(self, State::Waiting | State::Opened)
    }

    pub fn next(self, to: State) -> bool {
        use State::*;
        match self {
            Waiting => matches!(
                to,
                Opened | Applying | Denied | Expired | Cancelled | Withdrawn
            ),
            Opened => matches!(
                to,
                Applying | Applied | Denied | Expired | Withdrawn | Cancelled | Waiting
            ),
            Applying => matches!(to, Applied | Failed | Withdrawn),
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnswerOn {
    Mac,
    MacOrPhone,
}

impl AnswerOn {
    pub fn as_str(self) -> &'static str {
        match self {
            AnswerOn::Mac => "mac",
            AnswerOn::MacOrPhone => "mac_or_phone",
        }
    }

    pub fn parse(text: &str) -> Option<AnswerOn> {
        match text {
            "mac" => Some(AnswerOn::Mac),
            "mac_or_phone" => Some(AnswerOn::MacOrPhone),
            _ => None,
        }
    }

    pub fn may_allow(self, by: &str) -> bool {
        match self {
            AnswerOn::Mac => by == "mac",
            AnswerOn::MacOrPhone => by == "mac" || by == "phone",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppliedBy {
    Daemon,
    App,
}

impl AppliedBy {
    pub fn as_str(self) -> &'static str {
        match self {
            AppliedBy::Daemon => "daemon",
            AppliedBy::App => "app",
        }
    }

    pub fn parse(text: &str) -> Option<AppliedBy> {
        match text {
            "daemon" => Some(AppliedBy::Daemon),
            "app" => Some(AppliedBy::App),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Consent {
    pub id: String,
    pub kind: String,
    pub target: String,
    pub change: Value,
    pub facts: Value,
    pub parts: Option<Value>,
    pub parts_allowed: Option<Value>,
    pub reason: Option<String>,
    pub session: Option<String>,
    pub tool: String,
    pub verified: bool,
    pub root: Option<String>,
    pub folder: Option<String>,
    pub organization: Option<String>,
    pub pid: u32,
    pub pid_start: Option<u64>,
    pub idem_key: Option<String>,
    pub state: State,
    pub answer_on: AnswerOn,
    pub applied_by: AppliedBy,
    pub created_ms: i64,
    pub expires_ms: i64,
    pub answered_ms: Option<i64>,
    pub by: Option<String>,
    pub used_ms: Option<i64>,
    pub used_op_id: Option<String>,
    pub result: Option<Value>,
    pub closed_ms: Option<i64>,
}

impl Consent {
    pub fn bucket(&self) -> String {
        bucket_of(self.verified, self.session.as_deref(), self.root.as_deref())
    }
}

pub fn bucket_of(verified: bool, session: Option<&str>, root: Option<&str>) -> String {
    match (verified, session) {
        (true, Some(session)) => format!("session:{session}"),
        _ => format!("root:{}", root.unwrap_or("unrooted")),
    }
}

#[derive(Clone, Debug)]
pub struct New {
    pub kind: String,
    pub target: String,
    pub change: Value,
    pub facts: Value,
    pub parts: Option<Value>,
    pub reason: Option<String>,
    pub session: Option<String>,
    pub tool: String,
    pub verified: bool,
    pub root: Option<String>,
    pub folder: Option<String>,
    pub organization: Option<String>,
    pub pid: u32,
    pub pid_start: Option<u64>,
    pub idem_key: Option<String>,
    pub answer_on: AnswerOn,
    pub applied_by: AppliedBy,
    pub expires_in_ms: i64,
}

impl New {
    pub fn bucket(&self) -> String {
        bucket_of(self.verified, self.session.as_deref(), self.root.as_deref())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    Busy { retry_after_ms: i64 },
    LongExpiry,
    KeyReused,
    Db(String),
}

pub fn new_id() -> String {
    crate::vault::random_id("c").unwrap_or_else(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("c{nanos:032x}")
    })
}

pub fn clean_reason(text: &str) -> String {
    text.chars()
        .filter(|c| *c == '\n' || !c.is_control())
        .take(REASON_CHARS)
        .collect()
}
