//! A row's selectable texts, from the model and in the order the row draws them, without drawing
//! it: what the conversation's selection will name by row, block and byte. Each text here is one
//! the view wraps in `select::selectable` (a user's words, a reply's paragraphs, headings, list
//! items, quotes, code and table cells, a tool step's input and output, a command's output); a
//! label, a chip, a diff line or a thought is not selectable and has no block. The render check
//! (`NOTCHLING_RENDER_CHECK_BLOCKS=1`) holds this to what the view really noted.

use crate::chat::drops::split_images;
use crate::chat::markdown_cache::{spans_text, Part};
use crate::chat::rows::Row;
use crate::chat::steps::{self, Detail};

/// What decides which of a row's texts are drawn.
pub struct Shown<'a> {
    /// Whether a group or a step is open, by its id (`ConversationState::is_open`).
    pub is_open: &'a dyn Fn(&str) -> bool,
    /// This row is the live step group (always open, its running call spinning).
    pub live: bool,
    /// Whether a path in your message is an image file that exists.
    pub is_file: &'a dyn Fn(&std::path::Path) -> bool,
}

/// The row's selectable texts in draw order. `id` is the row's item id (a step group's open key);
/// `message` is the row's parsed Markdown (`markdown_cache::Cache::message`) for an assistant row,
/// else `None`.
pub fn plain_blocks(row: &Row, id: &str, message: Option<&[Part]>, shown: &Shown) -> Vec<String> {
    match row {
        Row::User { text, .. } => {
            let (words, pictures) = split_images(text, shown.is_file);
            // The bubble draws its words, unless the message was only pictures.
            if !words.is_empty() || pictures.is_empty() {
                vec![words]
            } else {
                Vec::new()
            }
        }
        Row::Assistant { .. } => message.map(markdown_blocks).unwrap_or_default(),
        Row::Tools(tools) => {
            if !(shown.live || (shown.is_open)(id)) {
                return Vec::new();
            }
            let mut out = Vec::new();
            for step in tools {
                // A step's detail opens on its own click; a diff's lines aren't selectable.
                if (shown.is_open)(&step.id) && matches!(steps::detail(step), Detail::Text { .. }) {
                    out.extend(steps::input_text(step));
                    out.extend(steps::output_text(step));
                }
            }
            out
        }
        Row::CommandOutput { text } => vec![text.clone()],
        Row::Thinking { .. } | Row::TurnEnd { .. } | Row::Notice { .. } => Vec::new(),
    }
}

/// A reply's selectable texts, the way `view/markdown.rs` `render` draws its parts.
pub fn markdown_blocks(message: &[Part]) -> Vec<String> {
    let mut out = Vec::new();
    for part in message {
        match part {
            Part::Heading { spans, .. } => out.push(spans_text(spans)),
            Part::Paragraph(inline) | Part::Quote(inline) => out.push(inline.text()),
            Part::List(items) => out.extend(items.iter().map(|item| item.inline.text())),
            Part::Code { text, .. } => out.push(text.clone()),
            Part::Table { header, rows } => {
                out.extend(header.iter().map(|cell| spans_text(cell)));
                for row in rows {
                    out.extend(row.iter().map(|cell| spans_text(cell)));
                }
            }
            Part::Rule => {}
        }
    }
    out
}
