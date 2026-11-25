use notchling_engine::terminal::{TermCell, TermStyle};

use super::terminal::runs;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub col: usize,
    pub cells: usize,
    pub text: String,
    pub style: TermStyle,
}

impl Span {
    fn drawn_blank(style: &TermStyle) -> bool {
        style.bg.is_none() && !style.inverse && !style.underline
    }

    fn blank(&self) -> bool {
        self.text.trim().is_empty() && Span::drawn_blank(&self.style)
    }
}

fn plain(cell: &TermCell) -> bool {
    cell.width == 1 && matches!(cell.text.as_bytes(), [b' '..=b'~'])
}

pub fn spans(row: &[TermCell]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut col = 0;
    while col < row.len() {
        let cell = &row[col];
        if plain(cell) {
            let end = row[col..]
                .iter()
                .position(|c| !plain(c))
                .map_or(row.len(), |n| col + n);
            let mut at = col;
            for (text, style) in runs(&row[col..end]) {
                let width = text.len();
                let text = if Span::drawn_blank(&style) {
                    text.trim_end().to_owned()
                } else {
                    text
                };
                out.push(Span {
                    col: at,
                    cells: text.len(),
                    text,
                    style,
                });
                at += width;
            }
            col = end;
            continue;
        }
        if cell.width != 0 {
            out.push(Span {
                col,
                cells: usize::from(cell.width),
                text: cell.text.clone(),
                style: cell.style,
            });
        }
        col += 1;
    }
    out.retain(|s| !s.blank());
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caret {
    pub col: usize,
    pub cells: usize,
    pub text: String,
    pub style: TermStyle,
    pub filled: bool,
}

pub fn caret(rows: &[Vec<TermCell>], at: (usize, usize), focused: bool) -> Option<Caret> {
    let row = rows.get(at.0)?;
    let mut col = at.1.min(row.len().checked_sub(1)?);
    while col > 0 && row[col].width == 0 {
        col -= 1;
    }
    let cell = &row[col];
    Some(Caret {
        col,
        cells: usize::from(cell.width.max(1)),
        text: cell.text.clone(),
        style: cell.style,
        filled: focused,
    })
}

#[cfg(test)]
#[path = "terminal_paint_tests.rs"]
mod tests;
