use crate::envelope::Actor;
use crate::field::Hex;
use std::cell::{Cell, RefCell};

#[derive(Clone, Debug, Default)]
pub struct Scope {
    pub op_id: Option<Hex>,
    pub actor: Option<Actor>,
}

thread_local! {
    static CURRENT: RefCell<Scope> = RefCell::new(Scope::default());
    static RECORDED: Cell<bool> = const { Cell::new(false) };
}

pub struct ScopeGuard {
    previous: Option<Scope>,
    recorded_before: bool,
}

pub fn enter(scope: Scope) -> ScopeGuard {
    let previous = CURRENT.with(|c| std::mem::replace(&mut *c.borrow_mut(), scope));
    let recorded_before = RECORDED.with(|r| r.replace(false));
    ScopeGuard {
        previous: Some(previous),
        recorded_before,
    }
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            CURRENT.with(|c| *c.borrow_mut() = previous);
        }
        let inner = RECORDED.with(Cell::get);
        RECORDED.with(|r| r.set(self.recorded_before || inner));
    }
}

pub fn current() -> Scope {
    CURRENT.with(|c| c.borrow().clone())
}

pub fn mark_recorded() {
    RECORDED.with(|r| r.set(true));
}

pub fn take_recorded() -> bool {
    RECORDED.with(|r| r.replace(false))
}

impl Scope {
    pub fn run<R>(self, f: impl FnOnce() -> R) -> R {
        let _guard = enter(self);
        f()
    }
}
