//! Text selected across a conversation's messages, as plain positions: each piece of text drawn is
//! a block, in the order it's drawn, and an end of the selection is a byte in one of them. The
//! view (`view::select`) turns presses into these and paints what [`range_in`] says.

use std::ops::Range;

/// One end of a selection: the `order`th block drawn, byte `index` into its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct At {
    pub order: usize,
    pub index: usize,
}

/// The two ends in reading order, whichever way the pointer went.
pub fn ordered(anchor: At, focus: At) -> (At, At) {
    if focus < anchor {
        (focus, anchor)
    } else {
        (anchor, focus)
    }
}

/// The bytes of block `order` (of `len` bytes) between `start` and `end`, or `None` when none of
/// it is selected.
pub fn range_in(start: At, end: At, order: usize, len: usize) -> Option<Range<usize>> {
    if order < start.order || order > end.order {
        return None;
    }
    let from = if order == start.order { start.index } else { 0 };
    let to = if order == end.order { end.index } else { len };
    let (from, to) = (from.min(len), to.min(len));
    (from < to).then_some(from..to)
}

/// What a copy takes: each selected block's part, a line apart.
pub fn copied(blocks: &[&str], start: At, end: At) -> String {
    blocks
        .iter()
        .enumerate()
        .filter_map(|(order, text)| {
            let range = range_in(start, end, order, text.len())?;
            let range = floor(text, range.start)..floor(text, range.end);
            Some(&text[range])
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `index` moved back to the start of the character it falls in, so a slice never cuts one.
pub fn floor(text: &str, index: usize) -> usize {
    let mut i = index.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// The word around byte `index` (letters, digits and `_`), or the one character there when it's
/// neither (a space, a comma).
pub fn word_at(text: &str, index: usize) -> Range<usize> {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let i = floor(text, index);
    // At the very end, the word before it.
    let i = if i == text.len() {
        text[..i].char_indices().last().map_or(0, |(j, _)| j)
    } else {
        i
    };
    let Some(here) = text[i..].chars().next() else {
        return 0..0;
    };
    if !word(here) {
        return i..i + here.len_utf8();
    }
    let start = text[..i]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| word(c))
        .last()
        .map_or(i, |(j, _)| j);
    let end = text[i..]
        .char_indices()
        .find(|&(_, c)| !word(c))
        .map_or(text.len(), |(j, _)| i + j);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(order: usize, index: usize) -> At {
        At { order, index }
    }

    #[test]
    fn a_drag_backwards_selects_the_same_text() {
        assert_eq!(ordered(at(2, 5), at(0, 3)), (at(0, 3), at(2, 5)));
        assert_eq!(ordered(at(1, 9), at(1, 2)), (at(1, 2), at(1, 9)));
    }

    #[test]
    fn the_first_block_from_the_start_the_middle_whole_the_last_to_the_end() {
        let (s, e) = (at(1, 4), at(3, 2));
        assert_eq!(range_in(s, e, 0, 10), None);
        assert_eq!(range_in(s, e, 1, 10), Some(4..10));
        assert_eq!(range_in(s, e, 2, 7), Some(0..7));
        assert_eq!(range_in(s, e, 3, 10), Some(0..2));
        assert_eq!(range_in(s, e, 4, 10), None);
        assert_eq!(
            range_in(at(1, 3), at(1, 3), 1, 10),
            None,
            "a click selects nothing"
        );
        assert_eq!(
            range_in(at(0, 2), at(0, 50), 0, 6),
            Some(2..6),
            "past the end"
        );
    }

    #[test]
    fn a_copy_takes_part_of_one_message_or_several_a_line_apart() {
        let blocks = ["Hello there", "Second para", "Third"];
        assert_eq!(copied(&blocks, at(0, 6), at(0, 11)), "there");
        assert_eq!(
            copied(&blocks, at(0, 6), at(2, 3)),
            "there\nSecond para\nThi"
        );
        assert_eq!(copied(&blocks, at(1, 2), at(1, 2)), "");
    }

    #[test]
    fn a_double_click_takes_the_word_under_it() {
        let text = "Make ONE size_chart, please";
        assert_eq!(&text[word_at(text, 6)], "ONE");
        assert_eq!(&text[word_at(text, 12)], "size_chart");
        assert_eq!(&text[word_at(text, 0)], "Make");
        assert_eq!(&text[word_at(text, text.len())], "please");
        assert_eq!(
            &"Chào bạn"[word_at("Chào bạn", 3)],
            "Chào",
            "letters past ASCII are one word"
        );
        assert_eq!(word_at(text, 4), 4..5, "a space on its own");
    }

    #[test]
    fn a_copy_never_cuts_a_character() {
        let blocks = ["Chào bạn"];
        // Byte 3 is inside "à" (bytes 2..4).
        assert_eq!(copied(&blocks, at(0, 0), at(0, 3)), "Ch");
        assert_eq!(floor("à", 1), 0);
    }
}
