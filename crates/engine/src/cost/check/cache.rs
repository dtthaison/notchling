use super::data::{Data, Tally};
use super::{Finding, Ttl, ADVICE, MIN_REBUILD, SWITCH_WINDOW};
use crate::cost::ledger::TOOL_CLAUDE;

struct Rebuild {
    index: usize,
    stream: usize,
    gap: f64,
    extra: f64,
}

fn rebuilds(data: &Data, wanted: impl Fn(&Data, usize, f64) -> bool) -> Vec<Rebuild> {
    let mut out = Vec::new();
    for (s, range) in data.streams.iter().enumerate() {
        if data.calls[range.start].tool != TOOL_CLAUDE {
            continue;
        }
        for index in range.start + 1..range.end {
            let call = data.calls[index];
            let gap = call.ts - data.calls[index - 1].ts;
            if call.writes() < MIN_REBUILD || !wanted(data, s, gap) {
                continue;
            }
            if let Some(extra) = data.rebuild_extra(call) {
                out.push(Rebuild {
                    index,
                    stream: s,
                    gap,
                    extra,
                });
            }
        }
    }
    out
}

pub fn cold_resumes(data: &mut Data) -> Finding {
    let mut finding = Finding::new("cold_resumes");
    let found = rebuilds(data, |d, s, gap| d.ttl_secs(s).is_some_and(|t| gap > t));
    let (mut main, mut sub) = ([0.0; 2], [0.0; 2]);
    let (mut idle, mut saving, mut tokens) = (0.0, 0.0, 0.0);
    for r in &found {
        let call = data.calls[r.index];
        let slot = if call.sub { &mut sub } else { &mut main };
        slot[0] += 1.0;
        slot[1] += r.extra;
        idle += r.gap;
        if !data.is_claimed(r.index) {
            saving += r.extra * ADVICE;
            tokens += call.writes() as f64 * ADVICE;
        }
    }
    for r in &found {
        data.claim(r.index);
    }
    let (added, saved) = one_hour_helpers(data, &found);
    finding.ev("main_rebuilds", main[0]);
    finding.ev("sub_rebuilds", sub[0]);
    finding.ev("main_extra_usd", main[1]);
    finding.ev("sub_extra_usd", sub[1]);
    finding.ev(
        "avg_idle_min",
        if found.is_empty() {
            0.0
        } else {
            idle / found.len() as f64 / 60.0
        },
    );
    finding.ev("ttl1h_added_usd", added);
    finding.ev("ttl1h_saved_usd", saved);
    finding.ev("ttl1h_pays", f64::from(u8::from(saved > added)));
    if !found.is_empty() {
        finding.saving = Some(saving);
        finding.tokens_avoided = Some(tokens as u64);
    }
    finding
}

fn one_hour_helpers(data: &Data, found: &[Rebuild]) -> (f64, f64) {
    let mut added = 0.0;
    let mut saved = 0.0;
    for (s, range) in data.streams.iter().enumerate() {
        let first = data.calls[range.start];
        let own = Tally::of(data.calls[range.clone()].iter().copied());
        if !first.sub || first.tool != TOOL_CLAUDE || data.ttl[s] != Ttl::FiveMin {
            continue;
        }
        if own.ttl() != Ttl::FiveMin {
            continue;
        }
        for call in &data.calls[range.clone()] {
            if let Some(price) = data.price(call) {
                added += call.write_5m as f64 * price.input * 0.75 / 1_000_000.0;
            }
        }
    }
    for r in found {
        if data.calls[r.index].sub && data.ttl[r.stream] == Ttl::FiveMin && r.gap <= 3_600.0 {
            saved += r.extra;
        }
    }
    (added, saved)
}

pub fn model_switches(data: &mut Data) -> Finding {
    let mut finding = Finding::new("model_switches");
    let found = rebuilds(data, |d, s, gap| {
        gap <= d.ttl_secs(s).unwrap_or(SWITCH_WINDOW).max(SWITCH_WINDOW)
    });
    let found: Vec<Rebuild> = found
        .into_iter()
        .filter(|r| data.calls[r.index].model != data.calls[r.index - 1].model)
        .collect();
    let (mut extra, mut saving, mut tokens, mut rewritten) = (0.0, 0.0, 0.0, 0.0);
    for r in &found {
        let call = data.calls[r.index];
        extra += r.extra;
        rewritten += call.writes() as f64;
        if !data.is_claimed(r.index) {
            saving += r.extra * ADVICE;
            tokens += call.writes() as f64 * ADVICE;
        }
    }
    for r in &found {
        data.claim(r.index);
    }
    finding.ev("switches", found.len() as f64);
    finding.ev("extra_usd", extra);
    finding.ev("rewritten_tokens", rewritten);
    if !found.is_empty() {
        finding.saving = Some(saving);
        finding.tokens_avoided = Some(tokens as u64);
    }
    finding
}
