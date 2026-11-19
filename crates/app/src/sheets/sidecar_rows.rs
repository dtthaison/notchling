use notchling_locale as t;
use serde_json::Value;

pub const CHANNEL_KEY: &str = "antigravityChannel";
pub const CHANNEL: &str = "sidecar";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    NoApp,
    Off,
    Partial,
    On,
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub state: State,
    pub running: bool,
}

impl Shown {
    pub fn from_reply(reply: &Result<Value, String>) -> Option<Shown> {
        let reply = reply.as_ref().ok()?;
        let state = match reply.get("state")?.as_str()? {
            "no_app" => State::NoApp,
            "off" => State::Off,
            "partial" => State::Partial,
            "on" => State::On,
            "blocked" => State::Blocked,
            _ => return None,
        };
        Some(Shown {
            state,
            running: reply.get("running").and_then(Value::as_bool) == Some(true),
        })
    }

    pub fn word(&self) -> &'static str {
        match self.state {
            State::NoApp => t::settings::sidecar_state_no_app(),
            State::Off => t::settings::sidecar_state_off(),
            State::Partial => t::settings::sidecar_state_partial(),
            State::Blocked => t::settings::sidecar_state_blocked(),
            State::On if self.running => t::settings::sidecar_state_on(),
            State::On => t::settings::sidecar_state_waiting(),
        }
    }

    pub fn can_turn_on(&self) -> bool {
        matches!(self.state, State::Off | State::Partial)
    }

    pub fn can_turn_off(&self) -> bool {
        matches!(self.state, State::On | State::Partial)
    }
}

pub fn checking() -> &'static str {
    t::settings::sidecar_state_checking()
}

pub fn visible(chosen: bool, shown: Option<&Shown>) -> bool {
    chosen || shown.is_some_and(|s| s.can_turn_off())
}

pub fn is_chosen(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str) == Some(CHANNEL)
}

pub fn outcome(reply: &Result<Value, String>) -> (String, bool) {
    match reply {
        Ok(_) => (t::settings::sidecar_done().to_owned(), false),
        Err(why) => (why.clone(), true),
    }
}

#[cfg(test)]
#[path = "sidecar_rows_tests.rs"]
mod tests;
