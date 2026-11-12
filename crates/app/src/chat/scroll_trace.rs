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
/// What a frame may differ from the wheel by (points) before it counts as a jump.
pub const JUMP_TOLERANCE: f32 = 1.5;
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
    // Down at the end of the list: there is nowhere to go.
    if a.at_end && asked < 0.0 {
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
    // Stopped at the end, with less to go than asked.
    if b.at_end && asked < 0.0 && m.abs() < asked.abs() {
        return None;
    }
    ((m - asked).abs() > JUMP_TOLERANCE).then_some(Event::Jump { asked, moved: m })
}

/// What a run's frames add up to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// Draws of the conversation recorded.
    pub frames: usize,
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

#[cfg(test)]
#[path = "scroll_trace_tests.rs"]
mod tests;
