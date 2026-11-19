use std::collections::{HashMap, HashSet};

use notchling_engine::chrome::flow::{Done, Need, Phase, Step, Wait};
use notchling_engine::control::slash::SlashCommand;
use notchling_engine::sessions::AgentTarget;
use notchling_locale as t;
use serde_json::Value;

use crate::chat::data::{Session, Tool};

pub const POLL_MS: i64 = 1_000;
pub const NOTE_MS: i64 = 6_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub on: bool,
    pub phase: Phase,
    pub since_ms: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub offered: bool,
    pub on: Option<bool>,
    pub task: Option<Task>,
}

pub fn parse(value: &Value) -> Option<Status> {
    let offered = value.get("offered").and_then(Value::as_bool)?;
    let on = value.pointer("/detected/on").and_then(Value::as_bool);
    let task = match value.get("task") {
        None | Some(Value::Null) => None,
        Some(task) => Some(Task {
            on: task.get("on").and_then(Value::as_bool).unwrap_or(true),
            phase: serde_json::from_value(task.clone()).ok()?,
            since_ms: task.get("since_ms").and_then(Value::as_i64).unwrap_or(0),
        }),
    };
    Some(Status { offered, on, task })
}

pub fn offered(session: &Session) -> bool {
    session.tool == Tool::Claude
        && matches!(session.agent.target, AgentTarget::Job(_))
        && session.agent.can_reply()
        && session.agent.node.is_none()
        && !session.agent.headless
}

impl Task {
    pub fn is_over(&self) -> bool {
        self.phase.is_over()
    }

    pub fn needs(&self) -> Option<&Need> {
        match &self.phase {
            Phase::NeedsYou { need } => Some(need),
            _ => None,
        }
    }

    pub fn note(&self) -> Option<String> {
        let text = match &self.phase {
            Phase::Waiting { why } => match why {
                Wait::Turn => t::chat::chrome_wait_turn(),
                Wait::Box => t::chat::chrome_wait_box(),
                Wait::Question => t::chat::chrome_wait_question(),
            },
            Phase::Working { step } => match (step, self.on) {
                (Step::Checking, true) => t::chat::chrome_turning_on(),
                (Step::Checking, false) | (Step::Connecting, false) => {
                    t::chat::chrome_turning_off()
                }
                (Step::Restarting, true) => t::chat::chrome_restarting_on(),
                (Step::Restarting, false) => t::chat::chrome_restarting_off(),
                (Step::Connecting, true) => t::chat::chrome_connecting(),
            },
            Phase::Done { result } => match result {
                Done::AlreadyOn => t::chat::chrome_already_on(),
                Done::On => t::chat::chrome_is_on(),
                Done::Off => t::chat::chrome_is_off(),
                Done::AlreadyOff => t::chat::chrome_already_off(),
            },
            Phase::NeedsYou { .. } => t::chat::chrome_needs_you(),
            Phase::Cancelled => return None,
        };
        Some(text.to_owned())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    TryAgain,
    TurnOnAnyway,
    TurnOffAnyway,
    TurnOn,
    TurnOff,
    OpenTerminal,
}

impl Act {
    pub fn label(self) -> &'static str {
        match self {
            Act::TryAgain => t::chat::chrome_try_again(),
            Act::TurnOnAnyway => t::chat::chrome_turn_on_anyway(),
            Act::TurnOffAnyway => t::chat::chrome_turn_off_anyway(),
            Act::TurnOn => t::chat::chrome_turn_on(),
            Act::TurnOff => t::chat::chrome_turn_off(),
            Act::OpenTerminal => t::chat::chrome_open_terminal(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub session: String,
    pub on: Option<bool>,
    pub wanted_on: bool,
    pub need: Option<Need>,
}

impl Sheet {
    pub fn status(&self) -> &'static str {
        match self.on {
            Some(true) => t::chat::chrome_status_on(),
            Some(false) => t::chat::chrome_status_off(),
            None => t::chat::chrome_status_unknown(),
        }
    }

    pub fn problem(&self) -> Option<String> {
        let need = self.need.as_ref()?;
        Some(match need {
            Need::Tasks { count } => t::chat::chrome_tasks(*count),
            Need::TasksUnknown => t::chat::chrome_tasks_unknown().to_owned(),
            Need::NoExtension => t::chat::chrome_no_extension().to_owned(),
            Need::NotConnected => t::chat::chrome_not_connected().to_owned(),
            Need::TurnedOffInMcp => t::chat::chrome_mcp_off().to_owned(),
            Need::Unavailable => t::chat::chrome_unavailable().to_owned(),
            Need::Busy { why } => match why {
                Wait::Turn => t::chat::chrome_busy_turn().to_owned(),
                Wait::Box => t::chat::chrome_busy_box().to_owned(),
                Wait::Question => t::chat::chrome_busy_question().to_owned(),
            },
            Need::Failed { text } => t::chat::chrome_failed(text),
        })
    }

    pub fn acts(&self) -> Vec<Act> {
        match &self.need {
            Some(Need::Tasks { .. } | Need::TasksUnknown) if self.wanted_on => {
                vec![Act::TurnOnAnyway, Act::TryAgain]
            }
            Some(Need::Tasks { .. } | Need::TasksUnknown) => {
                vec![Act::TurnOffAnyway, Act::TryAgain]
            }
            Some(Need::TurnedOffInMcp) => vec![Act::OpenTerminal, Act::TryAgain],
            Some(Need::Failed { .. }) => vec![Act::TryAgain, Act::OpenTerminal],
            Some(_) => vec![Act::TryAgain],
            None if self.on == Some(true) => vec![Act::TurnOff],
            None => vec![Act::TurnOn],
        }
    }

    pub fn help(&self) -> [&'static str; 3] {
        [
            t::chat::chrome_help_install(),
            t::chat::chrome_help_account(),
            t::chat::chrome_help_profiles(),
        ]
    }
}

#[derive(Default)]
pub struct State {
    pub on: HashMap<String, bool>,
    pub tasks: HashMap<String, Task>,
    pub notes: HashMap<String, (String, i64)>,
    pub asking: HashSet<String>,
    pub peeked: HashSet<String>,
    pub last_poll_ms: i64,
}

impl State {
    pub fn apply(&mut self, id: &str, status: &Status, now_ms: i64) -> Applied {
        match status.on {
            Some(on) => {
                self.on.insert(id.to_owned(), on);
            }
            None => {
                self.on.remove(id);
            }
        }
        let Some(task) = &status.task else {
            self.tasks.remove(id);
            return Applied::Nothing;
        };
        self.tasks.insert(id.to_owned(), task.clone());
        match &task.phase {
            Phase::Cancelled => {
                self.tasks.remove(id);
                Applied::Finished
            }
            Phase::Done { result } => {
                self.on
                    .insert(id.to_owned(), matches!(result, Done::On | Done::AlreadyOn));
                if let Some(text) = task.note() {
                    self.notes.insert(id.to_owned(), (text, now_ms + NOTE_MS));
                }
                self.tasks.remove(id);
                Applied::Finished
            }
            Phase::NeedsYou { need } => {
                self.tasks.remove(id);
                self.notes.remove(id);
                let on = match (need, task.on) {
                    (Need::Busy { .. }, _) => status.on,
                    (Need::Tasks { .. }, wanted) => Some(!wanted),
                    (_, true) => Some(false),
                    (_, false) => status.on,
                };
                match on {
                    Some(on) => {
                        self.on.insert(id.to_owned(), on);
                    }
                    None => {
                        self.on.remove(id);
                    }
                }
                Applied::Needs(Sheet {
                    session: id.to_owned(),
                    on,
                    wanted_on: task.on,
                    need: Some(need.clone()),
                })
            }
            _ => Applied::Working,
        }
    }

