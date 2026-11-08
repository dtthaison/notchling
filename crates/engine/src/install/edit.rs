//! Edits by splicing: add a member or element at the end of an object or array, or remove one.
//! Every byte outside the few it touches stays as it was (spacing, comments, key order, number
//! spelling). Each call reads the text afresh, so a caller that makes several edits needs no
//! bookkeeping of moved positions.
//!
//! New text follows the file: the indent unit it uses, `\r\n` if it uses them, and one line per
//! item in a container that has one line per item.

use super::jsonc::{self, next_significant, Kind, Node, ParseError};
use super::render::{string, Json};

/// One step of a path into the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    Key(String),
    At(usize),
}

pub fn key(name: &str) -> Seg {
    Seg::Key(name.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    Parse(ParseError),
    /// The path leads nowhere, or to something that holds no items.
    Path(&'static str),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::Parse(e) => write!(f, "{e}"),
            EditError::Path(what) => write!(f, "{what}"),
        }
    }
}

impl From<ParseError> for EditError {
    fn from(e: ParseError) -> EditError {
        EditError::Parse(e)
    }
}

fn locate<'a>(root: &'a Node, path: &[Seg]) -> Option<&'a Node> {
    let mut node = root;
    for seg in path {
        node = match seg {
            Seg::Key(k) => node.get(k)?,
            Seg::At(i) => node.items()?.get(*i)?,
        };
    }
    Some(node)
}

/// The byte ranges of a container's items: a member runs from its key to the end of its value.
fn spans(node: &Node) -> Result<Vec<(usize, usize)>, EditError> {
    match &node.kind {
        Kind::Object(members) => Ok(members.iter().map(|m| (m.start, m.value.end)).collect()),
        Kind::Array(items) => Ok(items.iter().map(|n| (n.start, n.end)).collect()),
        _ => Err(EditError::Path("not an object or a list")),
    }
}

fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn line_start(text: &str, pos: usize) -> usize {
    text[..pos].rfind('\n').map_or(0, |i| i + 1)
}

/// The end of the line holding `pos`, before its `\r\n` or `\n`.
fn line_end(text: &str, pos: usize) -> usize {
    let end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
    if end > pos && text.as_bytes()[end - 1] == b'\r' {
        end - 1
    } else {
        end
    }
}

fn indent_of_line(text: &str, pos: usize) -> &str {
    let from = line_start(text, pos);
    let len = text[from..]
        .find(|c| c != ' ' && c != '\t')
        .unwrap_or(text.len() - from);
    &text[from..from + len]
}

/// Whether only space comes before `pos` on its line.
fn starts_line(text: &str, pos: usize) -> bool {
    text[line_start(text, pos)..pos].trim().is_empty()
}

/// The indent one level adds: what the first member of the top object is indented by.
fn indent_unit(text: &str, root: &Node) -> String {
    if let Some(first) = root.members().and_then(<[_]>::first) {
        if starts_line(text, first.start) {
            let indent = indent_of_line(text, first.start);
            if !indent.is_empty() {
                return indent.to_string();
            }
        }
    }
    "  ".to_string()
}

fn comment_only(rest: &str) -> bool {
    let rest = rest.trim();
    rest.starts_with("//") || (rest.starts_with("/*") && rest.ends_with("*/"))
}

fn splice(text: &str, mut edits: Vec<(usize, usize, String)>) -> String {
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = text.to_string();
    for (from, to, with) in edits {
        out.replace_range(from..to, &with);
    }
    out
}

