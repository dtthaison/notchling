use super::model::Tool;
use crate::reply::shell;
use crate::terminal::input_box::{holds_message, leads_with_picture, Fill, Input};
use crate::transcript::stored::{is_blank, precleaned};

const FOLDS_AT: usize = 200;
pub const BOX_SHOWN_MAX: usize = 1_000;

pub(super) fn command_start(text: &str) -> Option<char> {
    precleaned(text)
        .trim_start()
        .chars()
        .next()
        .filter(|c| matches!(c, '/' | '!'))
}

pub fn typed_command(tool: Tool, text: &str) -> bool {
    tool == Tool::Claude && command_start(text).is_some() && !leads_with_picture(&precleaned(text))
}

pub fn holds_command(tool: Tool, typed: &str, text: &str) -> bool {
    if !typed_command(tool, text) || is_blank(typed) {
        return false;
    }
    let pasted = shell::normalize(&precleaned(text));
    let folds = pasted.contains('\n') || pasted.chars().count() >= FOLDS_AT;
    let shown = typed.trim_end();
    let cut = shown.ends_with('…') && shown.chars().count() == BOX_SHOWN_MAX + 1;
    let input = Input {
        held: typed.lines().map(String::from).collect(),
        empty: false,
    };
    let owns = [text, pasted.as_str()];
    owns.iter().any(|own| match input.fill(own) {
        Fill::Whole => holds_message(typed, own),
        Fill::Folded => folds,
        Fill::Part => cut,
        Fill::Empty | Fill::Foreign => false,
    })
}
