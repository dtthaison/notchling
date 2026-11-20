use super::ask_dialog::{self, is_extra, option_row};
use super::permission_dialog::clean;

const MAX_HEADER_ROWS: usize = 14;
const MAX_OPTION_ROWS: usize = 24;
const MAX_TRAILING_ROWS: usize = 2;
const FOOTERS: [&str; 3] = ["Esc to cancel", "Enter to confirm", "Enter to select"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogOption {
    pub number: u64,
    pub label: String,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenDialog {
    pub title: String,
    pub body: Vec<String>,
    pub options: Vec<DialogOption>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogPick {
    pub title: String,
    pub number: u64,
    pub label: String,
}

impl ScreenDialog {
    pub fn same_as(&self, other: &ScreenDialog) -> bool {
        compact(&self.title) == compact(&other.title)
            && self.options.len() == other.options.len()
            && self
                .options
                .iter()
                .zip(&other.options)
                .all(|(a, b)| a.number == b.number && compact(&a.label) == compact(&b.label))
    }

    pub fn offers(&self, pick: &DialogPick) -> bool {
        compact(&self.title) == compact(&pick.title)
            && self
                .options
                .iter()
                .any(|o| o.number == pick.number && compact(&o.label) == compact(&pick.label))
    }
}

fn compact(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_rule(row: &str) -> bool {
    let rules = row.chars().filter(|c| matches!(c, '─' | '━' | '═')).count();
    rules >= 8 && rules * 2 >= row.chars().count()
}

fn is_footer(row: &str) -> bool {
    FOOTERS.iter().any(|f| row.contains(f))
}

fn is_chrome(row: &str) -> bool {
    row.is_empty() || is_rule(row) || is_footer(row)
}

fn is_past_dialog(row: &str) -> bool {
    row.starts_with(['⏺', '⎿', '✻', '✽'])
        || row.contains("? for shortcuts")
        || (row.starts_with(['❯', '>', '›']) && option_row(0, row).is_none())
}

struct Numbered {
    row: usize,
    number: u64,
    label: String,
    selected: bool,
}

fn numbered(rows: &[&str], from: usize) -> Option<Vec<Numbered>> {
    let mut found: Vec<Numbered> = Vec::new();
    let floor = from.saturating_sub(MAX_OPTION_ROWS);
    for row in (floor..=from).rev() {
        let Some(option) = option_row(row, rows[row]) else {
            continue;
        };
        let wanted = found.last().map_or(option.number, |n| n.number - 1);
        if option.number != wanted {
            return None;
        }
        found.push(Numbered {
            row,
            number: option.number,
            label: option.label,
            selected: rows[row].starts_with(['❯', '>', '›']),
        });
        if option.number == 1 {
            found.reverse();
            return Some(found);
        }
    }
    None
}

fn options_of(rows: &[&str], found: &[Numbered], end: usize) -> Vec<DialogOption> {
    found
        .iter()
        .enumerate()
        .map(|(i, option)| {
            let next = found.get(i + 1).map_or(end, |n| n.row);
            let mut label = option.label.clone();
            for row in &rows[option.row + 1..next] {
                if !is_chrome(row) {
                    label.push(' ');
                    label.push_str(row);
                }
            }
            DialogOption {
                number: option.number,
                label: compact(&label),
                selected: option.selected,
            }
        })
        .collect()
}

fn header_of(rows: &[&str], first: usize) -> Vec<Vec<String>> {
    let mut paragraphs: Vec<Vec<String>> = Vec::new();
    let mut open = false;
    let mut blanks = 0;
    for row in rows[first.saturating_sub(MAX_HEADER_ROWS)..first]
        .iter()
        .rev()
    {
        if is_rule(row) || is_past_dialog(row) {
            break;
        }
        if row.is_empty() {
            blanks += 1;
            if blanks >= 2 {
                break;
            }
            open = false;
            continue;
        }
        blanks = 0;
        if !open {
            paragraphs.push(Vec::new());
            open = true;
        }
        if let Some(last) = paragraphs.last_mut() {
            last.push(compact(row));
        }
    }
    paragraphs.reverse();
    for paragraph in &mut paragraphs {
        paragraph.reverse();
    }
    paragraphs
}

pub fn read(lines: &[String]) -> Option<ScreenDialog> {
    if ask_dialog::page(lines).is_some() {
        return None;
    }
    let rows: Vec<&str> = lines.iter().map(|line| clean(line)).collect();
    let drawn = rows.iter().rposition(|row| !row.is_empty())? + 1;
    let tail = rows[..drawn]
        .iter()
        .rposition(|row| option_row(0, row).is_some())?;
    let trailing: Vec<&str> = rows[tail + 1..drawn]
        .iter()
        .copied()
        .filter(|row| !is_chrome(row))
        .collect();
    if trailing.len() > MAX_TRAILING_ROWS || trailing.iter().any(|row| is_past_dialog(row)) {
        return None;
    }
    let found = numbered(&rows, tail)?;
    if found.len() < 2 {
        return None;
    }
    let options = options_of(&rows, &found, drawn);
    if options.iter().any(|o| is_extra(&o.label)) {
        return None;
    }
    let mut paragraphs = header_of(&rows, found[0].row).into_iter();
    let head = paragraphs.next()?;
    let (title, rest) = head.split_first()?;
    let mut body: Vec<String> = Vec::new();
    if !rest.is_empty() {
        body.push(rest.join(" "));
    }
    body.extend(paragraphs.map(|paragraph| paragraph.join(" ")));
    Some(ScreenDialog {
        title: title.clone(),
        body,
        options,
    })
}
