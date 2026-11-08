//! A conversation's text, gathered line by line.

use super::text::{first_line_title, SessionRole, TranscriptLine};

/// Enough for the gist of a long session; the newest text is kept (counted in characters).
pub const MAX_CHARS: usize = 40_000;

#[derive(Clone, Debug, Default)]
pub struct SessionTextBuilder {
    messages: Vec<String>,
    first_prompt: Option<String>,
    cwd: Option<String>,
    custom_title: Option<String>,
    ai_title: Option<String>,
}

impl SessionTextBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, line: TranscriptLine) {
        match line {
            TranscriptLine::Message { role, text, cwd } => {
                if self.first_prompt.is_none() && role == SessionRole::User {
                    self.first_prompt = Some(text.clone());
                }
                if self.cwd.is_none() {
                    self.cwd = cwd.filter(|c| !c.is_empty());
                }
                self.messages.push(text);
            }
            TranscriptLine::CustomTitle(t) => self.custom_title = Some(t),
            TranscriptLine::AiTitle(t) => self.ai_title = Some(t),
        }
    }

    /// The folder of the first line that named one.
    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    /// The first thing the user said.
    pub fn first_prompt(&self) -> Option<&str> {
        self.first_prompt.as_deref()
    }

    /// What you named it, else Claude Code's own title, else the start of the first prompt.
    pub fn title(&self) -> String {
        if let Some(t) = &self.custom_title {
            return t.clone();
        }
        if let Some(t) = &self.ai_title {
            return t.clone();
        }
        self.first_prompt
            .as_deref()
            .map(first_line_title)
            .unwrap_or_else(|| "Session".to_string())
    }

    /// The messages joined with newlines, cut from the front to fit `MAX_CHARS`.
    pub fn text(&self) -> String {
        let mut kept: Vec<String> = Vec::new();
        let mut total = 0;
        for message in self.messages.iter().rev() {
            let count = message.chars().count();
            if total + count > MAX_CHARS {
                if kept.is_empty() {
                    let skip = count - MAX_CHARS.min(count);
                    kept.push(message.chars().skip(skip).collect());
                }
                break;
            }
            kept.push(message.clone());
            total += count + 1;
        }
        kept.reverse();
        kept.join("\n")
    }
}
