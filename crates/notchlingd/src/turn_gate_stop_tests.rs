use crate::turn_gate::Stopping;
use notchling_engine::reply::exec::{wait_attached, Pty};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

struct Noisy {
    clock: u64,
    writes: usize,
}

impl Pty for Noisy {
    fn write(&mut self, _bytes: &[u8]) -> std::io::Result<()> {
        self.writes += 1;
        Ok(())
    }

    fn wait_quiet(&mut self, _quiet: Duration, timeout: Duration) -> bool {
        self.clock += timeout.as_millis() as u64;
        false
    }

    fn screen(&mut self) -> Vec<String> {
        vec!["⠋ Reconnecting".to_string()]
    }

    fn sleep(&mut self, duration: Duration) {
        self.clock += duration.as_millis() as u64;
    }

    fn now_ms(&self) -> u64 {
        self.clock
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn a_stop_is_seen_only_while_nothing_has_been_typed() {
    let stop = AtomicBool::new(false);
    let mut inner = Noisy {
        clock: 0,
        writes: 0,
    };
    let mut gated = Stopping {
        pty: &mut inner,
        stop: &stop,
        wrote: false,
    };
    assert!(!gated.stopping());
    stop.store(true, Ordering::SeqCst);
    assert!(gated.stopping());
    gated.write(b"x").unwrap();
    assert!(
        !gated.stopping(),
        "a key was typed, the wait is never cut short"
    );
}

#[test]
fn a_stop_cuts_a_noisy_attach_wait_after_one_slice() {
    let stop = AtomicBool::new(true);
    let mut inner = Noisy {
        clock: 0,
        writes: 0,
    };
    let mut gated = Stopping {
        pty: &mut inner,
        stop: &stop,
        wrote: false,
    };
    let drew = gated.wait_attached(ms(500), ms(15_000), ms(2_000));
    assert!(!drew);
    assert_eq!(inner.clock, 2_000);
}

#[test]
fn a_typed_wait_runs_to_its_end_even_with_a_stop_pending() {
    let stop = AtomicBool::new(true);
    let mut inner = Noisy {
        clock: 0,
        writes: 0,
    };
    let mut gated = Stopping {
        pty: &mut inner,
        stop: &stop,
        wrote: true,
    };
    wait_attached(&mut gated, ms(500), ms(15_000), ms(2_000));
    assert_eq!(inner.clock, 15_000);
}
