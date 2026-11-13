use std::cell::RefCell;

use gpui::{App, SharedString};
use notchling_engine::transcript::model::Conversation;

use super::conversation::live_group_of;
use super::conversation_view::{ConversationInput, ConversationState};
use super::row_select::key_of;
use super::ChatView;
use crate::chat::blocks::{plain_blocks, Shown};
use crate::chat::markdown_cache::Cache;
use crate::chat::rows::{rows, with_outgoing};

pub(super) fn rows_blocks(
    shown: &Conversation,
    outgoing: &[String],
    live: Option<usize>,
    folder: &str,
    cache: &RefCell<Cache>,
    is_open: &dyn Fn(&str) -> bool,
) -> Vec<(SharedString, Vec<String>)> {
    let is_file = |path: &std::path::Path| cache.borrow().is_file(path);
    with_outgoing(rows(shown), outgoing)
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let key = key_of(&shown.items, i, row);
            let message = match row {
                crate::chat::rows::Row::Assistant { text } => {
                    Some(cache.borrow_mut().message(&key, text, folder))
                }
                _ => None,
            };
            let on_show = Shown {
                is_open,
                live: live == Some(i),
                is_file: &is_file,
            };
            let blocks = plain_blocks(row, &key, message.as_deref().map(Vec::as_slice), &on_show);
            (key, blocks)
        })
        .collect()
}

pub(super) fn input_blocks(
    input: &ConversationInput,
    s: &ConversationState,
) -> Vec<(SharedString, Vec<String>)> {
    let folder = input.session.as_ref().map_or("", |s| s.cwd.as_str());
    rows_blocks(
        &input.shown,
        &input.outgoing,
        live_group_of(input),
        folder,
        &s.markdown,
        &|key| s.is_open(key),
    )
}

impl ChatView {
    pub fn check_blocks(&self, cx: &App) -> Result<usize, String> {
        let input = self.conversation_input(cx);
        let view = self.parts.conversation.read(cx);
        let drawn = view.s.sel.borrow().noted();
        let on_screen: std::collections::HashSet<&SharedString> =
            drawn.iter().map(|(row, ..)| row).collect();
        let model: Vec<(SharedString, u16, String)> = input_blocks(&input, &view.s)
            .into_iter()
            .filter(|(row, _)| on_screen.contains(row))
            .flat_map(|(row, blocks)| {
                blocks
                    .into_iter()
                    .enumerate()
                    .map(move |(b, text)| (row.clone(), b as u16, text))
            })
            .collect();
        for i in 0..drawn.len().max(model.len()) {
            match (drawn.get(i), model.get(i)) {
                (Some(d), Some(m)) if d == m => {}
                (Some((row, block, text)), model) => {
                    return Err(format!(
                        "mismatch at {i}: drawn {row}#{block} {text:?} model {:?}",
                        model.map(|(row, block, text)| format!("{row}#{block} {text}"))
                    ))
                }
                (None, model) => {
                    return Err(format!(
                        "mismatch at {i}: drawn nothing model {:?}",
                        model.map(|(row, block, text)| format!("{row}#{block} {text}"))
                    ))
                }
            }
        }
        Ok(drawn.len())
    }
}
