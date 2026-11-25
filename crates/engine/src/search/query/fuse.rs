use crate::search::RECENT_SECS;
use std::collections::HashMap;

pub const K: f64 = 60.0;
pub const RECENT_BONUS: f64 = 0.5;

pub struct List<'a> {
    pub ids: &'a [String],
    pub weight: f64,
}

pub fn rrf(lists: &[List], k: f64) -> Vec<(String, f64)> {
    let mut order: Vec<&str> = Vec::new();
    let mut scores: HashMap<&str, f64> = HashMap::new();
    for list in lists {
        let mut seen: Vec<&str> = Vec::new();
        for (i, id) in list.ids.iter().enumerate() {
            let id = id.as_str();
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            let entry = scores.entry(id).or_insert_with(|| {
                order.push(id);
                0.0
            });
            *entry += list.weight / (k + (i + 1) as f64);
        }
    }
    let mut out: Vec<(String, f64)> = order
        .into_iter()
        .map(|id| (id.to_string(), scores[id]))
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

pub fn recency(
    fused: Vec<(String, f64)>,
    age_secs: &dyn Fn(&str) -> Option<f64>,
) -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = fused
        .into_iter()
        .map(|(id, score)| {
            let lift = match age_secs(&id) {
                Some(age) => 1.0 + RECENT_BONUS * (1.0 - age / RECENT_SECS).max(0.0),
                None => 1.0,
            };
            (id, score * lift)
        })
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}
