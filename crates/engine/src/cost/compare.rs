use super::ledger::Rollup;
use super::summary::day_key;
use serde::Serialize;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Side {
    pub from: String,
    pub to: String,
    pub days: u32,
    pub calls: u64,
    pub tokens: u64,
    pub input: u64,
    pub output: u64,
    pub cached: u64,
    pub cost: Option<f64>,
    pub unpriced_calls: u64,
    pub unpriced_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Comparison {
    pub window: u32,
    pub applied: String,
    pub before: Side,
    pub after: Side,
}

fn side(rollups: &[Rollup], first: i64, last: i64) -> Side {
    let (from, to) = (day_key(first), day_key(last));
    let mut out = Side {
        days: (last - first + 1).max(0) as u32,
        from: from.clone(),
        to: to.clone(),
        ..Side::default()
    };
    for row in rollups
        .iter()
        .filter(|r| r.day.as_str() >= from.as_str() && r.day.as_str() <= to.as_str())
    {
        let cached = row.cache_read + row.write_5m + row.write_1h;
        let tokens = row.input + row.output + cached;
        out.calls += row.calls;
        out.tokens += tokens;
        out.input += row.input;
        out.output += row.output;
        out.cached += cached;
        if row.unpriced < row.calls {
            out.cost = Some(out.cost.unwrap_or(0.0) + row.cost);
        }
        if row.unpriced > 0 && row.calls > 0 {
            out.unpriced_calls += row.unpriced;
            out.unpriced_tokens += (tokens as f64 * row.unpriced as f64 / row.calls as f64) as u64;
        }
    }
    out
}

pub fn window_start(today: i64, applied_day: i64, window: u32) -> i64 {
    applied_day.min(today) - i64::from(window.max(1))
}

pub fn compare(rollups: &[Rollup], today: i64, applied_day: i64, window: u32) -> Comparison {
    let span = i64::from(window.max(1));
    let applied_day = applied_day.min(today);
    let first_after = applied_day.max(today - span + 1);
    Comparison {
        window: window.max(1),
        applied: day_key(applied_day),
        before: side(rollups, applied_day - span, applied_day - 1),
        after: side(rollups, first_after, today),
    }
}
