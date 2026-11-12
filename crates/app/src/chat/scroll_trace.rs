//! Judging a scroll from what each frame painted: the bench's trajectory recorder keeps one
//! [`Frame`] per draw of the conversation (where every painted row was, how far the bench had
//! scrolled by then) and [`judge`] says what the content did between two frames against what the
//! wheel asked for. Pure, so the rules are tested on every system.
//!
//! The question it answers is "did each frame show the content where the wheel put it?", which a
//! frame's draw time never says: a stall (the wheel moved, the content didn't), a jump (it moved
//! by another amount), a reversal (the other way) and a frame that came late are what reads as
//! jerky, and each costs a frame under 16 ms.

use serde_json::{json, Value};

/// A move smaller than this (points) is no move: rounding and sub-pixel layout.
pub const NO_MOVE: f32 = 0.5;
/// What a frame may differ from the wheel by (points) before it counts as a jump, and the share of
/// a big step that is allowed on top (a fling of 1,000 points is measured to a few).
pub const JUMP_TOLERANCE: f32 = 1.5;
pub const JUMP_SHARE: f32 = 0.02;
/// Rows whose places disagree by more than this changed height between two frames.
pub const RELAYOUT: f32 = 1.5;
/// A wheel step that waits longer than this (ms) for the draw that shows it was shown late: two
/// frames at 60 a second.
pub const SLOW_FRAME_MS: f64 = 34.0;

/// What one draw of the conversation showed.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    /// Milliseconds since the recording started.
    pub at_ms: f64,
    /// How far the bench had scrolled by this draw, in points; positive moves the content down
    /// (towards what is above it).
    pub injected: f64,
    /// The window's frame counter at this draw.
    pub window_frame: u64,
    /// The rows painted, by key, with the top of each in window coordinates.
    pub rows: Vec<(String, f32)>,
    /// The list's scroll top: item and points into it.
    pub item_ix: usize,
    pub offset_in_item: f32,
    /// How far the list reports itself scrolled (the scroll bar's number, positive down).
    pub scrolled: f32,
    /// The list holds its end (it follows new content).
    pub at_end: bool,
    /// The list is at its very top: nothing above to scroll to.
    pub at_top: bool,
    /// Rows laid out in this draw's frame so far (the running count), for rows per draw.
    pub rows_built: u64,
}

/// What happened between two frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// The wheel asked for a move and the content stayed.
    Stall { asked: f32 },
    /// The content moved the other way.
    Reversal { asked: f32, moved: f32 },
    /// It moved the right way by another amount (`moved - asked`).
    Jump { asked: f32, moved: f32 },
    /// No row was in both frames: the content changed by more than a viewport.
    Lost { asked: f32 },
    /// The content moved though the wheel didn't ask.
    Drift { moved: f32 },
}

impl Event {
    /// How far the content was off from where the wheel put it.
    pub fn size(self) -> f32 {
        match self {
            Event::Stall { asked } => asked.abs(),
            Event::Reversal { asked, moved } | Event::Jump { asked, moved } => {
                (moved - asked).abs()
            }
            Event::Lost { .. } => f32::INFINITY,
            Event::Drift { moved } => moved.abs(),
        }
    }
}

/// How the content moved from `a` to `b`, in points: the median of how far each row painted in both
/// moved, and how far apart those rows' moves are (a row that changed height between the frames
/// shows as spread). `None` when no row was in both.
pub fn moved(a: &Frame, b: &Frame) -> Option<(f32, f32)> {
    let mut deltas: Vec<f32> = b
        .rows
        .iter()
        .filter_map(|(key, y)| {
            a.rows
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, before)| y - before)
        })
        .collect();
    if deltas.is_empty() {
        return None;
    }
    deltas.sort_by(f32::total_cmp);
    let (low, high) = (deltas[0], deltas[deltas.len() - 1]);
    Some((deltas[deltas.len() / 2], high - low))
}

