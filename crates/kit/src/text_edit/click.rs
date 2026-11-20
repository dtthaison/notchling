use std::ops::Range;

use super::moves::{self, Unit};
use super::TextEdit;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Press {
    pub anchor: Range<usize>,
    pub by: Unit,
}

pub fn unit_for(click_count: usize) -> Unit {
    match click_count {
        0 | 1 => Unit::Grapheme,
        2 => Unit::Word,
        _ => Unit::Line,
    }
}

pub fn target_range(text: &str, by: Unit, index: usize) -> Range<usize> {
    match by {
        Unit::Word => moves::word_at(text, index),
        Unit::Line => moves::line_start(text, index)..moves::line_end(text, index),
        Unit::Grapheme | Unit::Document => {
            let at = moves::clamp(text, index);
            at..at
        }
    }
}

pub fn dragged(text: &str, press: &Press, index: usize) -> (usize, usize) {
    let target = target_range(text, press.by, index);
    let anchor = &press.anchor;
    if target.start < anchor.start {
        (anchor.end, target.start)
    } else if target.end > anchor.end {
        (anchor.start, target.end)
    } else {
        (anchor.start, anchor.end)
    }
}

impl TextEdit {
    pub fn press_at(&mut self, index: usize, click_count: usize, extend: bool) -> Press {
        let by = unit_for(click_count);
        let anchor = target_range(self.text(), by, index);
        if by == Unit::Grapheme && extend {
            self.select_to(index);
        } else {
            self.set_selection(anchor.start, anchor.end);
        }
        Press { anchor, by }
    }

    pub fn drag_to(&mut self, press: &Press, index: usize) {
        let (anchor, head) = dragged(self.text(), press, index);
        self.set_selection(anchor, head);
    }
}
