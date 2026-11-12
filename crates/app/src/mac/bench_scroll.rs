//! The bench's scroll scenarios (`NOTCHLING_RENDER_BENCH_MODE=scroll-start|scroll-slow|scroll-fling|
//! scroll-momentum`): each feeds the conversation a script of trackpad steps, one every 1/60 s,
//! while the trajectory recorder (`chat::view::scroll_probe`) notes what every draw showed. The
//! report (`chat::scroll_trace`) goes in the `bench-json:` line as `scroll`.
//!
//! A step is whole points (the injected event carries an integer pixel delta) and positive moves
//! the content down, towards what is above it, the way the wheel does when you scroll up.

use crate::chat::view::scroll_probe::Recording;

/// What a scroll mode feeds the wheel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// From the end, small steps that grow: what a trackpad does when a gesture starts, then back
    /// down to the end.
    Start,
    /// A steady slow scroll up from just above the end, through rows nothing has measured.
    Slow,
    /// Flings of 300 to 1500 points a step up, and back down.
    Fling,
    /// Bursts of steps that decay (a trackpad's momentum): three up, three down.
    Momentum,
}

impl Scenario {
    pub const ALL: [Scenario; 4] = [
        Scenario::Start,
        Scenario::Slow,
        Scenario::Fling,
        Scenario::Momentum,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Scenario::Start => "scroll-start",
            Scenario::Slow => "scroll-slow",
            Scenario::Fling => "scroll-fling",
            Scenario::Momentum => "scroll-momentum",
        }
    }

    pub fn parse(name: &str) -> Option<Scenario> {
        Scenario::ALL.into_iter().find(|s| s.name() == name)
    }

    /// How many wheel steps the script has, a short rest at its end included.
    pub fn steps(self) -> u64 {
        match self {
            Scenario::Start => 2 * START_UP + START_REST + 18,
            Scenario::Slow => SLOW_STEPS,
            Scenario::Fling => 2 * FLING_STEPS + FLING_REST + 10,
            Scenario::Momentum => BURSTS * (BURST_STEPS + BURST_REST),
        }
    }

    /// One move before the numbers start, past the follow tolerance, for a scenario that is about
    /// something other than leaving the end.
    pub fn prime(self) -> f32 {
        match self {
            Scenario::Slow => 40.0,
            _ => 0.0,
        }
    }

    /// The wheel's step `n` in points.
    pub fn delta(self, n: u64) -> f32 {
        match self {
            Scenario::Start => start(n),
            Scenario::Slow => SLOW_STEP,
            Scenario::Fling => fling(n),
            Scenario::Momentum => momentum(n),
        }
    }
}

/// A slow scroll's step, and how many it makes (six seconds).
const SLOW_STEP: f32 = 6.0;
const SLOW_STEPS: u64 = 360;

/// The ramp a gesture starts with: 1, 1, 2, 2, 3, 3 … up to 30 a step.
fn ramp(n: u64) -> f32 {
    (1 + n / 2).min(30) as f32
}
const START_UP: u64 = 90;
const START_REST: u64 = 12;

/// Up the ramp for [`START_UP`] steps, a rest, then the same steps down: back at the end.
fn start(n: u64) -> f32 {
    if n < START_UP {
        ramp(n)
    } else if n < START_UP + START_REST {
        0.0
    } else if n < 2 * START_UP + START_REST {
        -ramp(n - START_UP - START_REST)
    } else {
        0.0
    }
}

const FLING_STEPS: u64 = 16;
const FLING_REST: u64 = 10;

/// 300, 380 … 1500 points a step for [`FLING_STEPS`], a rest, then the same down.
fn fling(n: u64) -> f32 {
    let step = |k: u64| (300 + 80 * k) as f32;
    if n < FLING_STEPS {
        step(n)
    } else if n < FLING_STEPS + FLING_REST {
        0.0
    } else if n < 2 * FLING_STEPS + FLING_REST {
        -step(n - FLING_STEPS - FLING_REST)
    } else {
        0.0
    }
}

/// How fast a momentum burst starts, how much of it is left each step, and where it stops.
const BURST_SPEED: f32 = 60.0;
const BURST_DECAY: f32 = 0.93;
const BURST_STEPS: u64 = 64;
const BURST_REST: u64 = 15;
const BURSTS: u64 = 6;

