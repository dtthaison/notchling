mod join;
mod parser;
mod queue;
mod tools;
mod user;

pub use parser::Parser;
use parser::MAX_LINE;
pub use tools::{diff_lines, make_call, pretty, result_text};
pub use user::{model_id_in_confirmation, user_item};

use super::model::Conversation;
use super::tail::{Cursors, LineParser, CHAT_TAIL_BYTES, LONG_LINE_SCAN};
use std::sync::{Arc, OnceLock};

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

    fn consume_oversized(&mut self, head: &[u8], len: u64) {
        self.0.consume_oversized(head, len);
    }
}

fn session_cursors() -> &'static Cursors<Parser, Arc<Conversation>> {
    static CURSORS: OnceLock<Cursors<Parser, Arc<Conversation>>> = OnceLock::new();
    CURSORS.get_or_init(|| {
        Cursors::new(CHAT_TAIL_BYTES, 60, |p: &Parser| Arc::new(p.conversation()))
            .skipping_lines_over(MAX_LINE, LONG_LINE_SCAN)
    })
}

fn subagent_cursors() -> &'static Cursors<SubagentParser, Arc<Conversation>> {
    static CURSORS: OnceLock<Cursors<SubagentParser, Arc<Conversation>>> = OnceLock::new();
    CURSORS.get_or_init(|| {
        Cursors::new(CHAT_TAIL_BYTES, 30, |p: &SubagentParser| {
            Arc::new(p.0.conversation())
        })
        .skipping_lines_over(MAX_LINE, LONG_LINE_SCAN)
    })
}

pub fn read_shared(path: &str) -> Arc<Conversation> {
    join::read(path)
}

pub fn read(path: &str) -> Conversation {
    (*read_shared(path)).clone()
}

pub fn read_subagent_shared(path: &str) -> Arc<Conversation> {
    subagent_cursors().read(path).unwrap_or_default()
}

pub fn read_subagent(path: &str) -> Conversation {
    (*read_subagent_shared(path)).clone()
}
