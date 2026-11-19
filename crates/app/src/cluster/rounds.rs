use std::sync::atomic::{AtomicU64, Ordering};

static LAST: AtomicU64 = AtomicU64::new(0);

pub fn next() -> u64 {
    LAST.fetch_add(1, Ordering::Relaxed) + 1
}
