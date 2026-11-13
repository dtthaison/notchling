use super::rank::{lowercase, IndexedSession};
use std::collections::BTreeSet;
use std::ops::Range;

pub const DEFAULT_RADIUS: usize = 56;

pub fn find(needle: &str, haystack: &str, from: usize) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    haystack.get(from..)?.find(needle).map(|i| i + from)
}

pub fn snippet<W: AsRef<str>>(
    session: &IndexedSession,
    words: &[W],
    radius: usize,
) -> Option<String> {
    let words: Vec<&str> = words
        .iter()
        .map(AsRef::as_ref)
        .filter(|w| !w.is_empty())
        .collect();
    let lower = session.lower.as_str();
    let mut position = None;
    if words.len() > 1 {
        position = find(&words.join(" "), lower, 0);
    }
    if position.is_none() {
        position = words.iter().filter_map(|w| find(w, lower, 0)).min();
    }
    let position = position?;
    let original = session.text.as_bytes();
    let bytes = if original.len() == lower.len() {
        original
    } else {
        lower.as_bytes()
    };
    let mut from = position.saturating_sub(radius);
    let mut to = bytes.len().min(position + radius * 2);
    while from > 0 && from < bytes.len() && bytes[from] & 0xC0 == 0x80 {
        from -= 1;
    }
    while to < bytes.len() && bytes[to] & 0xC0 == 0x80 {
        to += 1;
    }
    let mut out = collapse(&String::from_utf8_lossy(&bytes[from..to]));
    if from > 0 {
        let rest: String = out
            .chars()
            .skip_while(|c| !c.is_whitespace())
            .skip_while(|c| c.is_whitespace())
            .collect();
        out = format!("…{rest}");
    }
    if to < bytes.len() {
        if let Some(space) = out.rfind(char::is_whitespace) {
            out.truncate(space);
        }
        out.push('…');
    }
    Some(out)
}

pub fn collapse(text: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !space && !out.is_empty() {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(ch);
            space = false;
        }
    }
    out.trim_end_matches(' ').to_string()
}

pub fn highlights<W: AsRef<str>>(snippet: &str, words: &[W]) -> Vec<Range<usize>> {
    let lower: Vec<char> = lowercase(snippet).chars().collect();
    let offsets: Vec<usize> = snippet
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(snippet.len()))
        .collect();
    if lower.len() + 1 != offsets.len() {
        return Vec::new();
    }
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for word in words.iter().map(AsRef::as_ref).collect::<BTreeSet<&str>>() {
        let needle: Vec<char> = word.chars().collect();
        if needle.is_empty() {
            continue;
        }
        let mut at = 0;
        while at + needle.len() <= lower.len() {
            if lower[at..at + needle.len()] == needle[..] {
                ranges.push(offsets[at]..offsets[at + needle.len()]);
                at += needle.len();
            } else {
                at += 1;
            }
        }
    }
    merge(ranges)
}

fn merge(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_by_key(|r| (r.start, r.end));
    let mut out: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match out.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => out.push(range),
        }
    }
    out
}
