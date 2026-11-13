#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Typing,
    Deleting,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub text: String,
    pub anchor: usize,
    pub head: usize,
}

pub const LIMIT: usize = 200;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    undo: Vec<State>,
    redo: Vec<State>,
    open: Option<Group>,
}

impl History {
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

    pub fn close(&mut self) {
        self.open = None;
    }

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
