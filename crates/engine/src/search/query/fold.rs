use super::fold_table::{DROPPED, TABLE};

fn dropped(c: char) -> bool {
    DROPPED
        .binary_search_by(|&(from, to)| {
            if c < from {
                std::cmp::Ordering::Greater
            } else if c > to {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

pub fn fold_char(c: char) -> Option<char> {
    if c.is_ascii() {
        return Some(c.to_ascii_lowercase());
    }
    let base = if let Ok(i) = TABLE.binary_search_by_key(&c, |&(from, _)| from) {
        TABLE[i].1
    } else if dropped(c) {
        return None;
    } else {
        c.to_lowercase().next()?
    };
    Some(if base == 'đ' { 'd' } else { base })
}

pub fn fold(text: &str) -> String {
    text.chars().filter_map(fold_char).collect()
}

pub fn words(text: &str) -> Vec<String> {
    fold(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}
