use std::path::Path;

use notchling_engine::awake::{self, Mode};
use notchling_engine::settings::Settings;
use serde_json::{json, Value};

use crate::menu::{AwakeMode, Command, MenuState};

pub const MODE: &str = "mode";
pub const DISPLAY: &str = "keepDisplayOn";

fn engine(mode: AwakeMode) -> Mode {
    match mode {
        AwakeMode::Off => Mode::Off,
        AwakeMode::On => Mode::On,
        AwakeMode::WhileWorking => Mode::WhileWorking,
    }
}

fn menu(mode: Mode) -> AwakeMode {
    match mode {
        Mode::Off => AwakeMode::Off,
        Mode::On => AwakeMode::On,
        Mode::WhileWorking => AwakeMode::WhileWorking,
    }
}

pub fn word(mode: AwakeMode) -> &'static str {
    engine(mode).as_str()
}

pub fn from_word(text: &str) -> Option<AwakeMode> {
    Mode::parse(text).map(menu)
}

pub fn read(data_dir: &Path) -> MenuState {
    let setting = Settings::open(&data_dir.join("settings.json"))
        .map(|s| awake::read(&s))
        .unwrap_or_default();
    MenuState {
        awake: menu(setting.mode),
        keep_display_on: setting.display,
        ..MenuState::default()
    }
}

pub fn wanted(command: Command, state: &MenuState) -> Option<(&'static str, Value)> {
    match command {
        Command::SetAwake(mode) => Some((MODE, json!(word(mode)))),
        Command::ToggleDisplay => Some((DISPLAY, json!(!state.keep_display_on))),
        _ => None,
    }
}

pub fn with(state: &MenuState, key: &str, value: &Value) -> Option<MenuState> {
    let next = match key {
        MODE => MenuState {
            awake: from_word(value.as_str()?)?,
            ..state.clone()
        },
        DISPLAY => MenuState {
            keep_display_on: value.as_bool()?,
            ..state.clone()
        },
        _ => return None,
    };
    (next != *state).then_some(next)
}

pub fn write(data_dir: &Path, key: &str, value: &Value) -> Result<(), String> {
    let mut store = Settings::open(&data_dir.join("settings.json")).map_err(|e| e.to_string())?;
    store.set(key, value.clone()).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "keep_awake_tests.rs"]
mod tests;