    pub fn note(&self, id: &str, now_ms: i64) -> Option<String> {
        if let Some(task) = self.tasks.get(id) {
            return task.note();
        }
        self.notes
            .get(id)
            .filter(|(_, until)| *until > now_ms)
            .map(|(text, _)| text.clone())
    }

    pub fn active(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.tasks.keys().cloned().collect();
        ids.sort();
        ids
    }

    pub fn begin(&mut self, id: &str, on: bool, now_ms: i64) {
        self.notes.remove(id);
        self.tasks.insert(
            id.to_owned(),
            Task {
                on,
                phase: Phase::Working {
                    step: Step::Checking,
                },
                since_ms: now_ms,
            },
        );
    }

    pub fn due(&self, now_ms: i64) -> bool {
        !self.tasks.is_empty() && now_ms - self.last_poll_ms >= POLL_MS
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Applied {
    Nothing,
    Working,
    Finished,
    Needs(Sheet),
}

pub fn command(on: bool) -> &'static str {
    if on {
        "/chrome"
    } else {
        "/chrome off"
    }
}

pub const NAME_ON: &str = "chrome";
pub const NAME_OFF: &str = "chrome off";

pub fn suggestions() -> Vec<SlashCommand> {
    [
        (NAME_ON, t::chat::chrome_suggest_on()),
        (NAME_OFF, t::chat::chrome_suggest_off()),
    ]
    .into_iter()
    .map(|(name, description)| SlashCommand {
        name: name.to_owned(),
        description: description.to_owned(),
        argument_hint: None,
    })
    .collect()
}

pub fn commands_for(session: &Session, commands: &[SlashCommand]) -> Vec<SlashCommand> {
    let mut all: Vec<SlashCommand> = commands
        .iter()
        .filter(|c| c.name != NAME_ON)
        .cloned()
        .collect();
    if offered(session) {
        all.extend(suggestions());
    }
    all
}

pub fn spaced(text: &str) -> Vec<SlashCommand> {
    let Some(rest) = text.strip_prefix('/') else {
        return Vec::new();
    };
    let lower = rest.to_lowercase();
    let Some(after) = lower.strip_prefix(NAME_ON) else {
        return Vec::new();
    };
    if !after.starts_with(char::is_whitespace) {
        return Vec::new();
    }
    let arg = after.trim();
    let off = &NAME_OFF[NAME_ON.len() + 1..];
    if arg.is_empty() || arg == off || !off.starts_with(arg) {
        return Vec::new();
    }
    suggestions()
        .into_iter()
        .filter(|c| c.name == NAME_OFF)
        .collect()
}
