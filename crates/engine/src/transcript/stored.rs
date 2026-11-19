use std::borrow::Cow;

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
    if n < 0xA0 {
        return !is_line_break(c) && ((n < 0x20 && n != 9 && n != 10) || n >= 0x7F);
    }
    DROPPED.iter().any(|(from, to)| (*from..=*to).contains(&n))
}

fn changes(c: char) -> bool {
    is_line_break(c) || is_dropped(c)
}

pub fn as_stored(text: &str) -> Cow<'_, str> {
    if !text.chars().any(changes) {
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

pub fn squeezed(text: &str) -> String {
    as_stored(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
#[path = "stored_tests.rs"]
mod tests;
