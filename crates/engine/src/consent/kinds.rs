use super::model::{AnswerOn, AppliedBy, DEFAULT_EXPIRY_MS, MAX_EXPIRY_MS};
use crate::inbox::Kind as ItemKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    None,
    Stored,
    Handover,
}

pub struct KindSpec {
    pub name: &'static str,
    pub title: &'static str,
    pub answer_on: AnswerOn,
    pub applied_by: AppliedBy,
    pub result: Outcome,
    pub item: ItemKind,
    pub default_expiry_ms: i64,
    pub max_expiry_ms: i64,
    pub parts: bool,
}

pub const SETTING: &str = "setting";
pub const SECRET_REQUEST: &str = "secret_request";
pub const SWITCH: &str = "switch";
pub const SESSION: &str = "session";
pub const SESSION_STOP: &str = "session_stop";
pub const DISPATCH: &str = "dispatch";
pub const JOB: &str = "job";
pub const JOB_STOP: &str = "job_stop";
pub const CLUSTER: &str = "cluster";
pub const ACCOUNT: &str = "account";
pub const MACHINE: &str = "machine";
pub const UPDATE: &str = "update";
pub const INSTALL: &str = "install";
pub const COST_PROFILE: &str = "cost_profile";
pub const MODEL: &str = "model";

pub const KINDS: &[KindSpec] = &[
    KindSpec {
        name: SETTING,
        title: "Change a setting",
        answer_on: AnswerOn::MacOrPhone,
        applied_by: AppliedBy::Daemon,
        result: Outcome::Stored,
        item: ItemKind::SettingChange,
        default_expiry_ms: DEFAULT_EXPIRY_MS,
        max_expiry_ms: MAX_EXPIRY_MS,
        parts: false,
    },
    KindSpec {
        name: SECRET_REQUEST,
        title: "Add a secret",
        answer_on: AnswerOn::Mac,
        applied_by: AppliedBy::App,
        result: Outcome::Stored,
        item: ItemKind::SecretRequest,
        default_expiry_ms: DEFAULT_EXPIRY_MS,
        max_expiry_ms: MAX_EXPIRY_MS,
        parts: false,
    },
    KindSpec {
        name: SWITCH,
        title: "Turn a switch on",
        answer_on: AnswerOn::Mac,
        applied_by: AppliedBy::App,
        result: Outcome::None,
        item: ItemKind::Consent,
        default_expiry_ms: DEFAULT_EXPIRY_MS,
        max_expiry_ms: MAX_EXPIRY_MS,
        parts: false,
    },
    KindSpec {
        name: SESSION,
        title: "Start or drive a session",
        answer_on: AnswerOn::Mac,
        applied_by: AppliedBy::Daemon,
        result: Outcome::Stored,
        item: ItemKind::Consent,
        default_expiry_ms: DEFAULT_EXPIRY_MS,
        max_expiry_ms: MAX_EXPIRY_MS,
        parts: false,
    },
    KindSpec {
        name: SESSION_STOP,
        title: "Stop a session",
        answer_on: AnswerOn::MacOrPhone,
        applied_by: AppliedBy::Daemon,
        result: Outcome::None,
        item: ItemKind::Consent,
        default_expiry_ms: DEFAULT_EXPIRY_MS,
        max_expiry_ms: MAX_EXPIRY_MS,
        parts: false,
    },
    KindSpec {
        name: DISPATCH,
        title: "Run a job on another machine",
        answer_on: AnswerOn::Mac,
        applied_by: AppliedBy::Daemon,
        result: Outcome::Stored,
        item: ItemKind::Consent,
        default_expiry_ms: DEFAULT_EXPIRY_MS,
        max_expiry_ms: MAX_EXPIRY_MS,
        parts: false,
    },
    KindSpec {
        name: JOB,
        title: "Read or return a job",
        answer_on: AnswerOn::Mac,
        applied_by: AppliedBy::Daemon,
        result: Outcome::Stored,
        item: ItemKind::Consent,
        default_expiry_ms: DEFAULT_EXPIRY_MS,
        max_expiry_ms: MAX_EXPIRY_MS,
        parts: false,
    },
    KindSpec {
        name: JOB_STOP,
        title: "Stop a job",
        answer_on: AnswerOn::MacOrPhone,
        applied_by: AppliedBy::Daemon,
        result: Outcome::None,
        item: ItemKind::Consent,
        default_expiry_ms: DEFAULT_EXPIRY_MS,
        max_expiry_ms: MAX_EXPIRY_MS,
        parts: false,
    },
    KindSpec {
        name: CLUSTER,
        title: "Change the cluster",
        answer_on: AnswerOn::Mac,
        applied_by: AppliedBy::App,
        result: Outcome::Handover,
        item: ItemKind::Consent,
        default_expiry_ms: DEFAULT_EXPIRY_MS,
        max_expiry_ms: MAX_EXPIRY_MS,
        parts: false,
    },
    KindSpec {
        name: ACCOUNT,
        title: "Change accounts",
        answer_on: AnswerOn::Mac,
        applied_by: AppliedBy::App,
        result: Outcome::Stored,
        item: ItemKind::Consent,
        default_expiry_ms: DEFAULT_EXPIRY_MS,
        max_expiry_ms: MAX_EXPIRY_MS,
        parts: false,
    },
    KindSpec {
        name: MACHINE,
        title: "Change a machine",
        answer_on: AnswerOn::Mac,
        applied_by: AppliedBy::App,
        result: Outcome::Stored,
        item: ItemKind::Consent,
        default_expiry_ms: DEFAULT_EXPIRY_MS,
        max_expiry_ms: MAX_EXPIRY_MS,
        parts: false,
    },
    KindSpec {
        name: UPDATE,
        title: "Update a tool",
        answer_on: AnswerOn::Mac,
        applied_by: AppliedBy::App,
        result: Outcome::Stored,
        item: ItemKind::Consent,
        default_expiry_ms: DEFAULT_EXPIRY_MS,
        max_expiry_ms: MAX_EXPIRY_MS,
        parts: false,
    },
    KindSpec {
        name: INSTALL,
        title: "Set up Notchling",
        answer_on: AnswerOn::Mac,
        applied_by: AppliedBy::App,
        result: Outcome::Stored,
        item: ItemKind::Consent,
        default_expiry_ms: DEFAULT_EXPIRY_MS,
        max_expiry_ms: MAX_EXPIRY_MS,
        parts: true,
    },
    KindSpec {
        name: COST_PROFILE,
        title: "Change the Cost Profile",
        answer_on: AnswerOn::Mac,
        applied_by: AppliedBy::App,
        result: Outcome::Stored,
        item: ItemKind::Consent,
        default_expiry_ms: DEFAULT_EXPIRY_MS,
        max_expiry_ms: MAX_EXPIRY_MS,
        parts: true,
    },
    KindSpec {
        name: MODEL,
        title: "Download a model",
        answer_on: AnswerOn::Mac,
        applied_by: AppliedBy::App,
        result: Outcome::Stored,
        item: ItemKind::Consent,
        default_expiry_ms: DEFAULT_EXPIRY_MS,
        max_expiry_ms: MAX_EXPIRY_MS,
        parts: false,
    },
];

pub fn spec(name: &str) -> Option<&'static KindSpec> {
    KINDS.iter().find(|k| k.name == name)
}
