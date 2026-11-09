//! When Chat reads its folders again, pure so it is tested on every system.
//!
//! The watcher (`platform::watch`) sends a batch once the folders have been quiet for its debounce
//! time, or `max_latency` after the first change when they never are (a session appending to its
//! transcript every few hundred milliseconds); Chat asks for both in [`watch_config`]. A
//! [`ReloadGate`] then decides when to read: at most every [`MIN_GAP_MS`] while batches keep
//! arriving, and at least every [`FULL_EVERY_MS`] for the ages and the Done window, which move on
//! their own.

use std::time::Duration;

use notchling_platform::watch::WatchConfig;

/// Changes arriving one after another reload no more often than this.
pub const MIN_GAP_MS: i64 = 2_000;
/// A reload with no change at all, for ages and the Done window.
pub const FULL_EVERY_MS: i64 = 30_000;

/// How Chat's watcher batches: after half a second of quiet or one second after the first change,
/// whichever comes first, so steady writes still get through. The poll applies only where the
/// watcher has to poll (not on a Mac, where FSEvents says what changed).
pub fn watch_config() -> WatchConfig {
    WatchConfig {
        poll: Duration::from_secs(1),
        debounce: Duration::from_millis(500),
        max_latency: Duration::from_secs(1),
    }
}

/// Decides when a reload is due from the watcher's signals and the clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReloadGate {
    last_ms: Option<i64>,
    changed: bool,
}

impl ReloadGate {
    /// The watcher reported a change.
    pub fn changed(&mut self) {
        self.changed = true;
    }

    /// Whether to reload now: at the start, [`MIN_GAP_MS`] after the last reload if something
    /// changed since (the first change after a quiet spell goes at once), or [`FULL_EVERY_MS`]
    /// after it whatever happened.
    pub fn due(&self, now_ms: i64) -> bool {
        let Some(last) = self.last_ms else {
            return true;
        };
        let since = now_ms - last;
        (self.changed && since >= MIN_GAP_MS) || since >= FULL_EVERY_MS
    }

    /// A reload started at `now_ms`: what changed before it is being read.
    pub fn reloaded(&mut self, now_ms: i64) {
        self.last_ms = Some(now_ms);
        self.changed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_platform::watch::Watcher;
    use std::time::Instant;

    #[test]
    fn the_first_reload_is_at_once_and_a_quiet_one_waits_for_the_full_reload() {
        let mut gate = ReloadGate::default();
        assert!(gate.due(0));
        gate.reloaded(0);
        assert!(!gate.due(1));
        assert!(!gate.due(FULL_EVERY_MS - 1));
        assert!(gate.due(FULL_EVERY_MS));
    }

    #[test]
    fn a_change_after_a_quiet_spell_reloads_at_once_and_one_just_after_a_reload_waits() {
        let mut gate = ReloadGate::default();
        gate.reloaded(0);
        gate.changed();
        assert!(!gate.due(MIN_GAP_MS - 1), "too soon after the last reload");
        assert!(gate.due(MIN_GAP_MS), "but not lost");
        gate.reloaded(MIN_GAP_MS);
        gate.changed();
        assert!(!gate.due(MIN_GAP_MS + 500));
        // The first change after a long quiet spell is looked at on the next tick.
        let mut quiet = ReloadGate::default();
        quiet.reloaded(0);
        quiet.changed();
        assert!(quiet.due(20_000));
    }

    #[test]
    fn steady_changes_reload_every_gap_and_are_never_starved() {
        let mut gate = ReloadGate::default();
        let tick = 500;
        let mut reloads = Vec::new();
        for now in (0..60_000).step_by(tick as usize) {
            gate.changed();
            if gate.due(now) {
                gate.reloaded(now);
                reloads.push(now);
            }
        }
        assert!(reloads.len() >= 28, "{} reloads in a minute", reloads.len());
        for pair in reloads.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(
                (MIN_GAP_MS..=MIN_GAP_MS + tick).contains(&gap),
                "reloads {} ms apart",
                gap
            );
        }
    }

    #[test]
    fn steady_writes_still_make_the_watcher_report() {
        let scratch = crate::testutil::Scratch::new("steady-writes");
        let file = scratch.write("transcript.jsonl", "x");
        // The production config as it is, so the test takes a few seconds.
        let config = watch_config();
        assert!(
            config.max_latency > config.debounce,
            "or the debounce never waits"
        );
        let (_watcher, batches) =
            Watcher::start(vec![scratch.path().to_path_buf()], config).expect("a watcher");
        let started = Instant::now();
        let mut reported = 0;
        let mut size = 1;
        // Faster than the half-second quiet spell, the way a working session appends.
        while started.elapsed() < Duration::from_millis(3_500) {
            size += 1;
            std::fs::write(&file, vec![b'x'; size]).expect("append");
            std::thread::sleep(Duration::from_millis(30));
            while batches.try_recv().is_ok() {
                reported += 1;
            }
        }
        assert!(
            reported >= 1,
            "no batch in 3.5 s of steady writes: a debounce alone starves the reload"
        );
    }
}
