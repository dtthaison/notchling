use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Grapheme,
    Word,
    Line,
    Document,
}

pub fn clamp(text: &str, index: usize) -> usize {
    let mut i = index.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

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

fn is_word(segment: &str) -> bool {
    segment.chars().any(char::is_alphanumeric)
}

pub fn line_start(text: &str, index: usize) -> usize {
    text[..clamp(text, index)]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0)
}

pub fn line_end(text: &str, index: usize) -> usize {
    let index = clamp(text, index);
    text[index..]
        .find('\n')
        .map(|i| index + i)
        .unwrap_or(text.len())
}

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

pub fn to_utf16(text: &str, index: usize) -> usize {
    text[..clamp(text, index)].encode_utf16().count()
}

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
