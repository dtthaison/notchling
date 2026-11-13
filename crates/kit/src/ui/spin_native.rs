use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui::Window;

use super::icons::{spin_clock, SPIN};
use super::spin_layer::SpinSlot;

pub trait SpinSink {
    fn sync(&self, window: &Window, slots: &[SpinSlot]);
}

pub type SpinGate = Rc<Cell<bool>>;

pub(super) struct NativeSpin {
    sink: Rc<dyn SpinSink>,
    allowed: SpinGate,
    live: Cell<bool>,
}

impl NativeSpin {
    pub(super) fn new(sink: Rc<dyn SpinSink>, allowed: SpinGate) -> NativeSpin {
        NativeSpin {
            sink,
            allowed,
            live: Cell::new(false),
        }
    }

    pub(super) fn allowed(&self) -> bool {
        self.allowed.get()
    }

    pub(super) fn sync(&self, window: &Window, slots: &[SpinSlot]) {
        self.live.set(!slots.is_empty());
        self.sink.sync(window, slots);
    }

    pub(super) fn leave(&self, window: &Window) {
        if self.live.replace(false) {
            self.sink.sync(window, &[]);
        }
    }
}

pub fn spin_period() -> Duration {
    SPIN
}

pub fn spin_phase() -> Duration {
    Duration::from_nanos((spin_clock().as_nanos() % SPIN.as_nanos()) as u64)
}

pub(super) fn mix_key(place: usize, key: u64) -> u64 {
    key.rotate_left(7) ^ (place as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

#[derive(Debug, PartialEq)]
pub struct Diff<T> {
    pub added: Vec<(u64, T)>,
    pub changed: Vec<(u64, T)>,
    pub removed: Vec<u64>,
}

pub fn diff<T: PartialEq + Clone>(prev: &HashMap<u64, T>, next: &[(u64, T)]) -> Diff<T> {
    let mut last: HashMap<u64, usize> = HashMap::new();
    for (at, (key, _)) in next.iter().enumerate() {
        last.insert(*key, at);
    }
    let mut out = Diff {
        added: Vec::new(),
        changed: Vec::new(),
        removed: Vec::new(),
    };
    for (at, (key, value)) in next.iter().enumerate() {
        if last[key] != at {
            continue;
        }
        match prev.get(key) {
            None => out.added.push((*key, value.clone())),
            Some(was) if was != value => out.changed.push((*key, value.clone())),
            Some(_) => {}
        }
    }
    out.removed = prev
        .keys()
        .filter(|key| !last.contains_key(key))
        .copied()
        .collect();
    out.removed.sort_unstable();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(items: &[(u64, i32)]) -> HashMap<u64, i32> {
        items.iter().copied().collect()
    }

    #[test]
    fn a_new_key_is_added_a_gone_one_removed_and_a_same_one_kept() {
        let d = diff(&map(&[(1, 10), (2, 20)]), &[(2, 20), (3, 30)]);
        assert_eq!(d.added, vec![(3, 30)]);
        assert_eq!(d.removed, vec![1]);
        assert!(d.changed.is_empty(), "key 2 is untouched: it keeps turning");
    }

    #[test]
    fn a_key_that_only_moved_is_changed_not_added() {
        let d = diff(&map(&[(1, 10)]), &[(1, 11)]);
        assert!(d.added.is_empty() && d.removed.is_empty());
        assert_eq!(d.changed, vec![(1, 11)]);
    }

    #[test]
    fn a_key_seen_twice_in_a_frame_counts_once_by_its_last_place() {
        let d = diff(&map(&[]), &[(1, 10), (2, 20), (1, 12)]);
        assert_eq!(d.added, vec![(2, 20), (1, 12)]);
        let d = diff(&map(&[(1, 12)]), &[(1, 10), (1, 12)]);
        assert!(d.added.is_empty() && d.changed.is_empty() && d.removed.is_empty());
    }

    #[test]
    fn nothing_on_screen_removes_everything() {
        let d = diff(&map(&[(5, 1), (3, 1)]), &[]);
        assert_eq!(d.removed, vec![3, 5]);
        assert!(d.added.is_empty() && d.changed.is_empty());
    }

    #[test]
    fn a_hosts_place_tells_equal_keys_apart() {
        assert_ne!(mix_key(0, 42), mix_key(1, 42));
        assert_eq!(mix_key(2, 42), mix_key(2, 42));
    }

    #[test]
    fn the_phase_is_inside_one_turn() {
        assert!(spin_phase() < spin_period());
    }
}
