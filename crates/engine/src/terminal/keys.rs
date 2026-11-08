//! The bytes a key press or a click sends to the program, a port of `TerminalKeys` in
//! `TerminalView.swift`. Key codes are macOS virtual key codes (`NSEvent.keyCode`), since the terminal
//! view that sends them is the Mac's; another platform maps its keys onto these first.

/// One key press.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Press {
    pub key_code: u16,
    /// The characters the key types, with modifiers applied (`NSEvent.characters`).
    pub characters: String,
    pub shift: bool,
    pub control: bool,
    pub option: bool,
}

/// What to write for `key`, or `None` when it types nothing. `app_cursor_keys` is the screen's mode
/// (`CSI ? 1 h`): arrows send `ESC O` instead of `ESC [`.
pub fn bytes(key: &Press, app_cursor_keys: bool) -> Option<String> {
    let arrow = if app_cursor_keys { "\x1bO" } else { "\x1b[" };
    let fixed = match key.key_code {
        36 | 76 => Some("\r".to_string()),
        48 => Some(if key.shift { "\x1b[Z" } else { "\t" }.to_string()),
        51 => Some(if key.option { "\x1b\x7f" } else { "\x7f" }.to_string()),
        53 => Some("\x1b".to_string()),
        117 => Some("\x1b[3~".to_string()),
        123 => Some(if key.option {
            "\x1bb".to_string()
        } else {
            format!("{arrow}D")
        }),
        124 => Some(if key.option {
            "\x1bf".to_string()
        } else {
            format!("{arrow}C")
        }),
        125 => Some(format!("{arrow}B")),
        126 => Some(format!("{arrow}A")),
        115 => Some("\x1b[H".to_string()),
        119 => Some("\x1b[F".to_string()),
        116 => Some("\x1b[5~".to_string()),
        121 => Some("\x1b[6~".to_string()),
        _ => None,
    };
    if fixed.is_some() {
        return fixed;
    }
    if key.control {
        if let Some(c) = key.characters.to_lowercase().chars().next() {
            match c {
                'a'..='z' => return Some(char::from(c as u8 - 0x60).to_string()),
                ' ' => return Some("\0".to_string()),
                _ => {}
            }
        }
    }
    (!key.characters.is_empty()).then(|| key.characters.clone())
}

/// An SGR mouse report (`ESC [ < button ; col ; row M`), from 0-based cells; `m` on release.
pub fn mouse(button: u32, col: usize, row: usize, released: bool) -> String {
    let end = if released { 'm' } else { 'M' };
    format!("\x1b[<{button};{};{}{end}", col + 1, row + 1)
}
