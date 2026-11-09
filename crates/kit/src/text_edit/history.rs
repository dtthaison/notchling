//! Undo and redo for [`super::TextEdit`]: snapshots of the text and selection, grouped the way a
//! Mac text view groups them. Keystrokes of one kind in a row (typing, or deleting) are one step;
//! a caret move, a paste, a cut or a change of kind closes the group. A composition (IME marked
//! text) counts as typing.

/// What kind of edit an open group holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Typing,
    Deleting,
}

/// The text and selection before an edit, as undo puts them back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub text: String,
    pub anchor: usize,
    pub head: usize,
}

/// How many steps are kept; past that the oldest goes.
pub const LIMIT: usize = 200;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    undo: Vec<State>,
    redo: Vec<State>,
    /// The group the last edit belonged to, while nothing closed it.
    open: Option<Group>,
}

impl History {
    /// Records the state before an edit. An edit in the open group adds to the last step instead
    /// of starting one; `None` is a step of its own and closes whatever was open.
    pub fn record(&mut self, before: State, group: Option<Group>) {
        self.redo.clear();
        if group.is_some() && group == self.open {
            return;
        }
        self.open = group;
        self.undo.push(before);
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
    }

    /// Closes the open group, so the next edit starts a step (the caret moved).
    pub fn close(&mut self) {
        self.open = None;
    }

    /// Everything is forgotten (a new text was set).
    pub fn clear(&mut self) {
        *self = History::default();
    }

    /// The state to put back for undo, given the current one (kept for redo).
    pub fn undo(&mut self, current: State) -> Option<State> {
        self.open = None;
        let before = self.undo.pop()?;
        self.redo.push(current);
        Some(before)
    }

    pub fn redo(&mut self, current: State) -> Option<State> {
        self.open = None;
        let after = self.redo.pop()?;
        self.undo.push(current);
        Some(after)
    }
}
