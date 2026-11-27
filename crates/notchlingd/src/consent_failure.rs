use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};

const FAILURE_MAX: usize = 400;
const CODE_MAX: usize = 40;

const KNOWN: [(&str, &str); 13] = [
    ("step_failed", "A setup step didn't finish."),
    (
        "worker_claude_signed_out",
        "Claude Code isn't signed in on that machine. Sign in there, then dispatch again.",
    ),
    ("busy", "That machine is busy with another change."),
    ("refused", "The machine refused it."),
    ("not_sent", "It wasn't sent to the machine."),
    ("stale_term", "The cluster changed while it ran. Try again."),
    ("no_job", "No such job."),
    ("job_ended", "That job already ended."),
    (
        "not_yet",
        "Only a shell job on this Mac can be stopped yet.",
    ),
    ("no_machine", "No such machine in the cluster."),
    ("no_secret", "No such secret."),
    (
        "key_in_use",
        "That key already started another job. Use a new key.",
    ),
    (
        "vault_entry",
        "That isn't the Vault entry saved for this machine's admin password.",
    ),
];

const BAD_REQUEST: &str = "The request has a problem.";
const NOT_ALLOWED: &str = "It isn't allowed now.";
const NOT_AVAILABLE: &str = "That isn't available.";
const NOT_DONE: &str = "It didn't go through.";

pub(crate) fn is_known_line(text: &str) -> bool {
    KNOWN.iter().any(|(_, words)| *words == text)
        || [BAD_REQUEST, NOT_ALLOWED, NOT_AVAILABLE, NOT_DONE].contains(&text)
}

fn plain(word: &str) -> bool {
    word.len() <= CODE_MAX
        && word.starts_with(|c: char| c.is_ascii_lowercase())
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn words(code: &str, class: ErrorCode) -> &'static str {
    if let Some((_, words)) = KNOWN.iter().find(|(known, _)| *known == code) {
        return words;
    }
    match class {
        ErrorCode::BadRequest => BAD_REQUEST,
        ErrorCode::Forbidden | ErrorCode::NeedsApproval => NOT_ALLOWED,
        ErrorCode::UnknownOp | ErrorCode::Unimplemented => NOT_AVAILABLE,
        ErrorCode::Internal | ErrorCode::Failed => NOT_DONE,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub code: String,
    pub message: String,
    pub step: Option<String>,
}

impl Failure {
    pub fn new(code: &str, message: impl Into<String>) -> Failure {
        let message: String = message.into();
        Failure {
            code: code.to_string(),
            message: message.chars().take(FAILURE_MAX).collect(),
            step: None,
        }
    }

    pub fn of_api(e: &ApiError) -> Failure {
        let named = |key: &str| {
            e.data
                .as_ref()
                .and_then(|d| d.get(key))
                .and_then(Value::as_str)
                .filter(|code| plain(code))
        };
        let code = named("reason")
            .or_else(|| named("wire"))
            .unwrap_or(e.code.as_str());
        Failure {
            code: code.to_string(),
            message: words(code, e.code).to_string(),
            step: named("step").map(str::to_string),
        }
    }

    pub(crate) fn unspoken() -> Failure {
        Failure {
            code: String::new(),
            message: String::new(),
            step: None,
        }
    }

    pub(crate) fn stored(&self) -> Option<Value> {
        if self.code.is_empty() {
            return None;
        }
        let mut error = json!({"code": self.code, "message": self.message});
        if let Some(step) = &self.step {
            error["step"] = json!(step);
        }
        Some(json!({"error": error}))
    }
}

pub(crate) fn failure_reason(row: &notchling_engine::consent::Consent) -> Option<String> {
    row.result
        .as_ref()?
        .get("error")?
        .get("message")?
        .as_str()
        .map(str::to_string)
}

impl From<String> for Failure {
    fn from(message: String) -> Failure {
        Failure::new("failed", message)
    }
}
