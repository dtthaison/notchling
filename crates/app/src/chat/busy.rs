use notchling_engine::sessions::jobs::parse_object;
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use serde_json::Value;

use super::outbox::{Bubble, Status};

pub const COMMAND_KIND: &str = "local_bash";
pub const READ_AGAIN_MS: i64 = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Busy {
    Tasks { count: u64, commands: bool },
    Replying,
    Unconfirmed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InFlight {
    pub tasks: u64,
    pub commands: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BusyFacts {
    pub working: bool,
    pub at_prompt: bool,
    pub tasks: u64,
    pub commands: bool,
}

pub fn in_flight(job: &Value) -> InFlight {
    let Some(flight) = job.get("inFlight").filter(|f| f.is_object()) else {
        return InFlight::default();
    };
    let tasks = flight.get("tasks").and_then(Value::as_u64).unwrap_or(0);
    let kinds: Vec<&str> = flight
        .get("kinds")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    InFlight {
        tasks,
        commands: !kinds.is_empty() && kinds.iter().all(|kind| *kind == COMMAND_KIND),
    }
}

pub fn read_in_flight(file: &std::path::Path) -> InFlight {
    std::fs::read(file)
        .ok()
        .and_then(|data| parse_object(&data))
        .map(|job| in_flight(&job))
        .unwrap_or_default()
}

pub fn facts(agent: &Agent, flight: InFlight) -> BusyFacts {
    BusyFacts {
        working: agent.group == AgentGroup::Working,
        at_prompt: agent.at_prompt,
        tasks: flight.tasks,
        commands: flight.commands,
    }
}

pub fn why_busy(facts: &BusyFacts) -> Option<Busy> {
    if facts.working && facts.tasks > 0 {
        return Some(Busy::Tasks {
            count: facts.tasks,
            commands: facts.commands,
        });
    }
    (facts.working && !facts.at_prompt).then_some(Busy::Replying)
}

fn holds_the_line(status: &Status) -> Option<bool> {
    match status {
        Status::MayBeIn | Status::MayBeInHolding(_) | Status::AnswerUnseen => Some(true),
        Status::BoxHasText(_)
        | Status::BoxHoldsThis
        | Status::Stopped
        | Status::NotFound
        | Status::QueuedInSession(_)
        | Status::Editing
        | Status::NeedsAnswer => Some(false),
        _ => None,
    }
}

pub fn explain(bubbles: &mut [Bubble], facts: &BusyFacts) {
    let mut first_blocker_unsure: Option<bool> = None;
    for bubble in bubbles {
        if let Some(unsure) = holds_the_line(&bubble.status) {
            first_blocker_unsure.get_or_insert(unsure);
        }
        let why = match bubble.status {
            Status::Queued | Status::WaitsForTurn => why_busy(facts),
            Status::WaitsForPrevious if first_blocker_unsure == Some(true) => {
                Some(Busy::Unconfirmed)
            }
            _ => None,
        };
        if let Some(why) = why {
            bubble.status = Status::Busy(why);
        }
    }
}

pub fn job_of(agent: &Agent) -> Option<&str> {
    match &agent.target {
        AgentTarget::Job(job) if agent.can_reply() && agent.node.is_none() => Some(job),
        _ => None,
    }
}

pub fn wants_read(agent: &Agent, bubbles: &[Bubble]) -> bool {
    job_of(agent).is_some()
        && agent.group == AgentGroup::Working
        && bubbles
            .iter()
            .any(|b| matches!(b.status, Status::Queued | Status::WaitsForTurn))
}

#[derive(Debug, Default)]
pub struct Reading {
    job: Option<String>,
    flight: InFlight,
    asked: Option<String>,
    read_ms: i64,
}

impl Reading {
    pub fn key_of(agent: &Agent) -> Option<String> {
        job_of(agent).map(|job| format!("{job}|{}", agent.since_ms))
    }

    pub fn due(&self, key: &str, now_ms: i64) -> bool {
        self.asked.as_deref() != Some(key) || now_ms - self.read_ms >= READ_AGAIN_MS
    }

    pub fn begin(&mut self, key: &str, now_ms: i64) {
        self.asked = Some(key.to_owned());
        self.read_ms = now_ms;
    }

    pub fn finish(&mut self, key: &str, flight: InFlight) {
        if self.asked.as_deref() == Some(key) {
            self.job = key.split('|').next().map(str::to_owned);
            self.flight = flight;
        }
    }

    pub fn of(&self, agent: &Agent) -> InFlight {
        match (job_of(agent), self.job.as_deref()) {
            (Some(now), Some(read)) if now == read => self.flight,
            _ => InFlight::default(),
        }
    }
}

#[cfg(test)]
#[path = "busy_tests.rs"]
mod tests;
