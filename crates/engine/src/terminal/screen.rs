use std::collections::VecDeque;

use super::width;

pub const SCROLLBACK: usize = 5000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TermColor {
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TermStyle {
    pub fg: Option<TermColor>,
    pub bg: Option<TermColor>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermCell {
    pub text: String,
    pub style: TermStyle,
    pub width: u8,
}

impl Default for TermCell {
    fn default() -> Self {
        TermCell {
            text: " ".to_string(),
            style: TermStyle::default(),
            width: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
    Ground,
    Escape,
    EscapeIntermediate,
    Csi,
    Str,
    StrEscape,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SavedCursor {
    pub row: usize,
    pub col: usize,
    pub style: TermStyle,
}

#[derive(Clone, Debug)]
pub struct TerminalScreen {
    pub(super) rows: usize,
    pub(super) cols: usize,
    pub(super) cells: Vec<Vec<TermCell>>,
    pub(super) cursor_row: usize,
    pub(super) cursor_col: usize,
    pub(super) cursor_visible: bool,
    pub(super) app_cursor_keys: bool,
    pub(super) bracketed_paste: bool,
    pub(super) mouse_tracking: bool,
    pub(super) sgr_mouse: bool,
    pub(super) replies: Vec<u8>,
    pub(super) style: TermStyle,
    pub(super) wrap_pending: bool,
    pub(super) top: usize,
    pub(super) bottom: usize,
    pub(super) saved_cursor: Option<SavedCursor>,
    pub(super) saved_main: Option<Vec<Vec<TermCell>>>,
    pub(super) scrollback: VecDeque<Vec<TermCell>>,
    pub(super) state: State,
    pub(super) params: Vec<u8>,
    utf8: Vec<u8>,
    utf8_need: usize,
    join_next: bool,
}

impl TerminalScreen {
    pub fn new(rows: usize, cols: usize) -> TerminalScreen {
        let rows = rows.max(1);
        let cols = cols.max(1);
        TerminalScreen {
            rows,
            cols,
            cells: vec![vec![TermCell::default(); cols]; rows],
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: true,
            app_cursor_keys: false,
            bracketed_paste: false,
            mouse_tracking: false,
            sgr_mouse: false,
            replies: Vec::new(),
            style: TermStyle::default(),
            wrap_pending: false,
            top: 0,
            bottom: rows - 1,
            saved_cursor: None,
            saved_main: None,
            scrollback: VecDeque::new(),
            state: State::Ground,
            params: Vec::new(),
            utf8: Vec::new(),
            utf8_need: 0,
            join_next: false,
        }
    }

    pub fn rows(&self) -> usize {
        self.rows
    }
    pub fn cols(&self) -> usize {
        self.cols
    }
    pub fn cells(&self) -> &[Vec<TermCell>] {
        &self.cells
    }
    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }
    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }
    pub fn app_cursor_keys(&self) -> bool {
        self.app_cursor_keys
    }
    pub fn bracketed_paste(&self) -> bool {
        self.bracketed_paste
    }
    pub fn mouse_tracking(&self) -> bool {
        self.mouse_tracking
    }
    pub fn sgr_mouse(&self) -> bool {
        self.sgr_mouse
    }
    pub fn scrollback(&self) -> &VecDeque<Vec<TermCell>> {
        &self.scrollback
    }
    pub fn alt_screen(&self) -> bool {
        self.saved_main.is_some()
    }

    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    pub fn lines(&self) -> Vec<String> {
        self.cells
            .iter()
            .map(|row| {
                let mut s: String = row.iter().map(|c| c.text.as_str()).collect();
                let kept = s.trim_end_matches(' ').len();
                s.truncate(kept);
                s
            })
            .collect()
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        let new_rows = rows.max(1);
        let new_cols = cols.max(1);
        if new_rows == self.rows && new_cols == self.cols {
            return;
        }
        let fit = |grid: &mut Vec<Vec<TermCell>>| {
            grid.truncate(new_rows);
            for row in grid.iter_mut() {
                row.resize(new_cols, TermCell::default());
            }
            grid.resize(new_rows, vec![TermCell::default(); new_cols]);
        };
        fit(&mut self.cells);
        if let Some(main) = self.saved_main.as_mut() {
            fit(main);
        }
        self.rows = new_rows;
        self.cols = new_cols;
        self.top = 0;
        self.bottom = new_rows - 1;
        self.cursor_row = self.cursor_row.min(new_rows - 1);
        self.cursor_col = self.cursor_col.min(new_cols - 1);
        self.wrap_pending = false;
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.step(b);
        }
    }

    fn step(&mut self, b: u8) {
        match self.state {
            State::Ground => self.ground(b),
            State::Escape => self.escape(b),
            State::EscapeIntermediate => self.state = State::Ground,
            State::Csi => match b {
                0x20..=0x3F => self.params.push(b),
                0x40..=0x7E => {
                    self.state = State::Ground;
                    self.csi(b);
                }
                0x1B => self.state = State::Escape,
                0x00..=0x1F => self.control(b),
                _ => {}
            },
            State::Str => {
                if b == 0x07 {
                    self.state = State::Ground;
                } else if b == 0x1B {
                    self.state = State::StrEscape;
                }
            }
            State::StrEscape => {
                self.state = if b == 0x5C { State::Ground } else { State::Str };
            }
        }
    }

    fn ground(&mut self, b: u8) {
        if self.utf8_need > 0 {
            if b & 0xC0 == 0x80 {
                self.utf8.push(b);
                self.utf8_need -= 1;
                if self.utf8_need == 0 {
                    if let Some(c) = std::str::from_utf8(&self.utf8)
                        .ok()
                        .and_then(|s| s.chars().next())
                    {
                        self.put(c);
                    }
                }
                return;
            }
            self.utf8_need = 0;
        }
        match b {
            0x00..=0x1F => self.control(b),
            0x7F | 0x80..=0xBF | 0xF8..=0xFF => {}
            0xC0..=0xDF => self.begin_utf8(b, 1),
            0xE0..=0xEF => self.begin_utf8(b, 2),
            0xF0..=0xF7 => self.begin_utf8(b, 3),
            _ => self.put(char::from(b)),
        }
    }

    fn begin_utf8(&mut self, b: u8, need: usize) {
        self.utf8.clear();
        self.utf8.push(b);
        self.utf8_need = need;
    }

    pub(super) fn control(&mut self, b: u8) {
        match b {
            0x08 => {
                self.wrap_pending = false;
                self.cursor_col = self.cursor_col.saturating_sub(1);
            }
            0x09 => self.cursor_col = (self.cols - 1).min((self.cursor_col / 8 + 1) * 8),
            0x0A..=0x0C => self.line_feed(),
            0x0D => {
                self.wrap_pending = false;
                self.cursor_col = 0;
            }
            0x1B => self.state = State::Escape,
            _ => {}
        }
    }

    fn escape(&mut self, b: u8) {
        self.state = State::Ground;
        match b {
            b'[' => {
                self.params.clear();
                self.state = State::Csi;
            }
            b']' | b'P' | b'_' | b'^' | b'X' => self.state = State::Str,
            b'(' | b')' | b'*' | b'+' | b'#' | b'%' => self.state = State::EscapeIntermediate,
            b'7' => self.save_cursor(),
            b'8' => self.restore_cursor(),
            b'D' => self.line_feed(),
            b'E' => {
                self.cursor_col = 0;
                self.line_feed();
            }
            b'M' => self.reverse_index(),
            b'c' => self.reset(),
            _ => {}
        }
    }

    fn put(&mut self, c: char) {
        if self.join_next || width::is_combining(c) {
            self.join_next = c == '\u{200D}';
            self.append_to_previous(c);
            return;
        }
        let wide = width::is_wide(c);
        if self.wrap_pending {
            self.wrap_pending = false;
            self.cursor_col = 0;
            self.line_feed();
        }
        if wide && self.cursor_col == self.cols - 1 {
            self.cells[self.cursor_row][self.cursor_col] = self.blank();
            self.cursor_col = 0;
            self.line_feed();
        }
        let (row, col) = (self.cursor_row, self.cursor_col);
        self.cells[row][col] = TermCell {
            text: c.to_string(),
            style: self.style,
            width: if wide { 2 } else { 1 },
        };
        if wide && col + 1 < self.cols {
            self.cells[row][col + 1] = TermCell {
                text: String::new(),
                style: self.style,
                width: 0,
            };
        }
        self.cursor_col += if wide { 2 } else { 1 };
        if self.cursor_col >= self.cols {
            self.cursor_col = self.cols - 1;
            self.wrap_pending = true;
        }
    }

    fn append_to_previous(&mut self, c: char) {
        let col = if self.wrap_pending {
            self.cursor_col
        } else {
            match self.cursor_col.checked_sub(1) {
                Some(col) => col,
                None => return,
            }
        };
        let row = &mut self.cells[self.cursor_row];
        let col = if row[col].width == 0 && col > 0 {
            col - 1
        } else {
            col
        };
        row[col].text.push(c);
    }

    pub(super) fn blank(&self) -> TermCell {
        TermCell {
            text: " ".to_string(),
            style: TermStyle {
                bg: self.style.bg,
                ..TermStyle::default()
            },
            width: 1,
        }
    }

    pub(super) fn blank_row(&self) -> Vec<TermCell> {
        vec![self.blank(); self.cols]
    }

    pub(super) fn line_feed(&mut self) {
        self.wrap_pending = false;
        if self.cursor_row == self.bottom {
            self.scroll_up(1);
        } else if self.cursor_row < self.rows - 1 {
            self.cursor_row += 1;
        }
    }

    fn reverse_index(&mut self) {
        self.wrap_pending = false;
        if self.cursor_row == self.top {
            self.scroll_down(1);
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
        }
    }

    pub(super) fn scroll_up(&mut self, n: usize) {
        let n = n.min(self.bottom - self.top + 1);
        if n == 0 {
            return;
        }
        let gone = self.cells.drain(self.top..self.top + n);
        if self.top == 0 && self.saved_main.is_none() {
            self.scrollback.extend(gone);
            let over = self.scrollback.len().saturating_sub(SCROLLBACK);
            self.scrollback.drain(..over);
        } else {
            drop(gone);
        }
        let at = self.bottom + 1 - n;
        let blank = self.blank_row();
        self.cells.splice(at..at, std::iter::repeat_n(blank, n));
    }

    pub(super) fn scroll_down(&mut self, n: usize) {
        let n = n.min(self.bottom - self.top + 1);
        if n == 0 {
            return;
        }
        self.cells.drain(self.bottom + 1 - n..=self.bottom);
        let blank = self.blank_row();
        self.cells
            .splice(self.top..self.top, std::iter::repeat_n(blank, n));
    }

    pub(super) fn save_cursor(&mut self) {
        self.saved_cursor = Some(SavedCursor {
            row: self.cursor_row,
            col: self.cursor_col,
            style: self.style,
        });
    }

    pub(super) fn restore_cursor(&mut self) {
        let Some(saved) = self.saved_cursor else {
            return;
        };
        self.cursor_row = saved.row.min(self.rows - 1);
        self.cursor_col = saved.col.min(self.cols - 1);
        self.style = saved.style;
        self.wrap_pending = false;
    }

    fn reset(&mut self) {
        self.style = TermStyle::default();
        self.cells = vec![vec![TermCell::default(); self.cols]; self.rows];
        self.saved_main = None;
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.top = 0;
        self.bottom = self.rows - 1;
        self.cursor_visible = true;
        self.wrap_pending = false;
    }
}

#[cfg(test)]
mod scrollback_tests {
    use super::*;

    fn text(row: &[TermCell]) -> String {
        row.iter()
            .map(|c| c.text.as_str())
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    #[test]
    fn rows_scrolled_off_the_top_are_kept_and_clear_drops_them() {
        let mut screen = TerminalScreen::new(2, 10);
        screen.feed(b"one\r\ntwo\r\nthree\r\nfour");
        let kept: Vec<String> = screen.scrollback().iter().map(|r| text(r)).collect();
        assert_eq!(kept, vec!["one", "two"]);
        screen.feed(b"\x1b[?1049hx\r\ny\r\nz\x1b[?1049l");
        assert_eq!(
            screen.scrollback().len(),
            2,
            "the alternate screen adds nothing"
        );
        screen.feed(b"\x1b[H\x1b[2J\x1b[3J");
        assert!(screen.scrollback().is_empty());
    }
}
