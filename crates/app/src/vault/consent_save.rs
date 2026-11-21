use notchling_local_api::ApiError;
use notchling_locale as t;

use crate::switches::VAULT_OFF;

const CHANGED: &str = "That request no longer matches what you were shown.";
const NO_SUCH: &str = "No such request.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Expired,
    Changed,
    VaultOff,
    Withdrawn,
}

impl Refusal {
    pub fn line(self) -> &'static str {
        match self {
            Refusal::Expired => t::consent::card_form_expired(),
            Refusal::Changed => t::consent::card_form_changed(),
            Refusal::VaultOff => t::consent::card_form_vault_off(),
            Refusal::Withdrawn => t::consent::card_form_withdrawn(),
        }
    }

    pub fn final_for_request(self) -> bool {
        self != Refusal::VaultOff
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Refused(Refusal),
    Other(String),
}

impl Failure {
    pub fn line(&self) -> String {
        match self {
            Failure::Refused(refusal) => refusal.line().to_owned(),
            Failure::Other(message) => message.clone(),
        }
    }
}

fn needs_consent(e: &ApiError) -> bool {
    e.data
        .as_ref()
        .and_then(|d| d.get("reason"))
        .and_then(|r| r.as_str())
        == Some("needs_consent")
}

pub fn wants_state(e: &ApiError) -> bool {
    needs_consent(e) || e.message == NO_SUCH
}

pub fn refusal(e: &ApiError, state: Option<&str>) -> Option<Refusal> {
    if e.message == VAULT_OFF {
        return Some(Refusal::VaultOff);
    }
    if e.message == NO_SUCH {
        return Some(Refusal::Withdrawn);
    }
    if !needs_consent(e) {
        return None;
    }
    let open = matches!(state, None | Some("waiting" | "opened"));
    Some(match state {
        Some("expired") => Refusal::Expired,
        _ if e.message == CHANGED && open => Refusal::Changed,
        _ => Refusal::Withdrawn,
    })
}

pub fn failure(e: &ApiError, state: Option<&str>) -> Failure {
    match refusal(e, state) {
        Some(refusal) => Failure::Refused(refusal),
        None => Failure::Other(e.message.clone()),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Flow {
    #[default]
    Editing,
    Saving,
    Saved,
    Refused(Refusal),
    Failed(String),
}

impl Flow {
    pub fn can_save(&self) -> bool {
        match self {
            Flow::Editing | Flow::Failed(_) => true,
            Flow::Refused(refusal) => !refusal.final_for_request(),
            Flow::Saving | Flow::Saved => false,
        }
    }

    pub fn begin(&mut self) -> bool {
        if !self.can_save() {
            return false;
        }
        *self = Flow::Saving;
        true
    }

    pub fn settle(&mut self, result: Result<(), Failure>) {
        *self = match result {
            Ok(()) => Flow::Saved,
            Err(Failure::Refused(refusal)) => Flow::Refused(refusal),
            Err(Failure::Other(message)) => Flow::Failed(message),
        };
    }

    pub fn line(&self) -> Option<String> {
        match self {
            Flow::Refused(refusal) => Some(refusal.line().to_owned()),
            Flow::Failed(message) => Some(message.clone()),
            _ => None,
        }
    }

    pub fn closes(&self) -> bool {
        *self == Flow::Saved
    }
}

#[cfg(test)]
#[path = "consent_save_tests.rs"]
mod tests;
