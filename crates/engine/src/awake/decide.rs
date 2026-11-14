use crate::sessions::{Agent, AgentGroup};
use crate::settings::Settings;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Off,
    On,
    WhileWorking,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Off, Mode::On, Mode::WhileWorking];

    pub fn parse(text: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.as_str() == text)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::On => "on",
            Mode::WhileWorking => "whileWorking",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Setting {
    pub mode: Mode,
    pub display: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    System,
    Display,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    pub codex_turns: usize,
    pub agy_sessions: usize,
}

pub fn busy(agents: &[Agent], work: &Work) -> bool {
    agents.iter().any(|a| a.group == AgentGroup::Working)
        || work.codex_turns > 0
        || work.agy_sessions > 0
}

pub fn want(setting: Setting, busy: impl FnOnce() -> bool) -> Option<Kind> {
    let hold = match setting.mode {
        Mode::Off => false,
        Mode::On => true,
        Mode::WhileWorking => busy(),
    };
    hold.then_some(if setting.display {
        Kind::Display
    } else {
        Kind::System
    })
}

pub fn read(settings: &Settings) -> Setting {
    Setting {
        mode: settings
            .get_str("mode")
            .and_then(|text| Mode::parse(&text))
            .unwrap_or_default(),
        display: settings.get_bool("keepDisplayOn").unwrap_or(false),
    }
}
