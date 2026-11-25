use super::words;

fn is_uuid(token: &str) -> bool {
    let parts: Vec<&str> = token.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(p, n)| p.len() == n && p.chars().all(|c| c.is_ascii_hexdigit()))
}

fn is_hex_token(token: &str) -> bool {
    token.len() >= 7
        && token.chars().all(|c| c.is_ascii_hexdigit())
        && token.chars().any(|c| c.is_ascii_digit())
}

fn is_artifact_id(query: &str) -> bool {
    query.len() == 22
        && query.chars().all(|c| c.is_ascii_alphanumeric())
        && query.chars().any(|c| !c.is_ascii_lowercase())
}

fn is_url(query: &str) -> bool {
    let lower = query.to_lowercase();
    lower.contains("://")
        || lower.starts_with("www.")
        || (lower.contains("claude.ai/") && lower.contains("artifact"))
}

fn is_path(query: &str) -> bool {
    query.starts_with('~') || (query.contains('/') && !query.chars().any(char::is_whitespace))
}

fn is_quoted(query: &str) -> bool {
    let mut open = None;
    for (i, c) in query.char_indices() {
        match (open, c) {
            (None, '"' | '\u{201c}') => open = Some(i + c.len_utf8()),
            (Some(start), '"' | '\u{201d}') if i > start => return true,
            _ => {}
        }
    }
    false
}

pub fn text_only(query: &str) -> bool {
    let query = query.trim();
    if is_url(query) || is_artifact_id(query) || is_path(query) || is_quoted(query) {
        return true;
    }
    if query
        .split_whitespace()
        .any(|t| is_uuid(t) || is_hex_token(t))
    {
        return true;
    }
    words(query).iter().all(|w| w.chars().count() <= 2)
}
