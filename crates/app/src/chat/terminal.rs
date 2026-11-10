//! The Details Panel's terminal, the pure part: which bytes a GPUI key press sends (through the
//! engine's `terminal::keys`, which speaks macOS key codes), how many rows and columns fit, a cell's
//! color, and a screen row cut into runs of one style for drawing.

use notchling_engine::terminal::keys::{self, Press};
use notchling_engine::terminal::{TermCell, TermColor, TermStyle};

/// The terminal's type size and line height, in points (`metric::TERMINAL_FONT` is the sheet's).
pub const FONT: f32 = 12.0;
pub const LINE: f32 = 16.0;
/// One cell's width: Menlo's advance is 0.602 em.
pub const CELL: f32 = FONT * 0.602;

/// GPUI's key names onto the macOS key codes `keys::bytes` reads.
fn key_code(key: &str) -> u16 {
    match key {
        "enter" => 36,
        "tab" => 48,
        "backspace" => 51,
        "escape" => 53,
        "delete" => 117,
        "left" => 123,
        "right" => 124,
        "down" => 125,
        "up" => 126,
        "home" => 115,
        "end" => 119,
        "pageup" => 116,
        "pagedown" => 121,
        // Not one of the keys above: what it types decides.
        _ => u16::MAX,
    }
}

/// What a key press writes to the shell, or `None` when it types nothing. `key_char` is what the
/// key types with its modifiers (GPUI's `Keystroke::key_char`); with Control it is often missing,
/// so the key's own name stands in.
pub fn bytes(
    key: &str,
    key_char: Option<&str>,
    control: bool,
    option: bool,
    shift: bool,
    app_cursor_keys: bool,
) -> Option<String> {
    let characters = match key_char {
        Some(c) if !c.is_empty() => c.to_owned(),
        _ if key == "space" => " ".to_owned(),
        _ if key.chars().count() == 1 => key.to_owned(),
        _ => String::new(),
    };
    let press = Press {
        key_code: key_code(key),
        characters,
        shift,
        control,
        option,
    };
    keys::bytes(&press, app_cursor_keys)
}

/// Rows and columns that fit `width` × `height` points, at least 2 × 10.
pub fn grid(width: f32, height: f32) -> (u16, u16) {
    let cols = (width / CELL).floor().clamp(10.0, 500.0) as u16;
    let rows = (height / LINE).floor().clamp(2.0, 300.0) as u16;
    (rows, cols)
}

/// The 16 standard colors as a dark terminal draws them (Terminal.app's Basic in dark mode).
const BASIC: [u32; 16] = [
    0x000000, 0xc23621, 0x25bc24, 0xadad27, 0x492ee1, 0xd338d3, 0x33bbc8, 0xcbcccd, 0x818383,
    0xfc391f, 0x31e722, 0xeaec23, 0x5833ff, 0xf935f8, 0x14f0f0, 0xe9ebeb,
];

/// A cell color as 0xRRGGBB: the 16 standard ones, the 6×6×6 cube, the grays, or 24-bit.
pub fn rgb(color: TermColor) -> u32 {
    match color {
        TermColor::Rgb(r, g, b) => (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
        TermColor::Indexed(i) if i < 16 => BASIC[usize::from(i)],
        TermColor::Indexed(i) if i < 232 => {
            let i = u32::from(i) - 16;
            let level = |v: u32| if v == 0 { 0 } else { 55 + v * 40 };
            (level(i / 36) << 16) | (level((i / 6) % 6) << 8) | level(i % 6)
        }
        TermColor::Indexed(i) => {
            let v = 8 + (u32::from(i) - 232) * 10;
            (v << 16) | (v << 8) | v
        }
    }
}

/// A row as runs of one style, for drawing; the right half of a wide character adds nothing.
pub fn runs(row: &[TermCell]) -> Vec<(String, TermStyle)> {
    let mut out: Vec<(String, TermStyle)> = Vec::new();
    for cell in row.iter().filter(|c| c.width != 0) {
        match out.last_mut() {
            Some((text, style)) if *style == cell.style => text.push_str(&cell.text),
            _ => out.push((cell.text.clone(), cell.style)),
        }
    }
    out
}

/// The text to write for a paste: wrapped in the bracketed-paste marks when the shell asked for
/// them, so a pasted newline doesn't run half a command.
pub fn paste(text: &str, bracketed: bool) -> String {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        format!("\x1b[200~{text}\x1b[201~")
    } else {
        text
    }
}

/// The terminal's tabs: each a shell, named "Terminal N" in the order they opened, one of them on
/// show. `T` is the shell, so the order and the numbering are tested without one.
#[derive(Debug)]
pub struct Tabs<T> {
    tabs: Vec<(u32, T)>,
    active: usize,
    next: u32,
}

impl<T> Default for Tabs<T> {
    fn default() -> Self {
        Tabs {
            tabs: Vec::new(),
            active: 0,
            next: 1,
        }
    }
}

impl<T> Tabs<T> {
    /// A new tab after the others, on show.
    pub fn add(&mut self, tab: T) {
        self.tabs.push((self.next, tab));
        self.next += 1;
        self.active = self.tabs.len() - 1;
    }

    /// Closes tab `i`; the one after it shows (or before, for the last), and the numbers restart
    /// once none is left.
    pub fn close(&mut self, i: usize) -> Option<T> {
        if i >= self.tabs.len() {
            return None;
        }
        let (_, tab) = self.tabs.remove(i);
        if self.tabs.is_empty() {
            self.active = 0;
            self.next = 1;
        } else if self.active > i || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        Some(tab)
    }

