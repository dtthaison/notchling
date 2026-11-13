use crate::chat::drops::split_images;
use crate::chat::markdown_cache::{spans_text, Part};
use crate::chat::rows::Row;
use crate::chat::steps::{self, Detail};

pub struct Shown<'a> {
    pub is_open: &'a dyn Fn(&str) -> bool,
    pub live: bool,
    pub is_file: &'a dyn Fn(&std::path::Path) -> bool,
}

pub fn plain_blocks(row: &Row, id: &str, message: Option<&[Part]>, shown: &Shown) -> Vec<String> {
    match row {
        Row::User { text, .. } => {
            let (words, pictures) = split_images(text, shown.is_file);
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
