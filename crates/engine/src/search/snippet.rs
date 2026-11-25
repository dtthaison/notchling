use super::query::{fold, fold_char};
use super::rank::{folded_words, IndexedSession};
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
    let words = folded_words(words);
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let lower = session.lower.as_str();
    let mut position = None;
    if words.len() > 1 {
        position = find(&words.join(" "), lower, 0);
    }
    if position.is_none() {
        position = words.iter().filter_map(|w| find(w, lower, 0)).min();
    }
    let position = original_offset(&session.text, position?);
    let bytes = session.text.as_bytes();
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

fn original_offset(text: &str, folded_at: usize) -> usize {
    let mut folded = 0;
    for (at, c) in text.char_indices() {
        if folded >= folded_at {
            return at;
        }
        if let Some(f) = fold_char(c) {
            folded += f.len_utf8();
        }
    }
    text.len()
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
    let mut kept: Vec<(char, usize, usize)> = Vec::new();
    for (at, c) in snippet.char_indices() {
        match fold_char(c) {
            Some(f) => kept.push((f, at, at + c.len_utf8())),
            None => {
                if let Some(last) = kept.last_mut() {
                    last.2 = at + c.len_utf8();
                }
            }
        }
    }
    let lower: Vec<char> = kept.iter().map(|k| k.0).collect();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let words: BTreeSet<String> = words.iter().map(|w| fold(w.as_ref())).collect();
    for word in &words {
        let needle: Vec<char> = word.chars().collect();
        if needle.is_empty() {
            continue;
        }
        let mut at = 0;
        while at + needle.len() <= lower.len() {
            if lower[at..at + needle.len()] == needle[..] {
                ranges.push(kept[at].1..kept[at + needle.len() - 1].2);
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