/// What the content did between `a` and `b` against what the wheel asked for, if it went wrong.
/// Moves that the list can't make (down at its end) aren't wrong.
pub fn judge(a: &Frame, b: &Frame) -> Option<Event> {
    let asked = (b.injected - a.injected) as f32;
    let moved = moved(a, b);
    if asked.abs() < NO_MOVE {
        // Nothing asked: new rows at the end of a list that follows it push the content up, which
        // is no drift; any other move is.
        let (m, _) = moved?;
        let growing_at_end = a.at_end && b.at_end;
        return (m.abs() >= JUMP_TOLERANCE && !growing_at_end).then_some(Event::Drift { moved: m });
    }
    // Down at the end of the list, or up at its top: there is nowhere to go.
    if (a.at_end && asked < 0.0) || (a.at_top && asked > 0.0) {
        return None;
    }
    let Some((m, _)) = moved else {
        return Some(Event::Lost { asked });
    };
    if m.abs() < NO_MOVE {
        return Some(Event::Stall { asked });
    }
    if m.signum() != asked.signum() {
        return Some(Event::Reversal { asked, moved: m });
    }
    // Stopped at the end or the top, with less to go than asked.
    if ((b.at_end && asked < 0.0) || (b.at_top && asked > 0.0)) && m.abs() < asked.abs() {
        return None;
    }
    ((m - asked).abs() > JUMP_TOLERANCE.max(asked.abs() * JUMP_SHARE))
        .then_some(Event::Jump { asked, moved: m })
}

/// What a run's frames add up to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// Draws of the conversation recorded.
    pub frames: usize,
    /// Wheel steps the bench made.
    pub steps: usize,
    /// Intervals between two draws in which the wheel asked for a move.
    pub asked: usize,
    pub stalls: usize,
    pub jumps: usize,
    pub reversals: usize,
    pub lost: usize,
    pub drifts: usize,
    /// The largest jump, drift or stall, in points (a lost frame counts as the viewport's height).
    pub max_jump: f32,
    /// Frames in which rows painted twice disagreed about how far they moved: a row changed height.
    pub relayouts: usize,
    /// Wheel steps shown more than [`SLOW_FRAME_MS`] after they were made.
    pub slow_frames: usize,
    /// How long a wheel step waited for the draw that showed it (ms): median, 99th percentile, max.
    pub latency_p50_ms: f64,
    pub latency_p99_ms: f64,
    pub latency_max_ms: f64,
    /// The first few wrong intervals, as `(frame index, event)`.
    pub first_events: Vec<(usize, Event)>,
}

/// How many wrong intervals a report keeps.
const KEPT: usize = 8;

/// Judges `frames` (in draw order). `viewport` is the list's height, the size of a lost frame;
/// `steps` are the times (ms) the bench injected a wheel step at, for how long each waited.
pub fn report(frames: &[Frame], steps: &[(f64, f64)], viewport: f32) -> Report {
    let mut r = Report {
        frames: frames.len(),
        steps: steps.len(),
        ..Report::default()
    };
    for (i, pair) in frames.windows(2).enumerate() {
        let (a, b) = (&pair[0], &pair[1]);
        let asked = (b.injected - a.injected).abs() as f32 >= NO_MOVE;
        if asked {
            r.asked += 1;
        }
        if let Some((_, spread)) = moved(a, b) {
            if spread > RELAYOUT {
                r.relayouts += 1;
            }
        }
        let Some(event) = judge(a, b) else { continue };
        // A step of a viewport or more can't leave a row in both frames: not a loss.
        if matches!(event, Event::Lost { asked } if asked.abs() >= viewport) {
            continue;
        }
        let size = match event {
            Event::Lost { .. } => viewport,
            other => other.size(),
        };
        r.max_jump = r.max_jump.max(size);
        match event {
            Event::Stall { .. } => r.stalls += 1,
            Event::Jump { .. } => r.jumps += 1,
            Event::Reversal { .. } => r.reversals += 1,
            Event::Lost { .. } => r.lost += 1,
            Event::Drift { .. } => r.drifts += 1,
        }
        if r.first_events.len() < KEPT {
            r.first_events.push((i + 1, event));
        }
    }
    let mut latencies: Vec<f64> = steps
        .iter()
        .filter_map(|&(at, injected)| {
            // The first draw that shows this step: the first whose total has reached it.
            frames
                .iter()
                .find(|f| f.at_ms >= at && f.injected >= injected - 0.01)
                .map(|f| f.at_ms - at)
        })
        .collect();
    latencies.sort_by(f64::total_cmp);
    if !latencies.is_empty() {
        let at = |q: f64| latencies[((latencies.len() - 1) as f64 * q).round() as usize];
        r.latency_p50_ms = at(0.5);
        r.latency_p99_ms = at(0.99);
        r.latency_max_ms = latencies[latencies.len() - 1];
        r.slow_frames = latencies.iter().filter(|l| **l > SLOW_FRAME_MS).count();
    }
    r
}

