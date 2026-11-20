use super::stored::{as_stored, squeezed};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    pub pictures: usize,
    pub bash: Option<String>,
    pub command: Option<(String, String)>,
    pub paths: Vec<PathBuf>,
}

impl Message {
    pub fn parse(text: &str, exists: impl Fn(&Path) -> bool) -> Message {
        let text = as_stored(text);
        let (words, pictures) = split_pictures(&text, exists);
        let lead = words.trim_start();
        let bash = lead
            .strip_prefix('!')
            .map(squeezed)
            .filter(|command| !command.is_empty());
        let command = lead.strip_prefix('/').and_then(|rest| {
            let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            (!name.is_empty()).then(|| (name.to_string(), squeezed(args)))
        });
        Message {
            text: squeezed(&words),
            pictures: pictures.len(),
            paths: pictures,
            bash,
            command,
        }
    }
}

pub fn cut_chips(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("[Image #") {
        let after = &rest[start + "[Image #".len()..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && after[digits..].starts_with(']') {
            out.push_str(&rest[..start]);
            rest = &after[digits + 1..];
        } else {
            out.push_str(&rest[..start + "[Image #".len()]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

pub fn chip_count(text: &str) -> usize {
    let mut count = 0;
    let mut rest = text;
    while let Some(start) = rest.find("[Image #") {
        let after = &rest[start + "[Image #".len()..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && after[digits..].starts_with(']') {
            count += 1;
            rest = &after[digits + 1..];
        } else {
            rest = after;
        }
    }
    count
}

const PICTURE_KINDS: [&str; 5] = [".png", ".jpg", ".jpeg", ".gif", ".webp"];

pub fn is_picture_name(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    PICTURE_KINDS.iter().any(|kind| lower.ends_with(kind))
}

pub fn is_image(path: &Path) -> bool {
    is_picture_name(&path.to_string_lossy())
}

pub fn image_spans(text: &str) -> Vec<(std::ops::Range<usize>, PathBuf)> {
    let mut out = Vec::new();
    let mut keep = |word: &mut String, from: usize, to: usize, bare_end: bool| {
        let mut to = to;
        if bare_end {
            while let Some(c) = word.chars().last().filter(|c| ",.;:!?)".contains(*c)) {
                word.pop();
                to -= c.len_utf8();
            }
        }
        let path = PathBuf::from(word.as_str());
        if word.starts_with('/') && is_image(&path) {
            out.push((from..to, path));
        }
        word.clear();
    };
    let mut word = String::new();
    let mut from: Option<usize> = None;
    let mut bare = false;
    let mut chars = text.char_indices();
    while let Some((at, c)) = chars.next() {
        if !c.is_whitespace() && from.is_none() {
            from = Some(at);
        }
        match c {
            '\'' => {
                let mut closed = false;
                for (_, q) in chars.by_ref() {
                    if q == '\'' {
                        closed = true;
                        break;
                    }
                    word.push(q);
                }
                if !closed {
                    return out;
                }
                bare = false;
            }
            '\\' => {
                if let Some((_, next)) = chars.next() {
                    word.push(next);
                }
                bare = false;
            }
            c if c.is_whitespace() => {
                if let Some(start) = from.take() {
                    keep(&mut word, start, at, bare);
                }
            }
            c => {
                word.push(c);
                bare = true;
            }
        }
    }
    if let Some(start) = from {
        keep(&mut word, start, text.len(), bare);
    }
    out
}

pub fn split_pictures(text: &str, exists: impl Fn(&Path) -> bool) -> (String, Vec<PathBuf>) {
    let mut words = String::new();
    let mut pictures: Vec<PathBuf> = Vec::new();
    let mut at = 0;
    for (range, path) in image_spans(text) {
        if !exists(&path) {
            continue;
        }
        let mut before = &text[at..range.start];
        let mut end = range.end;
        if let Some(cut) = before.strip_suffix(' ') {
            before = cut;
        } else if text[end..].starts_with(' ') {
            end += 1;
        }
        words.push_str(before);
        at = end;
        if !pictures.contains(&path) {
            pictures.push(path);
        }
    }
    words.push_str(&text[at..]);
    if !pictures.is_empty() {
        words = one_blank_line_at_most(&words);
    }
    (words.trim().to_owned(), pictures)
}

fn one_blank_line_at_most(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blanks = 0;
    for (i, line) in text.split('\n').enumerate() {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
#[path = "message_tests.rs"]
mod tests;
