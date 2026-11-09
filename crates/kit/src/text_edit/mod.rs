//! The text editor's model, with no GPUI in it: a buffer, a selection (an anchor and the caret),
//! the input method's marked text, undo groups, and the moves and deletes a Mac text view has.
//! Indices are UTF-8 byte offsets; the `*_utf16` methods speak the platform input handler's
//! UTF-16 units, since that is what AppKit's `NSTextInputClient` sends. What needs a layout
//! (up and down across wrapped lines, ⌘-arrows to the edge of a wrapped row) lives in
//! `ui::text_editor`, which calls [`TextEdit::move_to`] and [`TextEdit::select_to`].

mod history;
mod moves;
#[cfg(test)]
mod tests;

use std::ops::Range;

use history::{Group, History, State};
pub use moves::Unit;

/// What Return does in a box that can hold several lines: with Shift or Option it adds a line,
/// bare (or with ⌘ or Ctrl) it submits.
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

/// A selection in UTF-16 units, as the platform asks for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection16 {
    pub range: Range<usize>,
    /// The caret is at the start.
    pub reversed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextEdit {
    text: String,
    /// Where the selection began.
    anchor: usize,
    /// The caret.
    head: usize,
    /// Text the input method is still composing, drawn underlined.
    marked: Option<Range<usize>>,
    /// What was composing when the owner cleared the box (it sent the text): the input method
    /// may still commit it afterwards, and that one commit is dropped.
    stale_composition: Option<String>,
    history: History,
}

impl TextEdit {
    /// The caret at the end, nothing selected.
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

    /// Another text altogether (another session's draft, or nothing after a send): the caret at
    /// its end, no history. A composition in progress is remembered, so its commit, if the input
    /// method still sends it, doesn't put the sent words back (see [`TextEdit::replace_text`]).
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

    /// The selection, start before end; empty at the caret.
    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    /// The caret is at the selection's start (it was extended leftwards).
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

    /// `index` on a character boundary, at or before it, inside the text.
    pub fn snap(&self, index: usize) -> usize {
        moves::clamp(&self.text, index)
    }

    /// Selects from `anchor` to `head` (the caret); both snap to character boundaries. Closes the
    /// undo group, as any caret move does.
    pub fn set_selection(&mut self, anchor: usize, head: usize) {
        self.anchor = moves::clamp(&self.text, anchor);
        self.head = moves::clamp(&self.text, head);
        self.history.close();
    }

    /// Puts the caret at `index`, nothing selected.
    pub fn move_to(&mut self, index: usize) {
        self.set_selection(index, index);
    }

    /// Moves the caret to `index`, keeping the anchor (Shift-click, Shift-arrows).
    pub fn select_to(&mut self, index: usize) {
        self.set_selection(self.anchor, index);
    }

    pub fn select_all(&mut self) {
        self.set_selection(0, self.text.len());
    }

    /// The word-boundary run at `index`: a double-click's pick.
    pub fn word_range_at(&self, index: usize) -> Range<usize> {
        moves::word_at(&self.text, index)
    }

    /// The paragraph holding `index`, without its newline.
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

    /// Left (or ⌥/⌘ Left). Without `extend`, a selection collapses to its start for a grapheme
    /// step; the other units move from the caret.
    pub fn move_left(&mut self, unit: Unit, extend: bool) {
        let selection = self.selection();
        let target = if !extend && unit == Unit::Grapheme && !selection.is_empty() {
            selection.start
        } else {
            moves::previous(&self.text, self.head, unit)
        };
        self.go(target, extend);
    }

    /// Right (or ⌥/⌘ Right), the mirror of [`TextEdit::move_left`].
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

    /// Replaces `range` with `text`, records the step in `group`, and puts the caret after it.
    fn splice(&mut self, range: Range<usize>, text: &str, group: Option<Group>) {
        self.stale_composition = None;
        self.history.record(self.state(), group);
        self.text.replace_range(range.clone(), text);
        let after = range.start + text.len();
        self.anchor = after;
        self.head = after;
        self.marked = None;
    }

    /// Typed text: replaces the selection; one undo step with the keystrokes around it.
    pub fn insert(&mut self, text: &str) {
        self.splice(self.selection(), text, Some(Group::Typing));
    }

    /// Pasted text: replaces the selection; an undo step of its own.
    pub fn paste(&mut self, text: &str) {
        self.splice(self.selection(), text, None);
    }

    /// Backspace, ⌥ Backspace or ⌘ Backspace: the selection if there is one, else back by `unit`.
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

    /// Delete (fn Backspace), with ⌥ or ⌘: forward by `unit`, or the selection.
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

    /// ⌘X: removes the selection and returns it, or `None` when nothing is selected.
    pub fn cut(&mut self) -> Option<String> {
        let selection = self.selection();
        if selection.is_empty() {
            return None;
        }
        let text = self.text[selection.clone()].to_owned();
        self.splice(selection, "", None);
        Some(text)
    }

    /// ⌘Z. `false` when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        match self.history.undo(self.state()) {
            Some(before) => {
                self.restore(before);
                true
            }
            None => false,
        }
    }

    /// ⌘⇧Z.
    pub fn redo(&mut self) -> bool {
        match self.history.redo(self.state()) {
            Some(after) => {
                self.restore(after);
                true
            }
            None => false,
        }
    }

    // The platform input handler's side (NSTextInputClient), in UTF-16 units.

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

    /// The text in a UTF-16 range, and the range it really covers (snapped to characters and
    /// the text's end).
    pub fn text_for_range_utf16(&self, range: Range<usize>) -> (String, Range<usize>) {
        let utf8 = self.range_from_utf16(&range);
        (
            self.text[utf8.clone()].to_owned(),
            self.range_to_utf16(&utf8),
        )
    }

    /// The range an input method edit applies to: the one it names, else the marked text, else
    /// the selection.
    fn target(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        range_utf16
            .map(|r| self.range_from_utf16(&r))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection())
    }

    /// `insertText`: typed or committed text in `range_utf16` (see [`TextEdit::target`]). The
    /// commit of a composition the owner already cleared away is dropped, once.
    pub fn replace_text(&mut self, range_utf16: Option<Range<usize>>, text: &str) {
        let stale = self.stale_composition.take();
        if range_utf16.is_none() && self.marked.is_none() && stale.as_deref() == Some(text) {
            return;
        }
        let range = self.target(range_utf16);
        self.splice(range, text, Some(Group::Typing));
    }

    /// `setMarkedText`: the composition so far, kept marked, with the input method's selection
    /// inside it (UTF-16, relative to the new text). An empty text ends the composition.
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

    /// `unmarkText`: the composition stands as typed.
    pub fn unmark(&mut self) {
        self.marked = None;
    }
}
