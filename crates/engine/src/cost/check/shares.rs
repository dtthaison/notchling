use super::data::{Data, Tally};
use super::{CacheTimes, Share, Totals, Ttl, TtlCounts, Unknown};
use crate::cost::ledger::{Call, TOOL_CLAUDE};
use std::collections::BTreeMap;

pub fn totals(data: &Data) -> Totals {
    Totals {
        calls: data.calls.len() as u64,
        tokens: data.calls.iter().map(|c| c.tokens()).sum(),
        cost: data.total_cost,
        unpriced_calls: data.calls.iter().filter(|c| c.cost.is_none()).count() as u64,
        unpriced_tokens: data
            .calls
            .iter()
            .filter(|c| c.cost.is_none())
            .map(|c| c.tokens())
            .sum(),
    }
}

#[derive(Default)]
struct Group {
    calls: u64,
    tokens: u64,
    cost: Option<f64>,
    input: u64,
    output: u64,
    cached: u64,
    unpriced_calls: u64,
}

fn shares(data: &Data, key_of: impl Fn(&Call) -> String) -> Vec<Share> {
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for call in &data.calls {
        let entry = groups.entry(key_of(call)).or_default();
        entry.calls += 1;
        entry.tokens += call.tokens();
        entry.input += call.input;
        entry.output += call.output;
        entry.cached += call.cache_read + call.writes();
        match call.cost {
            Some(cost) => entry.cost = Some(entry.cost.unwrap_or(0.0) + cost),
            None => entry.unpriced_calls += 1,
        }
    }
    let all_tokens: u64 = data.calls.iter().map(|c| c.tokens()).sum();
    let mut out: Vec<Share> = groups
        .into_iter()
        .map(|(key, group)| Share {
            key,
            calls: group.calls,
            tokens: group.tokens,
            input: group.input,
            output: group.output,
            cached: group.cached,
            unpriced_calls: group.unpriced_calls,
            cost: group.cost,
            cost_share: group
                .cost
                .zip(data.total_cost)
                .filter(|(_, t)| *t > 0.0)
                .map(|(c, t)| c / t),
            token_share: if all_tokens > 0 {
                group.tokens as f64 / all_tokens as f64
            } else {
                0.0
            },
        })
        .collect();
    out.sort_by(|a, b| {
        b.cost
            .unwrap_or(-1.0)
            .total_cmp(&a.cost.unwrap_or(-1.0))
            .then(b.tokens.cmp(&a.tokens))
            .then(a.key.cmp(&b.key))
    });
    out
}

pub fn by_role(data: &Data) -> Vec<Share> {
    shares(data, |c| if c.sub { "sub" } else { "main" }.to_string())
}

pub fn by_model(data: &Data) -> Vec<Share> {
    shares(data, |c| c.model.clone())
}

fn bump(counts: &mut TtlCounts, ttl: Ttl) {
    match ttl {
        Ttl::FiveMin => counts.five_min += 1,
        Ttl::OneHour => counts.one_hour += 1,
        Ttl::Mixed => counts.mixed += 1,
        Ttl::Unknown => counts.unknown += 1,
    }
}

pub fn cache_times(data: &Data) -> CacheTimes {
    let mut out = CacheTimes::default();
    let mut sessions: BTreeMap<&str, Tally> = BTreeMap::new();
    for range in &data.streams {
        let first = data.calls[range.start];
        if first.tool != TOOL_CLAUDE {
            continue;
        }
        let own = Tally::of(data.calls[range.clone()].iter().copied());
        if first.sub {
            bump(&mut out.sub, own.ttl());
        } else {
            sessions.entry(&first.session).or_default().add(own);
        }
    }
    for tally in sessions.values() {
        bump(&mut out.main, tally.ttl());
    }
    out
}

pub fn unknown(data: &Data) -> Vec<Unknown> {
    let mut out = Vec::new();
    let mut add = |id: &'static str, count: u64| {
        if count > 0 {
            out.push(Unknown { id, count });
        }
    };
    add(
        "price",
        data.calls.iter().filter(|c| c.cost.is_none()).count() as u64,
    );
    let blind: u64 = data
        .streams
        .iter()
        .enumerate()
        .filter(|(s, range)| {
            data.ttl[*s] == Ttl::Unknown
                && data.calls[range.start].tool == TOOL_CLAUDE
                && data.calls[(*range).clone()].iter().any(|c| c.writes() > 0)
        })
        .map(|(_, range)| range.len() as u64)
        .sum();
    add("cache_ttl", blind);
    add(
        "agent_type",
        data.calls
            .iter()
            .filter(|c| c.sub && c.tool == TOOL_CLAUDE && c.agent_type.is_none())
            .count() as u64,
    );
    out
}