/// Three bursts up, then three down.
fn momentum(n: u64) -> f32 {
    let (burst, k) = (
        n / (BURST_STEPS + BURST_REST),
        n % (BURST_STEPS + BURST_REST),
    );
    if burst >= BURSTS || k >= BURST_STEPS {
        return 0.0;
    }
    let speed = (BURST_SPEED * BURST_DECAY.powi(k as i32)).round();
    if burst < BURSTS / 2 {
        speed
    } else {
        -speed
    }
}

/// Where a run writes every frame's numbers, one JSON object per line (never a row's key or text).
pub const DUMP_ENV: &str = "NOTCHLING_BENCH_SCROLL_DUMP";

/// Writes the recorded frames to the file `DUMP_ENV` names, if it is set, and the timed draws to
/// the same name with `.draws` added: what a person reads to see where a jump or a slow draw
/// happened.
pub fn dump(recording: &Recording) {
    use std::io::Write;
    let Some(path) = std::env::var_os(DUMP_ENV) else {
        return;
    };
    if let Ok(mut file) = std::fs::File::create(&path) {
        for (i, f) in recording.frames.iter().enumerate() {
            let line = serde_json::json!({
                "i": i,
                "t": (f.at_ms * 10.0).round() / 10.0,
                "wheel": f.injected,
                "win": f.window_frame,
                "item": f.item_ix,
                "offset": f.offset_in_item.round(),
                "scrolled": f.scrolled.round(),
                "end": f.at_end,
                "top": f.at_top,
                "rows": f.rows.len(),
                "first_y": f.rows.first().map(|(_, y)| y.round()),
                "built": f.rows_built,
            });
            let _ = writeln!(file, "{line}");
        }
    }
    let mut draws_path = path;
    draws_path.push(".draws");
    if let Ok(mut file) = std::fs::File::create(draws_path) {
        for d in &recording.draws {
            let line = serde_json::json!({
                "t": d.at_ms.round(),
                "cpu": (d.cpu_ms * 10.0).round() / 10.0,
                "wall": (d.wall_ms * 10.0).round() / 10.0,
                "build": (d.build_ms * 10.0).round() / 10.0,
                "prepaint": (d.prepaint_ms * 10.0).round() / 10.0,
                "between": (d.between_cpu_ms * 10.0).round() / 10.0,
                "rows": d.rows,
                "changes": d.changes,
            });
            let _ = writeln!(file, "{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total(scenario: Scenario, steps: u64) -> f32 {
        (0..steps).map(|n| scenario.delta(n)).sum()
    }

    #[test]
    fn every_scenario_parses_by_its_name() {
        for s in Scenario::ALL {
            assert_eq!(Scenario::parse(s.name()), Some(s));
        }
        assert_eq!(Scenario::parse("scroll"), None);
    }

    #[test]
    fn a_gesture_starts_with_steps_under_the_follow_tolerance() {
        // The first 24 points of travel take several steps: what the end's tolerance swallowed.
        let first: Vec<f32> = (0..8).map(|n| Scenario::Start.delta(n)).collect();
        assert!(first.iter().all(|d| *d < 6.0), "{first:?}");
        assert!(first.iter().sum::<f32>() < 24.0);
        assert!(total(Scenario::Start, 12) > 0.0);
    }

    #[test]
    fn the_scripts_that_go_up_come_back_to_the_end() {
        for s in [Scenario::Start, Scenario::Fling] {
            assert_eq!(total(s, 1000), 0.0, "{}", s.name());
        }
        // Momentum's rounding leaves a few points either way.
        assert!(total(Scenario::Momentum, 1000).abs() < 10.0);
    }

    #[test]
    fn a_script_has_all_its_moves_in_its_steps() {
        // Every step that moves is inside the count, and the rest after it is quiet. (The slow
        // scroll never rests: its count is its length.)
        for s in [Scenario::Start, Scenario::Fling, Scenario::Momentum] {
            let last = (0..2000).rev().find(|n| s.delta(*n) != 0.0).unwrap_or(0);
            assert!(s.steps() > last, "{} stops at {last}", s.name());
            assert!(s.steps() < last + 120, "{} rests too long", s.name());
        }
        assert_eq!(Scenario::Slow.steps(), SLOW_STEPS);
    }

    #[test]
    fn steps_are_whole_points() {
        for s in Scenario::ALL {
            for n in 0..600 {
                let d = s.delta(n);
                assert_eq!(d, d.trunc(), "{} step {n}", s.name());
            }
        }
    }
}
