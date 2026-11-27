pub fn is_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

pub fn valid_model(text: &str) -> bool {
    matches!(text, "flash_lite" | "flash" | "pro")
        || (!text.is_empty()
            && text.len() <= 64
            && text.starts_with(|c: char| c.is_ascii_alphanumeric())
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')))
}
