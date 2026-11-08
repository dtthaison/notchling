//! Control sequences (`ESC [ … final`) for [`TerminalScreen`]: cursor moves, erases, scroll regions,
//! modes and colors. Numbers are clamped rather than trusted, since a program can send any digits.

use super::screen::{TermColor, TermStyle, TerminalScreen};

/// The parameters of one sequence: `None` where a field was empty or not a number.
type Numbers = Vec<Option<i64>>;

fn numbers(raw: &[u8]) -> Numbers {
    let text: String = raw
        .iter()
        .filter(|&&b| b != b' ')
        .map(|&b| if b == b':' { ';' } else { char::from(b) })
        .collect();
    text.split(';')
        .map(|part| part.parse::<i64>().ok())
        .collect()
}

fn arg(numbers: &Numbers, i: usize, fallback: i64) -> i64 {
    match numbers.get(i).copied().flatten() {
        Some(v) if v > 0 => v,
        _ => fallback,
    }
}

fn clamp_u8(v: i64) -> u8 {
    v.clamp(0, 255) as u8
}

fn to_i64(v: usize) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// `n` as a count no bigger than `limit`.
fn count(n: i64, limit: usize) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX).min(limit)
}

impl TerminalScreen {
    pub(super) fn csi(&mut self, final_byte: u8) {
        let mut raw = std::mem::take(&mut self.params);
        let prefix = match raw.first() {
            Some(&first) if (0x3C..=0x3F).contains(&first) => Some(raw.remove(0)),
            _ => None,
        };
        let numbers = numbers(&raw);
        let n = arg(&numbers, 0, 1);

        if prefix == Some(b'?') {
            if final_byte == b'h' || final_byte == b'l' {
                let modes: Vec<i64> = numbers.iter().flatten().copied().collect();
                self.set_modes(&modes, final_byte == b'h');
            }
            return;
        }
        // Device-attribute queries (CSI c, CSI > c, CSI > q) go unanswered, as in a plain pty: answered,
        // Claude Code 2.1.318 dropped its colors and its line layout, and the answer landed in its input box.
        if prefix.is_some() {
            return;
        }

        let (row, col) = (to_i64(self.cursor_row), to_i64(self.cursor_col));
        let (top, bottom) = (to_i64(self.top), to_i64(self.bottom));
        let last_row = to_i64(self.rows - 1);
        let first = numbers.first().copied().flatten().unwrap_or(0);
        match final_byte {
            b'A' => {
                let floor = if row >= top { top } else { 0 };
                self.move_to(floor.max(row.saturating_sub(n)), col);
            }
            b'B' | b'e' => {
                let ceiling = if row <= bottom { bottom } else { last_row };
                self.move_to(ceiling.min(row.saturating_add(n)), col);
            }
            b'C' | b'a' => self.move_to(row, col.saturating_add(n)),
            b'D' => self.move_to(row, col.saturating_sub(n)),
            b'E' => self.move_to(row.saturating_add(n), 0),
            b'F' => self.move_to(row.saturating_sub(n), 0),
            b'G' | b'`' => self.move_to(row, n - 1),
            b'd' => self.move_to(n - 1, col),
            b'H' | b'f' => self.move_to(arg(&numbers, 0, 1) - 1, arg(&numbers, 1, 1) - 1),
            b'J' => self.erase_display(first),
            b'K' => self.erase_line(first),
            b'X' => {
                let end = self.cols.min(self.cursor_col + count(n, self.cols));
                for c in self.cursor_col..end {
                    self.cells[self.cursor_row][c] = self.blank();
                }
            }
            b'P' => {
                let k = count(n, self.cols - self.cursor_col);
                let blank = self.blank();
                let line = &mut self.cells[self.cursor_row];
                line.drain(self.cursor_col..self.cursor_col + k);
                line.extend(std::iter::repeat_n(blank, k));
            }
            b'@' => {
                let k = count(n, self.cols - self.cursor_col);
                let blank = self.blank();
                let line = &mut self.cells[self.cursor_row];
                line.splice(
                    self.cursor_col..self.cursor_col,
                    std::iter::repeat_n(blank, k),
                );
                line.truncate(self.cols);
            }
            b'L' => {
                if !(self.top..=self.bottom).contains(&self.cursor_row) {
                    return;
                }
                let k = count(n, self.bottom - self.cursor_row + 1);
                self.cells.drain(self.bottom + 1 - k..=self.bottom);
                let blank = self.blank_row();
                let at = self.cursor_row;
                self.cells.splice(at..at, std::iter::repeat_n(blank, k));
            }
            b'M' => {
                if !(self.top..=self.bottom).contains(&self.cursor_row) {
                    return;
                }
                let k = count(n, self.bottom - self.cursor_row + 1);
                self.cells.drain(self.cursor_row..self.cursor_row + k);
                let blank = self.blank_row();
                let at = self.bottom + 1 - k;
                self.cells.splice(at..at, std::iter::repeat_n(blank, k));
            }
            b'S' => self.scroll_up(count(n, usize::MAX)),
            b'T' => self.scroll_down(count(n, usize::MAX)),
            b'm' => self.sgr(&numbers),
            b'r' => {
                let t = arg(&numbers, 0, 1) - 1;
                let b = arg(&numbers, 1, to_i64(self.rows)) - 1;
                if t >= b || b > last_row {
                    return;
                }
                self.top = count(t, usize::MAX);
                self.bottom = count(b, usize::MAX);
                self.move_to(0, 0);
            }
            b's' => self.save_cursor(),
            b'u' => self.restore_cursor(),
            b'n' => {
                if n == 6 {
                    let report = format!("\x1b[{};{}R", self.cursor_row + 1, self.cursor_col + 1);
                    self.replies.extend_from_slice(report.as_bytes());
                }
                if n == 5 {
                    self.replies.extend_from_slice(b"\x1b[0n");
                }
            }
            _ => {}
        }
    }