/// Adds `value` (under `key` in an object) after the last item of the container at `path`.
pub fn append(
    text: &str,
    path: &[Seg],
    key: Option<&str>,
    value: &Json,
) -> Result<String, EditError> {
    let root = jsonc::parse(text)?;
    let target = locate(&root, path).ok_or(EditError::Path("no such place"))?;
    let items = spans(target)?;
    if matches!(target.kind, Kind::Object(_)) != key.is_some() {
        return Err(EditError::Path(
            "a member needs a key and an element has none",
        ));
    }
    let (open, close) = (target.start, target.end - 1);
    let (nl, unit) = (newline(text), indent_unit(text, &root));
    let base = indent_of_line(text, open).to_string();
    let render = |inline: bool, indent: &str| {
        let body = if inline {
            value.compact()
        } else {
            value.pretty(indent, &unit, nl)
        };
        match key {
            Some(k) => format!("{}: {body}", string(k)),
            None => body,
        }
    };

    let Some(&(first_start, _)) = items.first() else {
        let interior_blank = text[open + 1..close].trim().is_empty();
        if !text.trim().contains('\n') {
            return Ok(splice(text, vec![(open + 1, close, render(true, ""))]));
        }
        let inner = format!("{base}{unit}");
        let one = format!("{nl}{inner}{}", render(false, &inner));
        let edit = if interior_blank {
            (open + 1, close, format!("{one}{nl}{base}"))
        } else {
            (open + 1, open + 1, one)
        };
        return Ok(splice(text, vec![edit]));
    };

    let last_end = items.last().map_or(first_start, |i| i.1);
    let comma = next_significant(text, last_end);
    let trailing_comma = text.as_bytes().get(comma) == Some(&b',');
    let anchor = if trailing_comma { comma + 1 } else { last_end };
    let multi_line = text[open + 1..first_start].contains('\n');
    if !multi_line {
        let sep = if text[open + 1..close].contains(", ") {
            " "
        } else {
            ""
        };
        let lead = if trailing_comma { "" } else { "," };
        let add = format!("{lead}{sep}{}", render(true, ""));
        return Ok(splice(text, vec![(anchor, anchor, add)]));
    }
    let indent = if starts_line(text, first_start) {
        indent_of_line(text, first_start).to_string()
    } else {
        format!("{base}{unit}")
    };
    let item = format!("{nl}{indent}{}", render(false, &indent));
    // A comment after the last item stays on that item's line.
    let eol = line_end(text, anchor);
    let at = if comment_only(&text[anchor..eol]) {
        eol
    } else {
        anchor
    };
    let mut edits = Vec::new();
    if trailing_comma || at == anchor {
        let lead = if trailing_comma { "" } else { "," };
        edits.push((at, at, format!("{lead}{item}")));
    } else {
        edits.push((anchor, anchor, ",".to_string()));
        edits.push((at, at, item));
    }
    Ok(splice(text, edits))
}

/// Removes item `index` of the container at `path`, with the comma that went with it.
pub fn remove(text: &str, path: &[Seg], index: usize) -> Result<String, EditError> {
    let root = jsonc::parse(text)?;
    let target = locate(&root, path).ok_or(EditError::Path("no such place"))?;
    let items = spans(target)?;
    let &(start, end) = items.get(index).ok_or(EditError::Path("no such item"))?;
    let (open, close) = (target.start, target.end - 1);
    let last = index + 1 == items.len();

    // Nothing else inside: leave `[]` or `{}`.
    if items.len() == 1 {
        let others = format!("{}{}", &text[open + 1..start], &text[end..close]);
        if others.chars().all(|c| c.is_whitespace() || c == ',') {
            return Ok(splice(text, vec![(open + 1, close, String::new())]));
        }
    }

    let after = next_significant(text, end);
    let has_comma = text.as_bytes().get(after) == Some(&b',');
    let done = if has_comma { after + 1 } else { end };
    let eol = line_end(text, done);
    let whole_lines = starts_line(text, start)
        && (text[done..eol].trim().is_empty() || comment_only(&text[done..eol]));
    let mut edits = Vec::new();
    if whole_lines {
        let from = line_start(text, start);
        let to = text[eol..].find('\n').map_or(text.len(), |i| eol + i + 1);
        edits.push((from, to, String::new()));
        if last && !has_comma && index > 0 {
            let p = next_significant(text, items[index - 1].1);
            edits.push((p, p + 1, String::new()));
        }
    } else if !last {
        edits.push((start, items[index + 1].0, String::new()));
    } else if index > 0 {
        let p = next_significant(text, items[index - 1].1);
        edits.push((p, end, String::new()));
    } else {
        edits.push((start, done, String::new()));
    }
    Ok(splice(text, edits))
}
