use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    SecretApproval,
    SecretRequest,
    LimitNotice,
    ActionFailed,
    SettingChange,
    VersionCheckFailed,
    VersionDrift,
    NodeJoin,
    OutboxUnsent,
    LoginNeeded,
    PrimaryUnreachable,
    Failover,
    PausedStep,
    NoPrimary,
    PrimaryBack,
    SessionWaits,
    Question,
    Consent,
    ShellStuck,
    #[serde(other)]
    #[default]
    Notice,
}

pub const SHELL_STUCK_ID_PREFIX: &str = "shell-stuck:";

impl Kind {
    pub fn stays_on_node(self) -> bool {
        matches!(self, Kind::ShellStuck)
    }
}

impl Item {
    pub fn stays_on_node(&self) -> bool {
        self.kind.stays_on_node()
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionId {
    Allow,
    AllowOnce,
    AllowAlways,
    Deny,
    Continue,
    Cancel,
    Done,
    Open,
    Send,
    Discard,
    MoveHere,
    MoveAnyway,
    TakeBack,
    Keep,
    ContinueHere,
    WaitFor,
    Stop,
    KeepWaiting,
    #[serde(rename = "choice_1")]
    Choice1,
    #[serde(rename = "choice_2")]
    Choice2,
    #[serde(rename = "choice_3")]
    Choice3,
    #[serde(rename = "choice_4")]
    Choice4,
    #[serde(rename = "choice_5")]
    Choice5,
    #[serde(rename = "choice_6")]
    Choice6,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Action {
    pub id: ActionId,
    pub label: String,
    pub primary: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    #[default]
    Open,
    Answered,
    Expired,
    Withdrawn,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Source {
    pub node: Option<String>,
    pub session: Option<String>,
    pub job: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Waiter {
    Cli,
    Ask,
    Vault,
    Cluster,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AlertAction {
    Allow,
    Deny,
    Change,
    Open,
    Reply,
    Dismiss,
    OpenForm,
    Cancel,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeState {
    Applying,
    Applied,
    Failed,
    Withdrawn,
}

pub const NOTE_MAX: usize = 300;
pub const REASON_MAX: usize = 300;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Outcome {
    pub state: OutcomeState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub at_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Place {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Live {
    pub waiting_since: i64,
    pub waiter: Waiter,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub id: String,
    pub kind: Kind,
    pub title: String,
    pub detail: String,
    pub source: Source,
    pub created_ms: i64,
    pub expires_ms: Option<i64>,
    pub actions: Vec<Action>,
    pub state: State,
    pub answer: Option<ActionId>,
    pub answered_by: Option<String>,
    pub seen: bool,
    pub open: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub switch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocking: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_since: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiter: Option<Waiter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alert_actions: Vec<AlertAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<Place>,
}

impl Item {
    pub fn without_live(&self) -> Item {
        Item {
            blocking: None,
            waiting_since: None,
            waiter: None,
            ..self.clone()
        }
    }

    pub fn clear_live(&mut self) {
        self.blocking = None;
        self.waiting_since = None;
        self.waiter = None;
    }

    pub fn is_blocking(&self) -> bool {
        self.blocking == Some(true)
    }
}
