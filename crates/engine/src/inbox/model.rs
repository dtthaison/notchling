use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
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
    #[serde(other)]
    Notice,
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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
}
