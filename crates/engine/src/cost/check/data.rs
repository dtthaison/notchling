use super::{Ttl, BUCKET};
use crate::catalog::{Catalog, Price, Tier, Tool};
use crate::cost::ledger::{Call, TOOL_ANTIGRAVITY, TOOL_CLAUDE, TOOL_CODEX};
use crate::cost::prices;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

pub struct Target {
    pub model: String,
    pub price: Price,
}

pub struct Data<'a> {
    pub calls: Vec<&'a Call>,
    pub streams: Vec<Range<usize>>,
    pub ttl: Vec<Ttl>,
    secs: Vec<Option<f64>>,
    pub catalog: &'a Catalog,
    pub target: Option<Target>,
    pub total_cost: Option<f64>,
    claimed: Vec<bool>,
}

pub fn tool_of(call: &Call) -> Option<Tool> {
    match call.tool.as_str() {
        TOOL_CLAUDE => Some(Tool::Claude),
        TOOL_CODEX => Some(Tool::Codex),
        TOOL_ANTIGRAVITY => Some(Tool::Antigravity),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tally {
    pub five: u64,
    pub hour: u64,
}

impl Tally {
    pub fn of<'a>(calls: impl IntoIterator<Item = &'a Call>) -> Tally {
        let mut tally = Tally::default();
        for call in calls.into_iter().filter(|c| c.split) {
            tally.five += call.write_5m;
            tally.hour += call.write_1h;
        }
        tally
    }

    pub fn add(&mut self, other: Tally) {
        self.five += other.five;
        self.hour += other.hour;
    }

    pub fn ttl(&self) -> Ttl {
        match (self.five > 0, self.hour > 0) {
            (true, true) => Ttl::Mixed,
            (true, false) => Ttl::FiveMin,
            (false, true) => Ttl::OneHour,
            (false, false) => Ttl::Unknown,
        }
    }

    pub fn secs(&self) -> Option<f64> {
        match self.ttl() {
            Ttl::Unknown => None,
            Ttl::FiveMin => Some(300.0),
            Ttl::OneHour => Some(3_600.0),
            Ttl::Mixed => Some(if self.hour >= self.five {
                3_600.0
            } else {
                300.0
            }),
        }
    }
}

pub fn ttl_of<'a>(calls: impl IntoIterator<Item = &'a Call>) -> Ttl {
    Tally::of(calls).ttl()
}

fn target_of(catalog: &Catalog) -> Option<Target> {
    let mut priced: Vec<(String, Price)> = catalog
        .models(Tool::Claude, &[])
        .into_iter()
        .filter(|m| m.tier == Tier::Current)
        .filter_map(|m| m.price.map(|p| (m.id, p)))
        .collect();
    priced.sort_by(|a, b| a.1.input.total_cmp(&b.1.input));
    let at = match priced.len() {
        0 | 1 => return None,
        2 => 0,
        _ => 1,
    };
    let (model, price) = priced.swap_remove(at);
    Some(Target { model, price })
}

impl<'a> Data<'a> {
    pub fn new(window: &'a [Call], catalog: &'a Catalog) -> Data<'a> {
        let mut calls: Vec<&Call> = window.iter().collect();
        calls.sort_by(|a, b| a.stream().cmp(&b.stream()).then(a.ts.total_cmp(&b.ts)));
        let mut streams: Vec<Range<usize>> = Vec::new();
        let mut start = 0;
        for i in 1..=calls.len() {
            if i == calls.len() || calls[i].stream() != calls[start].stream() {
                streams.push(start..i);
                start = i;
            }
        }
        let mut group: HashMap<(&str, bool), Tally> = HashMap::new();
        for range in &streams {
            let first = calls[range.start];
            group
                .entry((first.session.as_str(), first.sub))
                .or_default()
                .add(Tally::of(calls[range.clone()].iter().copied()));
        }
        let tallies: Vec<Tally> = streams
            .iter()
            .map(|range| {
                let first = calls[range.start];
                if first.tool != TOOL_CLAUDE {
                    return Tally::default();
                }
                let own = Tally::of(calls[range.clone()].iter().copied());
                if own.ttl() == Ttl::Unknown {
                    group
                        .get(&(first.session.as_str(), first.sub))
                        .copied()
                        .unwrap_or_default()
                } else {
                    own
                }
            })
            .collect();
        let ttl = tallies.iter().map(Tally::ttl).collect();
        let secs = tallies.iter().map(Tally::secs).collect();
        let priced: Vec<f64> = calls.iter().filter_map(|c| c.cost).collect();
        let total_cost = (!priced.is_empty()).then(|| priced.iter().sum());
        let claimed = vec![false; calls.len()];
        Data {
            calls,
            streams,
            ttl,
            secs,
            catalog,
            target: target_of(catalog),
            total_cost,
            claimed,
        }
    }

    pub fn price(&self, call: &Call) -> Option<Price> {
        tool_of(call).and_then(|tool| self.catalog.price(tool, &call.model))
    }

    pub fn tokens(call: &Call) -> [f64; 5] {
        [
            call.input as f64,
            call.output as f64,
            call.cache_read as f64,
            call.write_5m as f64,
            call.write_1h as f64,
        ]
    }

    pub fn rebuild_extra(&self, call: &Call) -> Option<f64> {
        let price = self.price(call)?;
        let writes = (call.write_5m + call.write_1h) as f64;
        let wrote = prices::estimate(
            &price,
            &[0.0, 0.0, 0.0, call.write_5m as f64, call.write_1h as f64],
        );
        let read = writes * price.cache_read / 1_000_000.0;
        Some((wrote - read).max(0.0))
    }

    pub fn is_claimed(&self, index: usize) -> bool {
        self.claimed[index]
    }

    pub fn claim(&mut self, index: usize) {
        self.claimed[index] = true;
    }

    pub fn ttl_secs(&self, stream: usize) -> Option<f64> {
        self.secs[stream]
    }

    pub fn buckets(&self) -> BTreeMap<i64, HashSet<usize>> {
        let mut out: BTreeMap<i64, HashSet<usize>> = BTreeMap::new();
        for (s, range) in self.streams.iter().enumerate() {
            for call in &self.calls[range.clone()] {
                out.entry((call.ts / BUCKET).floor() as i64)
                    .or_default()
                    .insert(s);
            }
        }
        out
    }
}
