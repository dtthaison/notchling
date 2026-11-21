use crate::server::AgentIdentity;
use std::cell::RefCell;

thread_local! {
    static BOUND: RefCell<Option<AgentIdentity>> = const { RefCell::new(None) };
}

pub struct Bound {
    previous: Option<AgentIdentity>,
}

pub fn bind(identity: Option<AgentIdentity>) -> Bound {
    let previous = BOUND.with(|b| std::mem::replace(&mut *b.borrow_mut(), identity));
    Bound { previous }
}

impl Drop for Bound {
    fn drop(&mut self) {
        let previous = self.previous.take();
        BOUND.with(|b| *b.borrow_mut() = previous);
    }
}

pub fn bound_identity() -> Option<AgentIdentity> {
    BOUND.with(|b| b.borrow().clone())
}
