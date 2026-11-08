//! Character widths, as `TerminalScreen.swift` decides them: a short table of wide ranges (CJK,
//! Hangul, fullwidth forms, the common emoji blocks) and the combining characters that join the cell
//! before them. Deliberately the Swift table, not a full East Asian Width table, so both emulators
//! lay out Claude Code's screen the same.

use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

/// Marks and format characters (joiners), variation selectors and skin-tone modifiers.
pub fn is_combining(c: char) -> bool {
    match c.general_category() {
        GeneralCategory::NonspacingMark
        | GeneralCategory::EnclosingMark
        | GeneralCategory::SpacingMark
        | GeneralCategory::Format => true,
        _ => matches!(u32::from(c), 0xFE00..=0xFE0F | 0x1F3FB..=0x1F3FF),
    }
}

/// Takes two cells.
pub fn is_wide(c: char) -> bool {
    matches!(
        u32::from(c),
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F900..=0x1F9FF
            | 0x1FA70..=0x1FAFF
            | 0x20000..=0x3FFFD
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_swift_tables() {
        assert!(is_wide('日') && is_wide('😀') && is_wide('한'));
        assert!(!is_wide('a') && !is_wide('é') && !is_wide('❯'));
        assert!(is_combining('\u{0301}') && is_combining('\u{200D}') && is_combining('\u{FE0F}'));
        assert!(
            is_combining('\u{1F3FB}'),
            "a skin tone joins the emoji before it"
        );
        assert!(!is_combining('e') && !is_combining(' '));
    }
}