    pub fn select(&mut self, i: usize) {
        if i < self.tabs.len() {
            self.active = i;
        }
    }

    pub fn active(&self) -> Option<&T> {
        self.tabs.get(self.active).map(|(_, t)| t)
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Each tab's title ("Terminal 2") and shell, in order.
    pub fn iter(&self) -> impl Iterator<Item = (String, &T)> {
        self.tabs.iter().map(|(n, t)| (format!("Terminal {n}"), t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_are_numbered_in_order_and_the_new_one_shows() {
        let mut tabs = Tabs::default();
        tabs.add("a");
        tabs.add("b");
        tabs.add("c");
        let titles: Vec<String> = tabs.iter().map(|(t, _)| t).collect();
        assert_eq!(titles, ["Terminal 1", "Terminal 2", "Terminal 3"]);
        assert_eq!(tabs.active(), Some(&"c"));
    }

    #[test]
    fn closing_a_tab_shows_its_neighbour_and_keeps_the_others_numbers() {
        let mut tabs = Tabs::default();
        tabs.add("a");
        tabs.add("b");
        tabs.add("c");
        tabs.select(1);
        assert_eq!(tabs.close(1), Some("b"));
        assert_eq!(tabs.active(), Some(&"c"));
        assert_eq!(tabs.close(1), Some("c"));
        assert_eq!(tabs.active(), Some(&"a"));
        tabs.add("d");
        assert_eq!(
            tabs.iter().last().map(|(t, _)| t).as_deref(),
            Some("Terminal 4")
        );
    }

    #[test]
    fn closing_a_tab_before_the_one_on_show_keeps_it_on_show() {
        let mut tabs = Tabs::default();
        tabs.add("a");
        tabs.add("b");
        tabs.close(0);
        assert_eq!(tabs.active(), Some(&"b"));
        tabs.close(0);
        assert!(tabs.is_empty() && tabs.active().is_none());
        tabs.add("e");
        assert_eq!(
            tabs.iter().next().map(|(t, _)| t).as_deref(),
            Some("Terminal 1")
        );
    }

    #[test]
    fn keys_send_what_a_terminal_sends() {
        assert_eq!(
            bytes("enter", None, false, false, false, false).as_deref(),
            Some("\r")
        );
        assert_eq!(
            bytes("backspace", None, false, false, false, false).as_deref(),
            Some("\x7f")
        );
        assert_eq!(
            bytes("up", None, false, false, false, false).as_deref(),
            Some("\x1b[A")
        );
        assert_eq!(
            bytes("up", None, false, false, false, true).as_deref(),
            Some("\x1bOA")
        );
        assert_eq!(
            bytes("a", Some("a"), false, false, false, false).as_deref(),
            Some("a")
        );
        assert_eq!(
            bytes("a", Some("A"), false, false, true, false).as_deref(),
            Some("A")
        );
        assert_eq!(
            bytes("space", None, false, false, false, false).as_deref(),
            Some(" ")
        );
    }

    #[test]
    fn control_letters_send_their_control_codes() {
        assert_eq!(
            bytes("c", None, true, false, false, false).as_deref(),
            Some("\x03")
        );
        assert_eq!(
            bytes("d", Some("d"), true, false, false, false).as_deref(),
            Some("\x04")
        );
    }

    #[test]
    fn a_modifier_alone_types_nothing() {
        assert_eq!(bytes("shift", None, false, false, true, false), None);
        assert_eq!(bytes("f5", None, false, false, false, false), None);
    }

    #[test]
    fn the_grid_fits_the_panel() {
        assert_eq!(grid(CELL * 80.0 + 3.0, LINE * 24.0 + 5.0), (24, 80));
        assert_eq!(grid(1.0, 1.0), (2, 10));
    }

    #[test]
    fn colors_follow_the_xterm_palette() {
        assert_eq!(rgb(TermColor::Rgb(1, 2, 3)), 0x010203);
        assert_eq!(rgb(TermColor::Indexed(1)), BASIC[1]);
        assert_eq!(rgb(TermColor::Indexed(16)), 0x000000);
        assert_eq!(rgb(TermColor::Indexed(231)), 0xffffff);
        assert_eq!(rgb(TermColor::Indexed(196)), 0xff0000);
        assert_eq!(rgb(TermColor::Indexed(232)), 0x080808);
        assert_eq!(rgb(TermColor::Indexed(255)), 0xeeeeee);
    }

    #[test]
    fn a_row_joins_cells_of_one_style() {
        let bold = TermStyle {
            bold: true,
            ..TermStyle::default()
        };
        let cell = |t: &str, style: TermStyle, width: u8| TermCell {
            text: t.into(),
            style,
            width,
        };
        let row = vec![
            cell("a", TermStyle::default(), 1),
            cell("b", TermStyle::default(), 1),
            cell("界", bold, 2),
            cell("", bold, 0),
            cell("c", bold, 1),
        ];
        assert_eq!(
            runs(&row),
            vec![
                ("ab".to_owned(), TermStyle::default()),
                ("界c".to_owned(), bold)
            ]
        );
    }

    #[test]
    fn a_paste_is_bracketed_when_asked() {
        assert_eq!(paste("ls\npwd", false), "ls\rpwd");
        assert_eq!(paste("ls", true), "\x1b[200~ls\x1b[201~");
    }
}
