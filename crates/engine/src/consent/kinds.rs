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
];

pub fn spec(name: &str) -> Option<&'static KindSpec> {
    KINDS.iter().find(|k| k.name == name)
}
