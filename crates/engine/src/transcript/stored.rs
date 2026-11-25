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

const FLAG_NAMES: [&str; 3] = ["gbeng", "gbsct", "gbwls"];
const FLAG_BASE: char = '\u{1F3F4}';
const TAG_LAST: u32 = 0xE007F;

fn kept_when_typed(c: char) -> bool {
    matches!(
        c as u32,
        0x034F
            | 0x061C
            | 0x17B4..=0x17B5
            | 0x180B..=0x180F
            | 0x200C..=0x200F
            | 0xFE00..=0xFE0F
            | 0x1107F
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0xE0100..=0xE01EF
    )
}

fn removed_when_typed(c: char) -> bool {
    is_dropped(c) && !kept_when_typed(c)
}

fn needs_script_spaces(c: char) -> bool {
    matches!(
        c as u32,
        0x0E00..=0x0EFF | 0x1000..=0x109F | 0x1780..=0x17FF | 0x19E0..=0x19FF | 0xA9E0..=0xA9FF | 0xAA60..=0xAA7F
    )
}

fn flag_end(chars: &[char], at: usize) -> Option<usize> {
    let mut name = String::new();
    for (index, c) in chars.iter().enumerate().skip(at + 1).take(6) {
        let n = *c as u32;
        if n == TAG_LAST {
            return FLAG_NAMES.contains(&name.as_str()).then_some(index);
        }
        if !(0xE0061..=0xE007A).contains(&n) {
            return None;
        }
        name.push(char::from_u32(n - 0xE0000)?);
    }
    None
}

pub fn precleaned(text: &str) -> Cow<'_, str> {
    if is_plain_ascii(text)
        || !text
            .chars()
            .any(|c| removed_when_typed(c) || is_line_break(c))
    {
        return Cow::Borrowed(text);
    }
    let chars: Vec<char> = text.chars().collect();
    let spaced = chars.iter().any(|c| needs_script_spaces(*c));
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        if c == FLAG_BASE {
            if let Some(end) = flag_end(&chars, at) {
                out.extend(&chars[at..=end]);
                at = end + 1;
                continue;
            }
        }
        if c == '\r' && chars.get(at + 1) == Some(&'\n') {
            at += 1;
            continue;
        }
        if is_line_break(c) {
            out.push(' ');
        } else if c == '\u{200B}' {
            if spaced {
                out.push(c);
            }
        } else if !removed_when_typed(c) {
            out.push(c);
        }
        at += 1;
    }
    Cow::Owned(out)
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
