use super::TerminalScreen;
use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

pub const DIALOG_MARKERS: [&str; 6] = [
    "Enter to select",
    "Esc to cancel",
    "Chat about this",
    "Do you want to proceed?",
    "Do you want to allow",
    "Submit answers",
];

pub fn dialog_open(screen: &TerminalScreen) -> bool {
    let lines = screen.lines();
    let drawn: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    let tail = &drawn[drawn.len().saturating_sub(6)..];
    tail.iter()
        .any(|line| DIALOG_MARKERS.iter().any(|m| line.contains(m)))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    pub question: String,
    pub options: Vec<String>,
    pub details: Vec<String>,
    pub multi_select: bool,
    pub tabs: Vec<String>,
    pub on_review: bool,
    pub header: String,
}

pub fn is_review(lines: &[String]) -> bool {
    let drawn: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    drawn[drawn.len().saturating_sub(8)..]
        .iter()
        .any(|l| l.contains("Submit answers"))
}

pub fn shows(question: &str, lines: &[String]) -> bool {
    let wanted: String = normalized(question).chars().take(48).collect();
    if wanted.is_empty() {
        return false;
    }
    match page(lines) {
        Some(page) if !page.on_review => normalized(&page.question).contains(&wanted),
        _ => false,
    }
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn footer(rows: &[&str]) -> Option<usize> {
    rows.iter().rposition(|r| {
        r.contains("Enter to select")
            || r.contains("Esc to cancel")
            || r.contains("Enter to confirm")
    })
}

pub(crate) struct Numbered {
    pub(crate) row: usize,
    pub(crate) number: u64,
    pub(crate) label: String,
    pub(crate) boxed: bool,
}

pub(crate) fn option_row(row: usize, text: &str) -> Option<Numbered> {
    let mut rest = text;
    if let Some(r) = rest.strip_prefix(['❯', '>', '›']) {
        rest = r.trim_start();
    }
    let mut boxed = false;
    if let Some(r) = strip_box(rest) {
        rest = r.trim_start();
        boxed = true;
    } else if let Some(r) = rest.strip_prefix(['☐', '☑', '☒', '◻', '◼', '□', '■']) {
        rest = r.trim_start();
        boxed = true;
    }
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    let number: u64 = rest[..digits].parse().ok()?;
    let after = rest[digits..].strip_prefix('.')?;
    if !after.starts_with(char::is_whitespace) {
        return None;
    }
    let label = after.trim();
    if label.is_empty() {
        return None;
    }
    if let Some(r) = strip_box(label) {
        return Some(Numbered {
            row,
            number,
            label: r.trim_start().to_string(),
            boxed: true,
        });
    }
    Some(Numbered {
        row,
        number,
        label: label.to_string(),
        boxed,
    })
}

fn strip_box(text: &str) -> Option<&str> {
    let inner = text.strip_prefix('[')?;
    let mut chars = inner.chars();
    let mark = chars.next()?;
    if !matches!(mark, ' ' | 'x' | 'X' | '✓' | '✔') {
        return None;
    }
    chars.as_str().strip_prefix(']')
}

pub fn page(lines: &[String]) -> Option<Page> {
    let rows: Vec<&str> = lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    let end = footer(&rows)?;
    let body = &rows[..end];
    if is_review(lines) {
        return Some(Page {
            question: "Review your answers".to_string(),
            tabs: tabs(body),
            on_review: true,
            ..Page::default()
        });
    }

    let mut numbered: Vec<Numbered> = body
        .iter()
        .enumerate()
        .filter_map(|(i, text)| option_row(i, text))
        .collect();
    let last = numbered.pop()?;
    let mut block = vec![last];
    while let Some(row) = numbered.pop() {
        if row.number + 1 != block[0].number {
            continue;
        }
        let first = row.number == 1;
        block.insert(0, row);
        if first {
            break;
        }
    }
    if block[0].number != 1 {
        return None;
    }

    let extras = block.iter().position(|r| is_extra(&r.label))?;
    if extras == 0 {
        return None;
    }
    let options = &block[..extras];
    let details = options
        .iter()
        .enumerate()
        .map(|(i, option)| {
            let next = block.get(i + 1).map_or(body.len(), |r| r.row);
            body[option.row + 1..next]
                .iter()
                .filter(|r| !is_separator(r))
                .copied()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();

    let mut question: Vec<&str> = body[..options[0].row]
        .iter()
        .rev()
        .take_while(|text| !is_separator(text) && !is_tab_row(text) && !text.starts_with('❯'))
        .copied()
        .collect();
    question.reverse();
    let mut header = String::new();
    if question.len() > 1 {
        if let Some(chip) = chip_title(question[0]) {
            header = chip;
            question.remove(0);
        }
    }
    if question.is_empty() {
        return None;
    }
    Some(Page {
        question: question.join(" "),
        options: options.iter().map(|o| o.label.clone()).collect(),
        details,
        multi_select: options.iter().any(|o| o.boxed),
        tabs: tabs(body),
        on_review: false,
        header,
    })
}

const STATE_MARKS: [char; 7] = ['☐', '☒', '☑', '✔', '✓', '□', '■'];

fn chip_title(row: &str) -> Option<String> {
    let rest = row.strip_prefix(STATE_MARKS)?;
    let title = rest.trim();
    (!title.is_empty() && title.chars().count() <= 40).then(|| title.to_string())
}

pub(crate) fn is_extra(label: &str) -> bool {
    let text = label.to_lowercase();
    text.starts_with("type something") || text == "other" || text.starts_with("chat about this")
}

fn is_separator(text: &str) -> bool {
    text.chars().count() >= 8 && text.chars().all(|c| matches!(c, '─' | '━' | '═' | '-'))
}

fn is_tab_row(text: &str) -> bool {
    (text.contains('←') || text.contains('→')) && text.to_lowercase().contains("submit")
}

pub fn tabs(body: &[&str]) -> Vec<String> {
    let Some(row) = body.iter().find(|r| is_tab_row(r)) else {
        return Vec::new();
    };
    let stripped = row.replace(['←', '→'], " ");
    let mut titles: Vec<String> = stripped
        .split("  ")
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|title| match title.chars().next() {
            Some(first)
                if !first.is_alphabetic()
                    && first.general_category() != GeneralCategory::DecimalNumber =>
            {
                title[first.len_utf8()..]
                    .trim_start_matches(super::width::is_combining)
                    .trim()
                    .to_string()
            }
            _ => title.to_string(),
        })
        .collect();
    if titles.last().is_some_and(|t| t.to_lowercase() == "submit") {
        titles.pop();
    }
    titles
}
