//! Small text helpers the readers share. Counting is in Unicode scalars (`chars`), where the Swift
//! app counted grapheme clusters; the two differ only for combined emoji and accents.

/// Characters Swift's `CharacterSet.newlines` holds.
fn is_newline(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

pub fn trim_newlines(text: &str) -> &str {
    text.trim_matches(is_newline)
}

/// The first `n` characters.
pub fn prefix_chars(text: &str, n: usize) -> &str {
    match text.char_indices().nth(n) {
        Some((i, _)) => &text[..i],
        None => text,
    }
}

/// The last `n` characters.
pub fn suffix_chars(text: &str, n: usize) -> &str {
    let count = text.chars().count();
    if count <= n {
        return text;
    }
    match text.char_indices().nth(count - n) {
        Some((i, _)) => &text[i..],
        None => text,
    }
}

/// The text between two tags. `trim: false` keeps leading spaces, which shell output lines up with.
pub fn between(text: &str, open: &str, close: &str, trim: bool) -> Option<String> {
    let a = text.find(open)? + open.len();
    let b = a + text[a..].find(close)?;
    let inner = &text[a..b];
    Some(if trim {
        inner.trim().to_string()
    } else {
        inner.to_string()
    })
}

/// Removes terminal color and cursor sequences (`ESC [ ... letter`).
pub fn strip_ansi(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\u{1b}' && chars.get(i + 1) == Some(&'[') {
            let mut j = i + 2;
            while j < chars.len()
                && (chars[j].is_ascii_digit() || chars[j] == ';' || chars[j] == '?')
            {
                j += 1;
            }
            if j < chars.len() && chars[j].is_ascii_alphabetic() {
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Text pasted into a prompt shows as "[Pasted text +N lines]", like the terminal shows it.
pub fn collapse_pastes(text: &str) -> String {
    if !text.contains("<pasted_content") {
        return text.to_string();
    }
    let mut out = text.to_string();
    while let Some(open) = out.find("<pasted_content") {
        let after_open = open + "<pasted_content".len();
        let Some(open_end) = out[after_open..].find('>').map(|i| after_open + i + 1) else {
            break;
        };
        let Some(close) = out[open_end..]
            .find("</pasted_content")
            .map(|i| open_end + i)
        else {
            break;
        };
        let after_close = close + "</pasted_content".len();
        let Some(close_end) = out[after_close..].find('>').map(|i| after_close + i + 1) else {
            break;
        };
        let body = trim_newlines(&out[open_end..close]);
        let count = body.split('\n').count();
        let replacement = if count > 1 {
            format!("[Pasted text +{count} lines]")
        } else {
            "[Pasted text]".to_string()
        };
        out.replace_range(open..close_end, &replacement);
    }
    out
}

/// The last path component, the way `NSString.lastPathComponent` reads it.
pub fn last_path_component(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return if path.is_empty() { "" } else { "/" };
    }
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

/// The first non-empty line.
pub fn first_line(text: &str) -> &str {
    text.split('\n').find(|l| !l.is_empty()).unwrap_or(text)
}
