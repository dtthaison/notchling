use std::borrow::Cow;
use std::cmp::Ordering;
use std::str::Chars;

const DROPPED: [(u32, u32); 20] = [
    (0x00AD, 0x00AD),
    (0x034F, 0x034F),
    (0x061C, 0x061C),
    (0x115F, 0x1160),
    (0x17B4, 0x17B5),
    (0x180B, 0x180F),
    (0x200B, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x206F),
    (0x3164, 0x3164),
    (0xFE00, 0xFE0F),
    (0xFEFF, 0xFEFF),
    (0xFFA0, 0xFFA0),
    (0xFFF0, 0xFFFB),
    (0x1107F, 0x1107F),
    (0x13430, 0x1343F),
    (0x16FE4, 0x16FE4),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0000, 0xE0FFF),
];

pub fn is_line_break(c: char) -> bool {
    matches!(
        c,
        '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

pub fn is_dropped(c: char) -> bool {
    let n = c as u32;
    if n < 0x80 {
        return n == 0x7f || (n < 0x20 && !matches!(n, 9..=13));
    }
    if n < 0xA0 {
        return n != 0x85;
    }
    if !(0xAD..=0xE0FFF).contains(&n) {
        return false;
    }
    DROPPED
        .binary_search_by(|&(from, to)| {
            if n < from {
                Ordering::Greater
            } else if n > to {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        })
        .is_ok()
}

fn changes(c: char) -> bool {
    is_dropped(c) || is_line_break(c)
}

fn is_plain_ascii(text: &str) -> bool {
    text.bytes()
        .all(|b| matches!(b, 0x20..=0x7e | b'\t' | b'\n'))
}

pub fn as_stored(text: &str) -> Cow<'_, str> {
    if is_plain_ascii(text) || !text.chars().any(changes) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' && chars.peek() == Some(&'\n') {
            continue;
        }
        if is_line_break(c) {
            out.push('\n');
        } else if !is_dropped(c) {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

pub struct Squeezed<'a> {
    chars: Chars<'a>,
    started: bool,
    gap: bool,
    held: Option<char>,
}

impl Iterator for Squeezed<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        if let Some(c) = self.held.take() {
            return Some(c);
        }
        for c in self.chars.by_ref() {
            if is_dropped(c) {
                continue;
            }
            if c.is_whitespace() {
                self.gap = self.started;
                continue;
            }
            self.started = true;
            if std::mem::take(&mut self.gap) {
                self.held = Some(c);
                return Some(' ');
            }
            return Some(c);
        }
        None
    }
}

pub fn squeezed_chars(text: &str) -> Squeezed<'_> {
    Squeezed {
        chars: text.chars(),
        started: false,
        gap: false,
        held: None,
    }
}

pub fn squeezed(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    out.extend(squeezed_chars(text));
    out
}

pub fn same_words(a: &str, b: &str) -> bool {
    squeezed_chars(a).eq(squeezed_chars(b))
}

pub fn same_stored(a: &str, b: &str) -> bool {
    as_stored(a) == as_stored(b)
}

pub fn is_blank(text: &str) -> bool {
    squeezed_chars(text).next().is_none()
}

#[cfg(test)]
#[path = "stored_tests.rs"]
mod tests;
