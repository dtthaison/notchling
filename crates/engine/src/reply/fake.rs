use super::exec::Pty;
use super::keys::{self, ENTER, PASTE_END, PASTE_START};
use crate::terminal::input_box;
use std::time::Duration;

pub const RULE: &str = "────────────────────────────────────────";
pub const PROMPT: [&str; 4] = [RULE, "❯ ", RULE, "  ? for shortcuts"];

#[derive(Clone, Debug, Default)]
pub struct FakePrompt {
    before: Vec<String>,
    after: Vec<String>,
    boxed: bool,
    text: String,
    hint: String,
    clock: u64,
    paste_ms: u64,
    landing: Option<(u64, String)>,
    deaf: bool,
    stuck: bool,
    pub writes: Vec<String>,
    pub sent: Vec<String>,
}

impl FakePrompt {
    pub fn new(lines: &[&str]) -> FakePrompt {
        let lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        let Some((top, bottom)) = input_box::span(&lines) else {
            return FakePrompt {
                before: lines,
                ..FakePrompt::default()
            };
        };
        let rows: Vec<&str> = lines[top + 1..bottom]
            .iter()
            .enumerate()
            .map(|(i, row)| match i {
                0 => row.strip_prefix('❯').unwrap_or(row).trim_start(),
                _ => row.trim_start(),
            })
            .collect();
        FakePrompt {
            before: lines[..=top].to_vec(),
            after: lines[bottom..].to_vec(),
            boxed: true,
            text: rows.join("\n").trim_end().to_string(),
            ..FakePrompt::default()
        }
    }

    pub fn idle() -> FakePrompt {
        FakePrompt::new(&PROMPT)
    }

    pub fn pasting_for(mut self, ms: u64) -> FakePrompt {
        self.paste_ms = ms;
        self
    }

    pub fn deaf(mut self) -> FakePrompt {
        self.deaf = true;
        self
    }

    pub fn stuck(mut self) -> FakePrompt {
        self.stuck = true;
        self
    }

    pub fn holding(mut self, text: &str) -> FakePrompt {
        self.text = text.to_string();
        self
    }

    pub fn hinting(mut self, hint: &str) -> FakePrompt {
        self.hint = hint.to_string();
        self
    }

    pub fn held(&self) -> Option<&str> {
        self.boxed.then_some(self.text.as_str())
    }

    fn rows(&self) -> Vec<String> {
        if self.text.is_empty() {
            return vec![format!("❯ {}", self.hint)];
        }
        self.text
            .split('\n')
            .enumerate()
            .map(|(i, line)| match i {
                0 => format!("❯ {line}"),
                _ => format!("  {line}"),
            })
            .collect()
    }

    fn pass(&mut self, ms: u64) {
        self.clock += ms;
        if let Some((due, text)) = self.landing.take() {
            if self.clock < due {
                self.landing = Some((due, text));
            } else if self.boxed {
                self.text.push_str(&text);
            }
        }
    }

    fn entered(&mut self) {
        if self.deaf || self.landing.is_some() || !self.boxed || self.text.is_empty() {
            return;
        }
        self.sent.push(std::mem::take(&mut self.text));
    }
}

impl Pty for FakePrompt {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let typed = String::from_utf8_lossy(bytes).into_owned();
        if typed == keys::clear_input() {
            if !self.stuck {
                self.text.clear();
            }
        } else if typed == ENTER {
            self.entered();
        } else if let Some(text) = typed
            .strip_prefix(PASTE_START)
            .and_then(|t| t.strip_suffix(PASTE_END))
        {
            self.landing = Some((self.clock + self.paste_ms, text.to_string()));
            self.pass(0);
        }
        self.writes.push(typed);
        Ok(())
    }

    fn wait_quiet(&mut self, quiet: Duration, _timeout: Duration) -> bool {
        self.pass(quiet.as_millis() as u64);
        true
    }

    fn screen(&mut self) -> Vec<String> {
        let mut lines = self.before.clone();
        if self.boxed {
            lines.extend(self.rows());
        }
        lines.extend(self.after.iter().cloned());
        lines
    }

    fn cursor(&mut self) -> Option<(usize, usize)> {
        if !self.boxed {
            return None;
        }
        let rows = self.rows();
        let typed = self.text.rsplit('\n').next().unwrap_or("").chars().count();
        Some((self.before.len() + rows.len() - 1, 2 + typed))
    }

    fn sleep(&mut self, duration: Duration) {
        self.pass(duration.as_millis() as u64);
    }

    fn now_ms(&self) -> u64 {
        self.clock
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paste_lands_in_the_box_and_enter_sends_it() {
        let mut pty = FakePrompt::idle();
        pty.write(keys::bracketed("hi\nthere").as_bytes()).unwrap();
        assert_eq!(pty.held(), Some("hi\nthere"));
        assert_eq!(pty.screen()[1..3], ["❯ hi", "  there"]);
        assert_eq!(pty.cursor(), Some((2, 7)));
        pty.write(ENTER.as_bytes()).unwrap();
        assert_eq!(pty.sent, ["hi\nthere"]);
        assert_eq!(pty.held(), Some(""));
        assert_eq!(pty.screen(), PROMPT);
        assert_eq!(pty.cursor(), Some((1, 2)));
    }

    #[test]
    fn an_enter_before_the_paste_lands_is_spent_on_an_empty_box() {
        let mut pty = FakePrompt::idle().pasting_for(400);
        pty.write(keys::bracketed("hi").as_bytes()).unwrap();
        pty.sleep(Duration::from_millis(150));
        pty.write(ENTER.as_bytes()).unwrap();
        pty.sleep(Duration::from_millis(250));
        assert!(pty.sent.is_empty());
        assert_eq!(pty.held(), Some("hi"));
        pty.write(ENTER.as_bytes()).unwrap();
        assert_eq!(pty.sent, ["hi"]);
    }

    #[test]
    fn a_hint_shows_in_an_empty_box_with_the_cursor_at_its_start() {
        let mut pty = FakePrompt::idle().hinting("Try \"fix the test\"");
        assert_eq!(pty.screen()[1], "❯ Try \"fix the test\"");
        assert_eq!(pty.cursor(), Some((1, 2)));
        assert_eq!(pty.held(), Some(""));
        pty.write(ENTER.as_bytes()).unwrap();
        assert!(pty.sent.is_empty());
    }

    #[test]
    fn a_draft_on_the_screen_is_read_and_cleared() {
        let mut pty = FakePrompt::new(&["⏺ ok", RULE, "❯ stale", "  draft", RULE]);
        assert_eq!(pty.held(), Some("stale\ndraft"));
        pty.write(keys::clear_input().as_bytes()).unwrap();
        assert_eq!(pty.held(), Some(""));
        assert_eq!(pty.screen(), ["⏺ ok", RULE, "❯ ", RULE]);
    }

    #[test]
    fn a_screen_with_no_box_holds_nothing() {
        let mut pty = FakePrompt::new(&["Do you want to proceed?", " 1. Yes"]);
        pty.write(keys::bracketed("hi").as_bytes()).unwrap();
        pty.write(ENTER.as_bytes()).unwrap();
        assert_eq!(pty.held(), None);
        assert_eq!(pty.cursor(), None);
        assert!(pty.sent.is_empty());
        assert_eq!(pty.writes.len(), 2);
        assert_eq!(pty.screen(), ["Do you want to proceed?", " 1. Yes"]);
    }
}
