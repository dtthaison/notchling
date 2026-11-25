pub const NAME_MAX: usize = 64;

pub fn unseen(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{ad}'
                | '\u{34f}'
                | '\u{115f}'
                | '\u{1160}'
                | '\u{61c}'
                | '\u{180e}'
                | '\u{200b}'..='\u{200f}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{206f}'
                | '\u{3164}'
                | '\u{fe00}'..='\u{fe0f}'
                | '\u{feff}'
                | '\u{ffa0}'
                | '\u{fff9}'..='\u{fffb}'
                | '\u{e0000}'..='\u{e007f}'
                | '\u{e0100}'..='\u{e01ef}'
        )
}

pub fn clean(text: &str, max: usize) -> String {
    text.chars()
        .take(max)
        .map(|c| if unseen(c) { ' ' } else { c })
        .collect()
}

pub fn plain(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !unseen(*c))
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}
