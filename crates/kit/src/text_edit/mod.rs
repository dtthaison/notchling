mod history;
mod moves;
#[cfg(test)]
mod tests;

use std::ops::Range;

use history::{Group, History, State};
pub use moves::Unit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnKey {
    Newline,
    Submit,
}

pub fn return_key(shift: bool, alt: bool) -> ReturnKey {
    if shift || alt {
        ReturnKey::Newline
    } else {
        ReturnKey::Submit
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection16 {
    pub range: Range<usize>,
    pub reversed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextEdit {
    text: String,
    anchor: usize,
    head: usize,
    marked: Option<Range<usize>>,
    stale_composition: Option<String>,
    history: History,
}

impl TextEdit {
    pub fn new(text: &str) -> TextEdit {
        TextEdit {
            text: text.to_owned(),
            anchor: text.len(),
            head: text.len(),
            marked: None,
            stale_composition: None,
            history: History::default(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn set_text(&mut self, text: &str) {
        let stale = self
            .marked
            .as_ref()
            .map(|m| self.text[m.clone()].to_owned());
        *self = TextEdit::new(text);
        self.stale_composition = stale;
    }

    pub fn caret(&self) -> usize {
        self.head
    }

    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_reversed(&self) -> bool {
        self.head < self.anchor
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }

    pub fn marked(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    fn state(&self) -> State {
        State {
            text: self.text.clone(),
            anchor: self.anchor,
            head: self.head,
        }
    }

    fn restore(&mut self, state: State) {
        self.text = state.text;
        self.anchor = state.anchor;
        self.head = state.head;
        self.marked = None;
    }

    pub fn snap(&self, index: usize) -> usize {
        moves::clamp(&self.text, index)
    }

    pub fn set_selection(&mut self, anchor: usize, head: usize) {
        self.anchor = moves::clamp(&self.text, anchor);
        self.head = moves::clamp(&self.text, head);
        self.history.close();
    }

    pub fn move_to(&mut self, index: usize) {
        self.set_selection(index, index);
    }

    pub fn select_to(&mut self, index: usize) {
        self.set_selection(self.anchor, index);
    }

    pub fn select_all(&mut self) {
        self.set_selection(0, self.text.len());
    }

    pub fn word_range_at(&self, index: usize) -> Range<usize> {
        moves::word_at(&self.text, index)
    }

    pub fn line_range_at(&self, index: usize) -> Range<usize> {
        moves::line_start(&self.text, index)..moves::line_end(&self.text, index)
    }

    pub fn select_word_at(&mut self, index: usize) {
        let range = self.word_range_at(index);
        self.set_selection(range.start, range.end);
    }

    pub fn select_line_at(&mut self, index: usize) {
        let range = self.line_range_at(index);
        self.set_selection(range.start, range.end);
    }

    pub fn move_left(&mut self, unit: Unit, extend: bool) {
        let selection = self.selection();
        let target = if !extend && unit == Unit::Grapheme && !selection.is_empty() {
            selection.start
        } else {
            moves::previous(&self.text, self.head, unit)
        };
        self.go(target, extend);
    }

    pub fn move_right(&mut self, unit: Unit, extend: bool) {
        let selection = self.selection();
        let target = if !extend && unit == Unit::Grapheme && !selection.is_empty() {
            selection.end
        } else {
            moves::next(&self.text, self.head, unit)
        };
        self.go(target, extend);
    }

    fn go(&mut self, target: usize, extend: bool) {
        if extend {
            self.select_to(target);
        } else {
            self.move_to(target);
        }
    }

    fn splice(&mut self, range: Range<usize>, text: &str, group: Option<Group>) {
        self.stale_composition = None;
        self.history.record(self.state(), group);
        self.text.replace_range(range.clone(), text);
        let after = range.start + text.len();
        self.anchor = after;
        self.head = after;
        self.marked = None;
    }

    pub fn insert(&mut self, text: &str) {
        self.splice(self.selection(), text, Some(Group::Typing));
    }

    pub fn paste(&mut self, text: &str) {
        self.splice(self.selection(), text, None);
    }

    pub fn delete_backward(&mut self, unit: Unit) {
        let selection = self.selection();
        if !selection.is_empty() {
            return self.splice(selection, "", None);
        }
        let start = moves::previous(&self.text, self.head, unit);
        if start < self.head {
            self.splice(start..self.head, "", Some(Group::Deleting));
        }
    }

    pub fn delete_forward(&mut self, unit: Unit) {
        let selection = self.selection();
        if !selection.is_empty() {
            return self.splice(selection, "", None);
        }
        let end = moves::next(&self.text, self.head, unit);
        if end > self.head {
            self.splice(self.head..end, "", Some(Group::Deleting));
        }
    }

    pub fn cut(&mut self) -> Option<String> {
        let selection = self.selection();
        if selection.is_empty() {
            return None;
        }
        let text = self.text[selection.clone()].to_owned();
        self.splice(selection, "", None);
        Some(text)
    }

    pub fn undo(&mut self) -> bool {
        match self.history.undo(self.state()) {
            Some(before) => {
                self.restore(before);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.history.redo(self.state()) {
            Some(after) => {
                self.restore(after);
                true
            }
            None => false,
        }
    }

    pub fn utf8_to_utf16(&self, index: usize) -> usize {
        moves::to_utf16(&self.text, index)
    }

    pub fn utf16_to_utf8(&self, offset: usize) -> usize {
        moves::from_utf16(&self.text, offset)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let start = self.utf16_to_utf8(range.start);
        start..self.utf16_to_utf8(range.end).max(start)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.utf8_to_utf16(range.start)..self.utf8_to_utf16(range.end)
    }

    pub fn selection_utf16(&self) -> Selection16 {
        Selection16 {
            range: self.range_to_utf16(&self.selection()),
            reversed: self.is_reversed(),
        }
    }

    pub fn marked_utf16(&self) -> Option<Range<usize>> {
        self.marked.as_ref().map(|m| self.range_to_utf16(m))
    }

    pub fn text_for_range_utf16(&self, range: Range<usize>) -> (String, Range<usize>) {
        let utf8 = self.range_from_utf16(&range);
        (
            self.text[utf8.clone()].to_owned(),
            self.range_to_utf16(&utf8),
        )
    }

    fn target(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        range_utf16
            .map(|r| self.range_from_utf16(&r))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection())
    }

    pub fn replace_text(&mut self, range_utf16: Option<Range<usize>>, text: &str) {
        let stale = self.stale_composition.take();
        if range_utf16.is_none() && self.marked.is_none() && stale.as_deref() == Some(text) {
            return;
        }
        let range = self.target(range_utf16);
        self.splice(range, text, Some(Group::Typing));
    }

    pub fn replace_and_mark(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        selected_utf16: Option<Range<usize>>,
    ) {
        let range = self.target(range_utf16);
        self.splice(range.clone(), text, Some(Group::Typing));
        if text.is_empty() {
            return;
        }
        self.marked = Some(range.start..range.start + text.len());
        if let Some(selected) = selected_utf16 {
            let start = range.start + moves::from_utf16(text, selected.start);
            let end = range.start + moves::from_utf16(text, selected.end);
            self.anchor = start;
            self.head = end;
        }
    }

    pub fn unmark(&mut self) {
        self.marked = None;
    }
}
