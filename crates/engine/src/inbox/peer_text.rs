const WORD_MAX: usize = 20;
const GAP: &str = "…";

fn is_format(c: char) -> bool {
    matches!(
        c as u32,
        0xAD | 0x600..=0x605
            | 0x61C
            | 0x6DD
            | 0x70F
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0xE0001
            | 0xE0020..=0xE007F
            | 0x110BD
            | 0x1D173..=0x1D17A
    )
}

fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() && !is_format(*c))
        .collect()
}

fn names_a_path(word: &str) -> bool {
    if word.contains(['~', '\\']) || word.contains(":/") || word.contains("..") {
        return true;
    }
    let word = word.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '/');
    if let Some(slash) = word.find('/') {
        if !word[..slash].chars().any(|c| c.is_alphabetic()) {
            return true;
        }
    }
    word.split('/').filter(|part| !part.is_empty()).count() >= 2
}

fn cut(text: String, max: usize) -> String {
    if text.chars().count() <= max {
        return text;
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub fn clean_note(text: &str, max: usize) -> Option<String> {
    let text = plain(text);
    let mut words: Vec<&str> = Vec::new();
    for word in text.split_whitespace() {
        let word = if word.chars().count() >= WORD_MAX || names_a_path(word) {
            GAP
        } else {
            word
        };
        if word == GAP && words.last() == Some(&GAP) {
            continue;
        }
        words.push(word);
    }
    let joined = words.join(" ");
    if joined.is_empty() || joined == GAP {
        return None;
    }
    Some(cut(joined, max))
}

fn is_uuid(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(part, len)| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()))
}

pub fn clean_place(text: &str, max: usize, allow_slash: bool, allow_uuid: bool) -> Option<String> {
    let text = plain(text);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
    }
    if allow_uuid && is_uuid(&text) {
        return Some(text);
    }
    let slashes = text.chars().filter(|c| *c == '/').count();
    let refused = text.contains("..")
        || text.contains(":/")
        || slashes > 1
        || (slashes == 1 && !allow_slash)
        || !text.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '#' | ':' | '/')
        })
        || text
            .split(' ')
            .any(|word| word.starts_with(['/', '~']) || word.chars().count() >= WORD_MAX);
    if refused {
        return None;
    }
    Some(cut(text, max))
}
