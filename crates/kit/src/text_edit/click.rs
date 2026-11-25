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
        Unit::Document => 0..text.len(),
        Unit::Grapheme => {
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
        self.press(index, unit_for(click_count), extend)
    }

    pub fn press_at_masked(&mut self, index: usize, click_count: usize, extend: bool) -> Press {
        let by = match unit_for(click_count) {
            Unit::Grapheme => Unit::Grapheme,
            _ => Unit::Document,
        };
        self.press(index, by, extend)
    }

    fn press(&mut self, index: usize, by: Unit, extend: bool) -> Press {
        if by == Unit::Grapheme && extend {
            let from = self.anchor;
            self.select_to(index);
            return Press {
                anchor: from..from,
                by,
            };
        }
        let anchor = target_range(self.text(), by, index);
        self.set_selection(anchor.start, anchor.end);
        Press { anchor, by }
    }

    pub fn drag_to(&mut self, press: &Press, index: usize) {
        let (anchor, head) = dragged(self.text(), press, index);
        self.set_selection(anchor, head);
    }
}