impl Report {
    /// The `scroll` object of the bench's `bench-json:` line.
    pub fn json(&self) -> Value {
        let round = |v: f64| (v * 10.0).round() / 10.0;
        let events: Vec<Value> = self
            .first_events
            .iter()
            .map(|(i, e)| json!({"frame": i, "event": format!("{e:?}")}))
            .collect();
        json!({
            "frames": self.frames,
            "steps": self.steps,
            "asked": self.asked,
            "stalls": self.stalls,
            "jumps": self.jumps,
            "reversals": self.reversals,
            "lost": self.lost,
            "drifts": self.drifts,
            "max_jump": if self.max_jump.is_finite() { round(f64::from(self.max_jump)) } else { -1.0 },
            "relayouts": self.relayouts,
            "slow_frames": self.slow_frames,
            "latency_p50_ms": round(self.latency_p50_ms),
            "latency_p99_ms": round(self.latency_p99_ms),
            "latency_max_ms": round(self.latency_max_ms),
            "first_events": events,
        })
    }
}

/// What a draw may cost (ms of the main thread's own CPU time) before a 60 Hz display misses its
/// frame.
pub const FRAME_BUDGET_MS: f64 = 16.7;

/// One window draw, as the bench's frame driver timed it. The thread's CPU time is the number to
/// read: wall time also holds whatever else the Mac was doing (a build next door).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draw {
    /// Milliseconds since the recording started, at the draw's start.
    pub at_ms: f64,
    pub wall_ms: f64,
    /// The main thread's CPU time inside the draw.
    pub cpu_ms: f64,
    /// The main thread's CPU time since the last draw ended, outside any draw: the wheel events,
    /// loads landing, timers.
    pub between_cpu_ms: f64,
    /// Conversation rows built in this draw.
    pub rows: u64,
    /// The list was changed (a load's splice, a reset) since the last draw.
    pub changes: u64,
    /// Of `cpu_ms`, what building the rows' elements took.
    pub build_ms: f64,
    /// Of `cpu_ms`, how much was spent by the end of the conversation's prepaint (the root's
    /// render, layout and measuring the rows); the rest is painting.
    pub prepaint_ms: f64,
}

impl Draw {
    /// What made the draw dear, in a word and its number.
    pub fn cause(&self) -> String {
        if self.cpu_ms <= FRAME_BUDGET_MS {
            "none".to_owned()
        } else if self.changes > 0 {
            format!("list changed ({} rows built)", self.rows)
        } else if self.rows > 0 {
            format!("{} rows built", self.rows)
        } else {
            "rows already laid out".to_owned()
        }
    }
}

