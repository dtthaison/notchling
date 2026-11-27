use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

static NEXT_SLOT: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
struct State {
    fps: BTreeSet<String>,
    join_open: bool,
    joiner: Option<(String, u64)>,
}

#[derive(Clone, Default)]
pub struct Pins {
    inner: Arc<Mutex<State>>,
}

pub enum Admit {
    Pinned,
    Joiner(JoinSlot),
}

pub struct JoinSlot {
    pins: Pins,
    slot: u64,
}

impl Drop for JoinSlot {
    fn drop(&mut self) {
        let mut state = self.pins.lock();
        if state.joiner.as_ref().is_some_and(|(_, s)| *s == self.slot) {
            state.joiner = None;
        }
    }
}

impl Pins {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set(&self, fps: impl IntoIterator<Item = String>) {
        let mut state = self.lock();
        state.fps = fps.into_iter().collect();
        if state
            .joiner
            .as_ref()
            .is_some_and(|(fp, _)| state.fps.contains(fp))
        {
            state.joiner = None;
        }
    }

    pub fn set_join_open(&self, open: bool) {
        self.lock().join_open = open;
    }

    pub fn join_open(&self) -> bool {
        self.lock().join_open
    }

    pub fn contains(&self, fp: &str) -> bool {
        self.lock().fps.contains(fp)
    }

    pub fn list(&self) -> Vec<String> {
        self.lock().fps.iter().cloned().collect()
    }

    pub fn may_handshake(&self, fp: &str) -> bool {
        let state = self.lock();
        state.fps.contains(fp) || (state.join_open && state.joiner.is_none())
    }

    pub fn admit(&self, fp: &str) -> Option<Admit> {
        let mut state = self.lock();
        if state.fps.contains(fp) {
            return Some(Admit::Pinned);
        }
        if !state.join_open || state.joiner.is_some() {
            return None;
        }
        let slot = NEXT_SLOT.fetch_add(1, Ordering::SeqCst);
        state.joiner = Some((fp.to_string(), slot));
        Some(Admit::Joiner(JoinSlot {
            pins: self.clone(),
            slot,
        }))
    }
}
