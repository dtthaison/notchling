use serde_json::{json, Value};

pub const NO_MOVE: f32 = 0.5;
pub const JUMP_TOLERANCE: f32 = 1.5;
pub const JUMP_SHARE: f32 = 0.02;
pub const RELAYOUT: f32 = 1.5;
pub const SLOW_FRAME_MS: f64 = 34.0;

#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub at_ms: f64,
    pub injected: f64,
    pub window_frame: u64,
    pub rows: Vec<(String, f32)>,
    pub item_ix: usize,
    pub offset_in_item: f32,
    pub scrolled: f32,
    pub at_end: bool,
    pub at_top: bool,
    pub rows_built: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Stall { asked: f32 },
    Reversal { asked: f32, moved: f32 },
    Jump { asked: f32, moved: f32 },
    Lost { asked: f32 },
    Drift { moved: f32 },
}

impl Event {
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

pub fn judge(a: &Frame, b: &Frame) -> Option<Event> {
    let asked = (b.injected - a.injected) as f32;
    let moved = moved(a, b);
    if asked.abs() < NO_MOVE {
        let (m, _) = moved?;
        let growing_at_end = a.at_end && b.at_end;
        return (m.abs() >= JUMP_TOLERANCE && !growing_at_end).then_some(Event::Drift { moved: m });
    }
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
    if ((b.at_end && asked < 0.0) || (b.at_top && asked > 0.0)) && m.abs() < asked.abs() {
        return None;
    }
    ((m - asked).abs() > JUMP_TOLERANCE.max(asked.abs() * JUMP_SHARE))
        .then_some(Event::Jump { asked, moved: m })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub frames: usize,
    pub steps: usize,
    pub asked: usize,
    pub stalls: usize,
    pub jumps: usize,
    pub reversals: usize,
    pub lost: usize,
    pub drifts: usize,
    pub max_jump: f32,
    pub relayouts: usize,
    pub slow_frames: usize,
    pub latency_p50_ms: f64,
    pub latency_p99_ms: f64,
    pub latency_max_ms: f64,
    pub first_events: Vec<(usize, Event)>,
}

const KEPT: usize = 8;

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

pub const FRAME_BUDGET_MS: f64 = 16.7;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draw {
    pub at_ms: f64,
    pub wall_ms: f64,
    pub cpu_ms: f64,
    pub between_cpu_ms: f64,
    pub rows: u64,
    pub changes: u64,
    pub build_ms: f64,
    pub prepaint_ms: f64,
}

impl Draw {
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawReport {
    pub draws: usize,
    pub cpu_p50_ms: f64,
    pub cpu_p99_ms: f64,
    pub cpu_max_ms: f64,
    pub wall_p99_ms: f64,
    pub wall_max_ms: f64,
    pub over_half: usize,
    pub over_budget: usize,
    pub over_two: usize,
    pub between_over_budget: usize,
    pub between_max_ms: f64,
    pub worst: Vec<Draw>,
}

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