/// The draws of a run added up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawReport {
    pub draws: usize,
    pub cpu_p50_ms: f64,
    pub cpu_p99_ms: f64,
    pub cpu_max_ms: f64,
    pub wall_p99_ms: f64,
    pub wall_max_ms: f64,
    /// Draws over half the budget, over the budget, and over two frames of it.
    pub over_half: usize,
    pub over_budget: usize,
    pub over_two: usize,
    /// Time outside draws worth a frame (CPU ms over the budget) and its largest.
    pub between_over_budget: usize,
    pub between_max_ms: f64,
    /// The dearest draws, worst first.
    pub worst: Vec<Draw>,
}

/// How many of the dearest draws a report keeps.
const WORST: usize = 6;

pub fn draw_report(draws: &[Draw]) -> DrawReport {
    let mut r = DrawReport {
        draws: draws.len(),
        ..DrawReport::default()
    };
    if draws.is_empty() {
        return r;
    }
    let quantile = |mut v: Vec<f64>, q: f64| {
        v.sort_by(f64::total_cmp);
        v[((v.len() - 1) as f64 * q).round() as usize]
    };
    let cpu: Vec<f64> = draws.iter().map(|d| d.cpu_ms).collect();
    let wall: Vec<f64> = draws.iter().map(|d| d.wall_ms).collect();
    r.cpu_p50_ms = quantile(cpu.clone(), 0.5);
    r.cpu_p99_ms = quantile(cpu.clone(), 0.99);
    r.cpu_max_ms = quantile(cpu, 1.0);
    r.wall_p99_ms = quantile(wall.clone(), 0.99);
    r.wall_max_ms = quantile(wall, 1.0);
    r.over_half = draws
        .iter()
        .filter(|d| d.cpu_ms > FRAME_BUDGET_MS / 2.0)
        .count();
    r.over_budget = draws.iter().filter(|d| d.cpu_ms > FRAME_BUDGET_MS).count();
    r.over_two = draws
        .iter()
        .filter(|d| d.cpu_ms > FRAME_BUDGET_MS * 2.0)
        .count();
    r.between_over_budget = draws
        .iter()
        .filter(|d| d.between_cpu_ms > FRAME_BUDGET_MS)
        .count();
    r.between_max_ms = draws.iter().map(|d| d.between_cpu_ms).fold(0.0, f64::max);
    let mut worst = draws.to_vec();
    worst.sort_by(|a, b| b.cpu_ms.total_cmp(&a.cpu_ms));
    worst.truncate(WORST);
    r.worst = worst;
    r
}

impl DrawReport {
    /// The `draws` object of the bench's `scroll` report.
    pub fn json(&self) -> Value {
        let round = |v: f64| (v * 10.0).round() / 10.0;
        let worst: Vec<Value> = self
            .worst
            .iter()
            .map(|d| {
                json!({
                    "at_ms": d.at_ms.round(),
                    "cpu_ms": round(d.cpu_ms),
                    "wall_ms": round(d.wall_ms),
                    "build_ms": round(d.build_ms),
                    "prepaint_ms": round(d.prepaint_ms),
                    "rows": d.rows,
                    "changes": d.changes,
                    "between_cpu_ms": round(d.between_cpu_ms),
                    "cause": d.cause(),
                })
            })
            .collect();
        json!({
            "draws": self.draws,
            "cpu_p50_ms": round(self.cpu_p50_ms),
            "cpu_p99_ms": round(self.cpu_p99_ms),
            "cpu_max_ms": round(self.cpu_max_ms),
            "wall_p99_ms": round(self.wall_p99_ms),
            "wall_max_ms": round(self.wall_max_ms),
            "over_half_budget": self.over_half,
            "over_budget": self.over_budget,
            "over_two_frames": self.over_two,
            "between_over_budget": self.between_over_budget,
            "between_max_ms": round(self.between_max_ms),
            "worst": worst,
        })
    }
}

#[cfg(test)]
#[path = "scroll_trace_tests.rs"]
mod tests;
