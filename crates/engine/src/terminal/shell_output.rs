use super::input_box::{is_rule, span};

pub const MAX_LINES: usize = 120;
pub const MAX_LINE_CHARS: usize = 4_000;

const CONNECTOR: char = '⎿';
const CONNECTOR_INDENT: usize = 4;
const CONNECTOR_PAD: usize = 2;
const WRAP_SLACK: usize = 4;
const MARKS: [char; 2] = ['❯', '>'];
const CHROME: [char; 12] = ['❯', '⏺', '●', '✻', '✽', '✶', '✳', '✢', '✦', '※', '·', '◐'];

fn squeezed(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn indent(row: &str) -> usize {
    row.chars().take_while(|c| *c == ' ').count()
}

fn echo_text(row: &str) -> Option<String> {
    let mut rest = row.trim_start();
    if let Some(mark) = rest.chars().next().filter(|c| MARKS.contains(c)) {
        rest = rest[mark.len_utf8()..].trim_start();
    }
    let typed = squeezed(rest.strip_prefix('!')?);
    (!typed.is_empty()).then_some(typed)
}

fn covers(screen: &[String], at: usize, typed: String, want: &str) -> Option<usize> {
    if !want.starts_with(&typed) {
        return None;
    }
    let mut got = typed;
    let mut last = at;
    while got.len() < want.len() {
        let next = squeezed(screen.get(last + 1)?);
        let left = &want[got.len()..];
        if left.starts_with(&next) && !next.is_empty() {
            got.push_str(&next);
        } else if next.starts_with(left) {
            got.push_str(left);
        } else {
            return None;
        }
        last += 1;
    }
    Some(last)
}

fn echo_end(screen: &[String], command: &str) -> Option<usize> {
    let whole = squeezed(command);
    let first = squeezed(command.lines().next().unwrap_or(""));
    if first.is_empty() {
        return None;
    }
    let ends = match span(screen) {
        Some((top, _)) => top,
        None => screen
            .iter()
            .rposition(|r| is_rule(r))
            .unwrap_or(screen.len()),
    };
    let mut wants = vec![whole.as_str()];
    if first != whole {
        wants.push(first.as_str());
    }
    (0..ends).rev().find_map(|at| {
        let typed = echo_text(&screen[at])?;
        wants
            .iter()
            .find_map(|want| covers(screen, at, typed.clone(), want))
    })
}

fn is_status(line: &str) -> bool {
    let line = line.trim();
    if line.starts_with("Running…") || line.starts_with("Running...") {
        return true;
    }
    if line.contains("ctrl+o to expand") || line.contains("ctrl+b to run in background") {
        return true;
    }
    if line.starts_with("… +") && line.contains("line") {
        return true;
    }
    let Some(inner) = line.strip_prefix('(').and_then(|l| l.strip_suffix(')')) else {
        return false;
    };
    let elapsed = inner.split(" · ").next().unwrap_or("");
    !elapsed.is_empty()
        && elapsed.split_whitespace().all(|part| {
            let digits = part.trim_end_matches(['s', 'm', 'h']);
            part.len() > digits.len()
                && part.len() - digits.len() == 1
                && !digits.is_empty()
                && digits.chars().all(|c| c.is_ascii_digit())
        })
}

fn chrome(row: &str) -> bool {
    row.chars().next().is_some_and(|c| CHROME.contains(&c))
}

fn continues_link(row: &str, cols: usize, inside: bool) -> bool {
    let full = row.chars().count() + WRAP_SLACK >= cols;
    let text = row.trim_start();
    let last = text.split_whitespace().last().unwrap_or("");
    full && !text.is_empty()
        && (last.contains("://") || (inside && !text.contains(char::is_whitespace)))
}

fn link_piece(row: &str, columns: usize) -> Option<&str> {
    let lead = indent(row);
    if lead != 0 && lead != columns {
        return None;
    }
    let piece = row[lead..].trim_end();
    (!piece.is_empty() && piece.chars().all(|c| c.is_ascii_graphic())).then_some(piece)
}

fn strip(row: &str, columns: usize) -> String {
    let drop = indent(row).min(columns);
    row.chars()
        .skip(drop)
        .collect::<String>()
        .trim_end()
        .to_string()
}

pub fn output_below(screen: &[String], command: &str, cols: usize) -> Vec<String> {
    let Some(echo) = echo_end(screen, command) else {
        return Vec::new();
    };
    let below = &screen[echo + 1..];
    let end = below.iter().position(|r| is_rule(r)).unwrap_or(below.len());
    let body = &below[..end];
    let Some(first) = body.iter().position(|r| !r.trim().is_empty()) else {
        return Vec::new();
    };
    let lead = indent(&body[first]);
    let connected = lead <= CONNECTOR_INDENT && body[first].trim_start().starts_with(CONNECTOR);
    let (pad, text_col) = if connected {
        let after = &body[first].trim_start()[CONNECTOR.len_utf8()..];
        let pad = after
            .chars()
            .take_while(|c| *c == ' ')
            .count()
            .min(CONNECTOR_PAD);
        (pad, lead + 1 + pad)
    } else {
        (0, 0)
    };
    let mut lines: Vec<String> = Vec::new();
    let mut link = false;
    for (n, row) in body[first..].iter().enumerate() {
        if row.trim().is_empty() {
            lines.push(String::new());
            link = false;
            continue;
        }
        let piece = if link {
            link_piece(row, text_col)
        } else {
            None
        };
        let joined = piece.is_some();
        match (piece, lines.last_mut()) {
            (Some(piece), Some(last)) => last.push_str(piece),
            _ => {
                let text = if connected && n == 0 {
                    let after = &row.trim_start()[CONNECTOR.len_utf8()..];
                    after
                        .chars()
                        .skip(pad)
                        .collect::<String>()
                        .trim_end()
                        .to_string()
                } else if connected {
                    if indent(row) < text_col {
                        break;
                    }
                    strip(row, text_col)
                } else {
                    if chrome(row) {
                        break;
                    }
                    strip(row, 0)
                };
                lines.push(text);
            }
        }
        link = continues_link(row, cols, joined);
    }
    lines.retain(|l| !is_status(l));
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let lead = lines.iter().take_while(|l| l.is_empty()).count();
    lines.drain(..lead);
    if lines.len() > MAX_LINES {
        lines.drain(..lines.len() - MAX_LINES);
    }
    lines
        .into_iter()
        .map(|l| l.chars().take(MAX_LINE_CHARS).collect())
        .collect()
}
