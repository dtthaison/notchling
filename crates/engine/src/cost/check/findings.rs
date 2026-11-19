use super::data::Data;
use super::{
    Finding, ADVICE, BIG_CONTEXT, BUILDER_CAP, CAP_FACTOR, HANDOFF, LONG_RUN, LOW_EFFORT_SHARE,
    RUN_ADOPTION, TURN_FACTOR,
};
use crate::cost::ledger::TOOL_CLAUDE;
use crate::cost::prices;
use std::collections::{BTreeMap, BTreeSet};

pub use super::cache::{cold_resumes, model_switches};

pub fn premium_subagents(data: &mut Data) -> Finding {
    let mut finding = Finding::new("premium_subagents");
    let mut runs = BTreeSet::new();
    let (mut calls, mut cost, mut saving, mut sub_cost) = (0.0, 0.0, 0.0, 0.0);
    let mut models = BTreeSet::new();
    let mut claim = Vec::new();
    for (s, range) in data.streams.iter().enumerate() {
        if !data.calls[range.start].sub || data.calls[range.start].tool != TOOL_CLAUDE {
            continue;
        }
        for index in range.clone() {
            let call = data.calls[index];
            sub_cost += call.cost.unwrap_or(0.0);
            let (Some(price), Some(target), Some(now)) =
                (data.price(call), data.target.as_ref(), call.cost)
            else {
                continue;
            };
            if price.input <= target.price.input {
                continue;
            }
            runs.insert(s);
            models.insert(call.model.clone());
            calls += 1.0;
            cost += now;
            let cheaper = prices::estimate(&target.price, &Data::tokens(call)) * TURN_FACTOR;
            let kept = (now - cheaper).max(0.0) * ADVICE;
            if kept > 0.0 && !data.is_claimed(index) {
                saving += kept;
                claim.push(index);
            }
        }
    }
    for index in claim {
        data.claim(index);
    }
    finding.ev("runs", runs.len() as f64);
    finding.ev("calls", calls);
    finding.ev("cost_usd", cost);
    finding.ev("share", if sub_cost > 0.0 { cost / sub_cost } else { 0.0 });
    if let Some(target) = &data.target {
        finding.detail.insert("target".into(), target.model.clone());
        finding.saving = (calls > 0.0).then_some(saving);
    }
    finding.detail.insert(
        "models".into(),
        models.into_iter().collect::<Vec<_>>().join(", "),
    );
    finding
}

