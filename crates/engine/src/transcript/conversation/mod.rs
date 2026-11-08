//! The Claude Code conversation reader behind Chat: `read(path)` gives the items, tasks, facts,
//! queued messages, finished subagents and background shells of a session transcript.

mod parser;
mod tools;
mod user;

pub use parser::Parser;
pub use tools::{diff_lines, make_call, pretty, result_text};
pub use user::{model_id_in_confirmation, user_item};

use super::model::Conversation;
use super::tail::{Cursors, LineParser, CHAT_TAIL_BYTES};
use std::sync::OnceLock;

/// A subagent's transcript parser: its lines are all marked sidechain.
pub struct SubagentParser(Parser);

impl Default for SubagentParser {
    fn default() -> Self {
        SubagentParser(Parser::for_subagent())
    }
}

impl LineParser for SubagentParser {
    fn consume(&mut self, line: &[u8]) {
        self.0.consume(line);
    }
}

fn session_cursors() -> &'static Cursors<Parser, Conversation> {
    static CURSORS: OnceLock<Cursors<Parser, Conversation>> = OnceLock::new();
    CURSORS.get_or_init(|| Cursors::new(CHAT_TAIL_BYTES, 60, Parser::conversation))
}

fn subagent_cursors() -> &'static Cursors<SubagentParser, Conversation> {
    static CURSORS: OnceLock<Cursors<SubagentParser, Conversation>> = OnceLock::new();
    CURSORS
        .get_or_init(|| Cursors::new(CHAT_TAIL_BYTES, 30, |p: &SubagentParser| p.0.conversation()))
}

/// A session transcript as Chat's conversation. The first call parses the last 4 MB; later calls parse
/// only the lines appended since. An unreadable file gives an empty conversation.
pub fn read(path: &str) -> Conversation {
    session_cursors().read(path).unwrap_or_default()
}

/// A subagent's own transcript (see `subagents`), whose every line is a sidechain line.
pub fn read_subagent(path: &str) -> Conversation {
    subagent_cursors().read(path).unwrap_or_default()
}
