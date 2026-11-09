//! Boundaries in a text: graphemes (what one caret step is), words (what Option-arrows step
//! over), paragraphs (what ⌘-arrows go to without a layout) and the document. Every index is a
//! UTF-8 byte offset on a character boundary.

use unicode_segmentation::UnicodeSegmentation;

/// How far a move or a delete reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// One user-perceived character: a letter with its combining marks, an emoji sequence.
    Grapheme,
    /// To the start of the previous word or the end of the next one.
    Word,
    /// To the start or the end of the paragraph (the text between two newlines).
    Line,
    /// To the start or the end of the text.
    Document,
}

/// Snaps `index` onto a character boundary at or before it, inside the text.
pub fn clamp(text: &str, index: usize) -> usize {
    let mut i = index.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// The boundary before `index`, by unit; `0` at the start.
pub fn previous(text: &str, index: usize, unit: Unit) -> usize {
    let index = clamp(text, index);
    match unit {
        Unit::Grapheme => text[..index]
            .grapheme_indices(true)
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0),
        Unit::Word => text
            .split_word_bound_indices()
            .rev()
            .find(|(start, word)| *start < index && is_word(word))
            .map(|(start, _)| start)
            .unwrap_or(0),
        Unit::Line => line_start(text, index),
        Unit::Document => 0,
    }
}

/// The boundary after `index`, by unit; the text's end at the end.
pub fn next(text: &str, index: usize, unit: Unit) -> usize {
    let index = clamp(text, index);
    match unit {
        Unit::Grapheme => text[index..]
            .graphemes(true)
            .next()
            .map(|g| index + g.len())
            .unwrap_or(text.len()),
        Unit::Word => text
            .split_word_bound_indices()
            .find(|(start, word)| start + word.len() > index && is_word(word))
            .map(|(start, word)| start + word.len())
            .unwrap_or(text.len()),
        Unit::Line => line_end(text, index),
        Unit::Document => text.len(),
    }
}

/// A word, as Option-arrows see one: a run with a letter or digit in it, not spaces or
/// punctuation.
fn is_word(segment: &str) -> bool {
    segment.chars().any(char::is_alphanumeric)
}

/// The start of the paragraph holding `index` (just after the previous newline).
pub fn line_start(text: &str, index: usize) -> usize {
    text[..clamp(text, index)]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0)
}

/// The end of the paragraph holding `index` (the next newline, or the text's end).
pub fn line_end(text: &str, index: usize) -> usize {
    let index = clamp(text, index);
    text[index..]
        .find('\n')
        .map(|i| index + i)
        .unwrap_or(text.len())
}

/// The word-boundary run holding `index` (a double-click's pick): a word, or the spaces or
/// punctuation between two. At the very end, the last run.
pub fn word_at(text: &str, index: usize) -> std::ops::Range<usize> {
    let index = clamp(text, index);
    let mut last = 0..0;
    for (start, segment) in text.split_word_bound_indices() {
        let range = start..start + segment.len();
        if range.contains(&index) {
            return range;
        }
        last = range;
    }
    last
}

/// UTF-16 offset of a UTF-8 index.
pub fn to_utf16(text: &str, index: usize) -> usize {
    text[..clamp(text, index)].encode_utf16().count()
}

/// UTF-8 index of a UTF-16 offset. An offset inside a surrogate pair, or past the end, lands on
/// the next character boundary (or the end).
pub fn from_utf16(text: &str, offset: usize) -> usize {
    let mut seen = 0;
    for (i, ch) in text.char_indices() {
        if seen >= offset {
            return i;
        }
        seen += ch.len_utf16();
    }
    text.len()
}