    fn move_to(&mut self, row: i64, col: i64) {
        self.wrap_pending = false;
        self.cursor_row = count(row.max(0), self.rows - 1);
        self.cursor_col = count(col.max(0), self.cols - 1);
    }

    fn erase_display(&mut self, mode: i64) {
        match mode {
            0 => {
                self.erase_line(0);
                for r in self.cursor_row + 1..self.rows {
                    self.cells[r] = self.blank_row();
                }
            }
            1 => {
                for r in 0..self.cursor_row {
                    self.cells[r] = self.blank_row();
                }
                self.erase_line(1);
            }
            _ => self.cells = vec![self.blank_row(); self.rows],
        }
    }

    fn erase_line(&mut self, mode: i64) {
        let range = match mode {
            0 => self.cursor_col..self.cols,
            1 => 0..self.cursor_col + 1,
            _ => 0..self.cols,
        };
        let blank = self.blank();
        for c in range {
            self.cells[self.cursor_row][c] = blank.clone();
        }
        self.wrap_pending = false;
    }

    fn set_modes(&mut self, modes: &[i64], on: bool) {
        for &mode in modes {
            match mode {
                1 => self.app_cursor_keys = on,
                25 => self.cursor_visible = on,
                2004 => self.bracketed_paste = on,
                1000 | 1002 | 1003 => self.mouse_tracking = on,
                1006 => self.sgr_mouse = on,
                47 | 1047 | 1049 => {
                    if on && self.saved_main.is_none() {
                        if mode == 1049 {
                            self.save_cursor();
                        }
                        let fresh = vec![vec![Default::default(); self.cols]; self.rows];
                        self.saved_main = Some(std::mem::replace(&mut self.cells, fresh));
                    } else if !on {
                        if let Some(main) = self.saved_main.take() {
                            self.cells = main;
                            if mode == 1049 {
                                self.restore_cursor();
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn sgr(&mut self, numbers: &Numbers) {
        let values: Vec<i64> = if numbers.is_empty() {
            vec![0]
        } else {
            numbers.iter().map(|v| v.unwrap_or(0)).collect()
        };
        let mut i = 0;
        // 38/48 then 5;n or 2;r;g;b. Anything else gives no color (and clears it, as in Swift).
        let color = |i: &mut usize| -> Option<TermColor> {
            if *i + 1 >= values.len() {
                return None;
            }
            if values[*i + 1] == 5 && *i + 2 < values.len() {
                let c = TermColor::Indexed(clamp_u8(values[*i + 2]));
                *i += 2;
                return Some(c);
            }
            if values[*i + 1] == 2 && *i + 4 < values.len() {
                let c = TermColor::Rgb(
                    clamp_u8(values[*i + 2]),
                    clamp_u8(values[*i + 3]),
                    clamp_u8(values[*i + 4]),
                );
                *i += 4;
                return Some(c);
            }
            None
        };
        while i < values.len() {
            let v = values[i];
            let style = &mut self.style;
            match v {
                0 => *style = TermStyle::default(),
                1 => style.bold = true,
                2 => style.dim = true,
                3 => style.italic = true,
                4 => style.underline = true,
                7 => style.inverse = true,
                22 => {
                    style.bold = false;
                    style.dim = false;
                }
                23 => style.italic = false,
                24 => style.underline = false,
                27 => style.inverse = false,
                30..=37 => style.fg = Some(TermColor::Indexed((v - 30) as u8)),
                38 => style.fg = color(&mut i),
                39 => style.fg = None,
                40..=47 => style.bg = Some(TermColor::Indexed((v - 40) as u8)),
                48 => style.bg = color(&mut i),
                49 => style.bg = None,
                90..=97 => style.fg = Some(TermColor::Indexed((v - 90 + 8) as u8)),
                100..=107 => style.bg = Some(TermColor::Indexed((v - 100 + 8) as u8)),
                _ => {}
            }
            i += 1;
        }
    }
}