pub fn repeated_reviews(data: &mut Data) -> Finding {
    let mut finding = Finding::new("repeated_reviews");
    let mut by_session: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (s, range) in data.streams.iter().enumerate() {
        let first = data.calls[range.start];
        let review = first
            .agent_type
            .as_deref()
            .is_some_and(|t| t.to_lowercase().contains("review"));
        if first.sub && first.tool == TOOL_CLAUDE && review {
            by_session.entry(&first.session).or_default().push(s);
        }
    }
    let (mut total, mut total_cost, mut again, mut again_cost) = (0.0, 0.0, 0.0, 0.0);
    let (mut saving, mut tokens, mut worst, mut sessions) = (0.0, 0.0, 0.0f64, 0.0);
    let mut claim = Vec::new();
    for streams in by_session.values_mut() {
        streams.sort_by(|a, b| {
            let at = |s: &usize| data.calls[data.streams[*s].start].ts;
            at(a).total_cmp(&at(b))
        });
        total += streams.len() as f64;
        worst = worst.max(streams.len() as f64);
        if streams.len() > 1 {
            sessions += 1.0;
        }
        for (n, s) in streams.iter().enumerate() {
            for index in data.streams[*s].clone() {
                let call = data.calls[index];
                total_cost += call.cost.unwrap_or(0.0);
                if n == 0 {
                    continue;
                }
                again_cost += call.cost.unwrap_or(0.0);
                if !data.is_claimed(index) {
                    saving += call.cost.unwrap_or(0.0) * ADVICE;
                    tokens += call.tokens() as f64 * ADVICE;
                    claim.push(index);
                }
            }
            if n > 0 {
                again += 1.0;
            }
        }
    }
    for index in claim {
        data.claim(index);
    }
    finding.ev("review_runs", total);
    finding.ev("review_cost_usd", total_cost);
    finding.ev("repeat_runs", again);
    finding.ev("repeat_cost_usd", again_cost);
    finding.ev("sessions_repeating", sessions);
    finding.ev("most_in_a_session", worst);
    if total > 0.0 {
        finding.saving = Some(saving);
        finding.tokens_avoided = Some(tokens as u64);
    }
    finding
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted
        .get(((sorted.len() as f64 * p) as usize).min(sorted.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0.0)
}

pub fn long_runs(data: &mut Data) -> Finding {
    let mut finding = Finding::new("long_runs");
    let mut lengths = Vec::new();
    let mut costs = Vec::new();
    let (mut over, mut big, mut peak) = (0.0, 0.0, 0.0f64);
    let (mut saving, mut tokens, mut unpriced) = (0.0, 0.0, false);
    let mut claim = Vec::new();
    for range in data.streams.clone() {
        if !data.calls[range.start].sub {
            continue;
        }
        let n = range.len();
        lengths.push(n as f64);
        let run: f64 = data.calls[range.clone()]
            .iter()
            .map(|c| c.cost.unwrap_or(0.0))
            .sum();
        costs.push(run);
        for call in &data.calls[range.clone()] {
            peak = peak.max(call.context() as f64);
            if call.context() > BIG_CONTEXT {
                big += 1.0;
            }
        }
        if n <= LONG_RUN {
            continue;
        }
        over += 1.0;
        let base = data.calls[range.start].context() as f64;
        let mut start = base;
        for (i, index) in range.clone().enumerate() {
            let call = data.calls[index];
            if i > 0 && i % LONG_RUN == 0 {
                start = call.context() as f64 - base - HANDOFF;
            }
            if i < LONG_RUN || data.is_claimed(index) {
                continue;
            }
            let avoid = start.max(0.0);
            match data.price(call) {
                Some(price) => saving += avoid * price.cache_read / 1_000_000.0,
                None => unpriced = true,
            }
            tokens += avoid;
            claim.push(index);
        }
    }
    for index in claim {
        data.claim(index);
    }
    lengths.sort_by(f64::total_cmp);
    costs.sort_by(f64::total_cmp);
    let spend: f64 = costs.iter().sum();
    let top: f64 = costs[costs.len() - costs.len() / 10..].iter().sum();
    finding.ev("sub_runs", lengths.len() as f64);
    finding.ev("runs_over_limit", over);
    finding.ev("p90_calls", percentile(&lengths, 0.9));
    finding.ev("max_calls", lengths.last().copied().unwrap_or(0.0));
    finding.ev("calls_over_150k", big);
    finding.ev("peak_context", peak);
    finding.ev("top10_share", if spend > 0.0 { top / spend } else { 0.0 });
    if over > 0.0 && !(unpriced && saving == 0.0) {
        finding.saving = Some(saving * RUN_ADOPTION);
        finding.tokens_avoided = Some((tokens * RUN_ADOPTION) as u64);
    }
    finding
}

pub fn many_builders(data: &mut Data) -> Finding {
    let mut finding = Finding::new("many_builders");
    let buckets = data.buckets();
    let subs = |set: &std::collections::HashSet<usize>| {
        set.iter().filter(|s| data.calls[data.streams[**s].start].sub).count()
    };
    let over: BTreeSet<i64> = buckets
        .iter()
        .filter(|(_, set)| subs(set) > BUILDER_CAP)
        .map(|(b, _)| *b)
        .collect();
    let crowded: BTreeSet<i64> = buckets
        .iter()
        .filter(|(_, set)| set.len() > BUILDER_CAP)
        .map(|(b, _)| *b)
        .collect();
    let peak = buckets.values().map(&subs).max().unwrap_or(0);
    let bucket_of = |ts: f64| (ts / super::BUCKET).floor() as i64;
    let (mut over_cost, mut crowded_cost, mut saving, mut tokens) = (0.0, 0.0, 0.0, 0.0);
    let mut claim = Vec::new();
    for index in 0..data.calls.len() {
        let call = data.calls[index];
        let bucket = bucket_of(call.ts);
        let cost = call.cost.unwrap_or(0.0);
        if crowded.contains(&bucket) {
            crowded_cost += cost;
        }
        if over.contains(&bucket) {
            over_cost += cost;
            if !data.is_claimed(index) {
                saving += cost * CAP_FACTOR;
                tokens += call.tokens() as f64 * CAP_FACTOR;
                claim.push(index);
            }
        }
    }
    for index in claim {
        data.claim(index);
    }
    let total = data.total_cost.unwrap_or(0.0);
    finding.ev("peak_builders", peak as f64);
    finding.ev("buckets_over_cap", over.len() as f64);
    finding.ev("cost_over_cap_usd", over_cost);
    finding.ev(
        "share_with_many_agents",
        if total > 0.0 { crowded_cost / total } else { 0.0 },
    );
    if !over.is_empty() {
        finding.saving = Some(saving);
        finding.tokens_avoided = Some(tokens as u64);
    }
    finding
}

pub fn main_effort(data: &Data) -> (Option<Finding>, u64) {
    let (mut known, mut low, mut unknown) = (0u64, 0u64, 0u64);
    for call in data
        .calls
        .iter()
        .filter(|c| !c.sub && c.tool == TOOL_CLAUDE)
    {
        match call.effort.as_deref() {
            Some(effort) => {
                known += 1;
                low += u64::from(matches!(effort, "low" | "medium"));
            }
            None => unknown += 1,
        }
    }
    if known == 0 || (low as f64) < LOW_EFFORT_SHARE * known as f64 {
        return (None, if known == 0 { unknown } else { 0 });
    }
    let mut finding = Finding::new("main_effort");
    finding.ev("calls", known as f64);
    finding.ev("low_or_medium_share", low as f64 / known as f64);
    (Some(finding), 0)
}
