use super::query::fuse::{recency, rrf, List, K};
use super::query::shape::text_only;
use super::store::DocHit;
use std::collections::HashMap;

pub const FLOOR_GRANITE: f32 = 0.80;
pub const FLOOR_STATIC: f32 = 0.0;
pub const TOP_TEXT: usize = 100;
pub const TOP_CHUNKS: usize = 300;
const TEXT_OR_WEIGHT: f64 = 0.5;
const CROWD: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Semantic {
    Off,
    NoModel,
    Warming,
    Indexing,
    Ready,
    Paused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Match {
    Text,
    Semantic,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub text: bool,
    pub statics: bool,
    pub granite: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Floors {
    pub granite: f32,
    pub statics: f32,
}

impl Default for Floors {
    fn default() -> Floors {
        Floors {
            granite: FLOOR_GRANITE,
            statics: FLOOR_STATIC,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Lists {
    pub text: Vec<String>,
    pub text_or: bool,
    pub granite: Option<Vec<DocHit>>,
    pub statics: Option<Vec<DocHit>>,
    pub floors: Floors,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ranked {
    pub key: String,
    pub score: f64,
    pub matched: Match,
    pub chunk: Option<[u8; 16]>,
}

pub fn plan(query: &str, state: Semantic) -> Plan {
    let semantic = !matches!(state, Semantic::Off | Semantic::NoModel) && !text_only(query);
    Plan {
        text: true,
        statics: semantic,
        granite: semantic
            && matches!(
                state,
                Semantic::Ready | Semantic::Indexing | Semantic::Paused
            ),
    }
}

fn above(hits: &Option<Vec<DocHit>>, floor: f32) -> Vec<&DocHit> {
    hits.iter().flatten().filter(|h| h.score >= floor).collect()
}

pub fn fuse(lists: &Lists, cap: usize, age_secs: &dyn Fn(&str) -> Option<f64>) -> Vec<Ranked> {
    fuse_with(lists, cap, age_secs, lists.floors)
}

pub fn fuse_with(
    lists: &Lists,
    cap: usize,
    age_secs: &dyn Fn(&str) -> Option<f64>,
    floors: Floors,
) -> Vec<Ranked> {
    let granite = above(&lists.granite, floors.granite);
    let statics = above(&lists.statics, floors.statics);
    let ids = |hits: &[&DocHit]| -> Vec<String> { hits.iter().map(|h| h.key.clone()).collect() };
    let (granite_ids, static_ids) = (ids(&granite), ids(&statics));
    let weight = if lists.text_or { TEXT_OR_WEIGHT } else { 1.0 };
    let ranked = rrf(
        &[
            List {
                ids: &lists.text,
                weight,
            },
            List {
                ids: &granite_ids,
                weight: 1.0,
            },
            List {
                ids: &static_ids,
                weight: 1.0,
            },
        ],
        K,
    );
    let ranked = recency(ranked, age_secs);
    let mut chunks: HashMap<&str, [u8; 16]> = HashMap::new();
    for hit in statics.iter().rev().chain(granite.iter().rev()) {
        chunks.insert(hit.key.as_str(), hit.chunk);
    }
    let in_text: std::collections::HashSet<&str> = lists.text.iter().map(String::as_str).collect();
    let crowded = lists.text.len() >= CROWD;
    let room = cap / 2;
    let mut meaning_only = 0;
    let mut out = Vec::new();
    for (key, score) in ranked {
        let text = in_text.contains(key.as_str());
        let semantic = chunks.contains_key(key.as_str());
        let matched = match (text, semantic) {
            (true, true) => Match::Both,
            (true, false) => Match::Text,
            _ => Match::Semantic,
        };
        if matched == Match::Semantic && crowded {
            if meaning_only >= room {
                continue;
            }
            meaning_only += 1;
        }
        let chunk = chunks.get(key.as_str()).copied();
        out.push(Ranked {
            key,
            score,
            matched,
            chunk,
        });
        if out.len() >= cap {
            break;
        }
    }
    out
}
