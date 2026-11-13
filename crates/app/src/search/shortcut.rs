use std::path::Path;

use notchling_engine::settings::Settings;
use serde_json::Value;

pub const KEY: &str = "searchShortcut";
pub const OFF: &str = "off";
pub const IN_WINDOW_KEY: &str = "cmd-shift-p";
pub const IN_WINDOW_LABEL: &str = "⌘⇧P";

const CMD_KEY: u32 = 1 << 8;
const SHIFT_KEY: u32 = 1 << 9;
const OPTION_KEY: u32 = 1 << 11;
const CONTROL_KEY: u32 = 1 << 12;

const CODES: [(char, u32); 37] = [
    ('a', 0x00),
    ('s', 0x01),
    ('d', 0x02),
    ('f', 0x03),
    ('h', 0x04),
    ('g', 0x05),
    ('z', 0x06),
    ('x', 0x07),
    ('c', 0x08),
    ('v', 0x09),
    ('b', 0x0B),
    ('q', 0x0C),
    ('w', 0x0D),
    ('e', 0x0E),
    ('r', 0x0F),
    ('y', 0x10),
    ('t', 0x11),
    ('1', 0x12),
    ('2', 0x13),
    ('3', 0x14),
    ('4', 0x15),
    ('6', 0x16),
    ('5', 0x17),
    ('9', 0x19),
    ('7', 0x1A),
    ('8', 0x1C),
    ('0', 0x1D),
    ('o', 0x1F),
    ('u', 0x20),
    ('i', 0x22),
    ('p', 0x23),
    ('l', 0x25),
    ('j', 0x26),
    ('k', 0x28),
    ('n', 0x2D),
    ('m', 0x2E),
    (' ', 0x31),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub mods: Mods,
    pub key: char,
}

pub const DEFAULT: Shortcut = Shortcut {
    mods: Mods {
        ctrl: true,
        alt: true,
        shift: false,
        cmd: false,
    },
    key: 'p',
};

fn key_name(key: char) -> String {
    if key == ' ' {
        "space".to_owned()
    } else {
        key.to_string()
    }
}

fn key_from_name(name: &str) -> Option<char> {
    let key = if name == "space" {
        ' '
    } else {
        let mut chars = name.chars();
        let one = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        one
    };
    CODES.iter().any(|(k, _)| *k == key).then_some(key)
}

impl Shortcut {
    pub fn parse(text: &str) -> Option<Shortcut> {
        let lowered = text.trim().to_lowercase();
        let mut parts: Vec<&str> = lowered.split('+').map(str::trim).collect();
        let key = key_from_name(parts.pop()?)?;
        let mut mods = Mods::default();
        for part in parts {
            match part {
                "ctrl" | "control" => mods.ctrl = true,
                "alt" | "option" | "opt" => mods.alt = true,
                "shift" => mods.shift = true,
                "cmd" | "command" => mods.cmd = true,
                _ => return None,
            }
        }
        Some(Shortcut { mods, key })
    }

    pub fn from_keystroke(key: &str, mods: Mods) -> Option<Shortcut> {
        Some(Shortcut {
            mods,
            key: key_from_name(&key.to_lowercase())?,
        })
    }

    pub fn value(&self) -> String {
        let mut out = String::new();
        for (on, name) in [
            (self.mods.ctrl, "ctrl"),
            (self.mods.alt, "alt"),
            (self.mods.shift, "shift"),
            (self.mods.cmd, "cmd"),
        ] {
            if on {
                out.push_str(name);
                out.push('+');
            }
        }
        out.push_str(&key_name(self.key));
        out
    }

    pub fn label(&self) -> String {
        let mut out = String::new();
        for (on, glyph) in [
            (self.mods.ctrl, '⌃'),
            (self.mods.alt, '⌥'),
            (self.mods.shift, '⇧'),
            (self.mods.cmd, '⌘'),
        ] {
            if on {
                out.push(glyph);
            }
        }
        if self.key == ' ' {
            out.push_str("Space");
        } else {
            out.push(self.key.to_ascii_uppercase());
        }
        out
    }

    pub fn problem(&self) -> Option<&'static str> {
        let m = self.mods;
        (!(m.ctrl || m.alt || m.cmd)).then_some("Add Control, Option or Command to the key.")
    }

    pub fn clash(&self) -> Option<&'static str> {
        let m = self.mods;
        (m.cmd && !m.ctrl && !m.alt && self.key.is_ascii_alphabetic())
            .then_some("Most apps use ⌘ with a letter, so this may stop one of their keys working.")
    }

    pub fn carbon(&self) -> Option<(u32, u32)> {
        let code = CODES.iter().find(|(k, _)| *k == self.key)?.1;
        let mut modifiers = 0;
        for (on, bit) in [
            (self.mods.ctrl, CONTROL_KEY),
            (self.mods.alt, OPTION_KEY),
            (self.mods.shift, SHIFT_KEY),
            (self.mods.cmd, CMD_KEY),
        ] {
            if on {
                modifiers |= bit;
            }
        }
        Some((code, modifiers))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Off,
    On(Shortcut),
}

impl Setting {
    pub const DEFAULT: Setting = Setting::On(DEFAULT);

    pub fn parse(text: &str) -> Option<Setting> {
        if text.trim().eq_ignore_ascii_case(OFF) {
            return Some(Setting::Off);
        }
        Shortcut::parse(text).map(Setting::On)
    }

    pub fn from_value(value: &Value) -> Setting {
        value
            .as_str()
            .and_then(Setting::parse)
            .unwrap_or(Setting::DEFAULT)
    }

    pub fn value(&self) -> String {
        match self {
            Setting::Off => OFF.to_owned(),
            Setting::On(shortcut) => shortcut.value(),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Setting::Off => "Off".to_owned(),
            Setting::On(shortcut) => shortcut.label(),
        }
    }
}

pub fn from_settings(settings: &Settings) -> Setting {
    settings
        .get_str(KEY)
        .as_deref()
        .and_then(Setting::parse)
        .unwrap_or(Setting::DEFAULT)
}

pub fn changed_to(key: &str, value: &Value, active: Setting) -> Option<Setting> {
    (key == KEY).then_some(())?;
    Some(Setting::from_value(value)).filter(|wanted| *wanted != active)
}

pub fn refusal(error: &str, before: Setting) -> String {
    match before {
        Setting::On(old) => format!("{error} {} stays.", old.label()),
        Setting::Off => error.to_owned(),
    }
}

pub fn read(data_dir: &Path) -> Setting {
    Settings::open(&data_dir.join("settings.json"))
        .map(|s| from_settings(&s))
        .unwrap_or(Setting::DEFAULT)
}

#[cfg(test)]
#[path = "shortcut_tests.rs"]
mod tests;
