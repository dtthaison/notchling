pub fn build_id() -> Option<&'static str> {
    option_env!("NOTCHLING_BUILD").and_then(accepted)
}

pub fn accepted(text: &str) -> Option<&str> {
    let hex = text.strip_suffix("-dirty").unwrap_or(text);
    let plain = (7..=40).contains(&hex.len())
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    plain.then_some(text)
}
