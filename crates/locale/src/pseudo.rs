pub(crate) fn accent(text: &str) -> String {
    text.chars().map(accent_char).collect()
}

fn accent_char(c: char) -> char {
    match c {
        'a' => '\u{e0}',
        'c' => '\u{e7}',
        'e' => '\u{e9}',
        'o' => '\u{f4}',
        'u' => '\u{fb}',
        'n' => '\u{f1}',
        't' => '\u{163}',
        's' => '\u{161}',
        'A' => '\u{c0}',
        'C' => '\u{c7}',
        'E' => '\u{c9}',
        'O' => '\u{d4}',
        'U' => '\u{db}',
        'N' => '\u{d1}',
        'T' => '\u{162}',
        'S' => '\u{160}',
        other => other,
    }
}

pub(crate) fn wrap(text: &str) -> String {
    let count = text.chars().count();
    let target = (count * 14).div_ceil(10);
    let mut out = String::with_capacity(text.len() + target - count + 6);
    out.push('\u{27e6}');
    out.push_str(text);
    for _ in count..target {
        out.push('~');
    }
    out.push('\u{27e7}');
    out
}

pub(crate) fn transform(text: &str) -> String {
    wrap(&accent(text))
}
