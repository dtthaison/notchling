use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt::Write;

pub const FLOORS: usize = 19;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    pub q: String,
    pub want: Vec<String>,
    pub lang: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadLine {
    pub line: usize,
    pub why: String,
}

#[derive(Deserialize)]
struct Raw {
    q: Option<String>,
    #[serde(default)]
    want: Vec<String>,
    lang: Option<String>,
}

pub fn parse_queries(text: &str, known: &dyn Fn(&str) -> bool) -> (Vec<Query>, Vec<BadLine>) {
    let mut queries = Vec::new();
    let mut bad = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let line_no = i + 1;
        let mut reject = |why: String| bad.push(BadLine { line: line_no, why });
        let raw: Raw = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                reject(format!("not a query line: {e}"));
                continue;
            }
        };
        let Some(q) = raw.q.filter(|q| !q.trim().is_empty()) else {
            reject("no query text".to_string());
            continue;
        };
        if raw.want.is_empty() {
            reject("no want".to_string());
            continue;
        }
        if let Some(missing) = raw.want.iter().find(|id| !known(id)) {
            reject(format!("{missing} is not in the data root"));
            continue;
        }
        queries.push(Query {
            q,
            want: raw.want,
            lang: raw.lang.filter(|l| !l.is_empty()),
        });
    }
    (queries, bad)
}

pub fn recall(rankings: &[Vec<String>], wants: &[Vec<String>], at: usize) -> f64 {
    let n = rankings.len().min(wants.len());
    if n == 0 {
        return 0.0;
    }
    let hits = rankings
        .iter()
        .zip(wants)
        .filter(|(ranking, want)| ranking.iter().take(at).any(|id| want.contains(id)))
        .count();
    hits as f64 / n as f64
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Scores {
    pub r1: f64,
    pub r5: f64,
    pub n: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    pub rows: Vec<(String, Scores)>,
}

pub fn groups(queries: &[Query], exact: &[bool]) -> Vec<(String, Vec<usize>)> {
    let mut out = vec![("all".to_string(), (0..queries.len()).collect())];
    let langs: BTreeSet<&str> = queries.iter().filter_map(|q| q.lang.as_deref()).collect();
    for lang in langs {
        let members = (0..queries.len())
            .filter(|&i| queries[i].lang.as_deref() == Some(lang))
            .collect();
        out.push((lang.to_string(), members));
    }
    let members: Vec<usize> = (0..queries.len())
        .filter(|&i| exact.get(i).copied().unwrap_or(false))
        .collect();
    if !members.is_empty() {
        out.push(("exact".to_string(), members));
    }
    out
}

pub fn report(queries: &[Query], exact: &[bool], systems: &[(String, Vec<Vec<String>>)]) -> Report {
    let mut rows = Vec::new();
    for (group, members) in groups(queries, exact) {
        let wants: Vec<Vec<String>> = members.iter().map(|&i| queries[i].want.clone()).collect();
        for (system, rankings) in systems {
            let picked: Vec<Vec<String>> = members
                .iter()
                .map(|&i| rankings.get(i).cloned().unwrap_or_default())
                .collect();
            rows.push((
                format!("{group}/{system}"),
                Scores {
                    r1: recall(&picked, &wants, 1),
                    r5: recall(&picked, &wants, 5),
                    n: members.len(),
                },
            ));
        }
    }
    Report { rows }
}

pub fn render(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<34} {:>4} {:>6} {:>6}",
        "group/system", "n", "R@1", "R@5"
    );
    for (label, s) in &report.rows {
        let _ = writeln!(out, "{label:<34} {:>4} {:>6.3} {:>6.3}", s.n, s.r1, s.r5);
    }
    out
}

pub fn hybrid(
    exact: bool,
    text: &[String],
    dense: &[Vec<String>],
    fuse: &dyn Fn(&[&[String]]) -> Vec<String>,
) -> Vec<String> {
    if exact {
        return text.to_vec();
    }
    let mut lists: Vec<&[String]> = vec![text];
    lists.extend(dense.iter().map(Vec::as_slice));
    fuse(&lists)
}

pub fn above_floor(list: &[(String, f32)], floor: f32) -> Vec<String> {
    list.iter()
        .filter(|(_, score)| floor <= 0.0 || *score >= floor)
        .map(|(id, _)| id.clone())
        .collect()
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FloorRow {
    pub floor: f32,
    pub r5: f64,
    pub meaning_only: usize,
    pub lost: f64,
}

pub fn sweep(
    wants: &[Vec<String>],
    text: &[Vec<String>],
    dense: &[Vec<(String, f32)>],
    exact: &[bool],
    fuse: &dyn Fn(&[&[String]]) -> Vec<String>,
) -> Vec<FloorRow> {
    let best: Vec<Option<f32>> = wants
        .iter()
        .zip(dense)
        .map(|(want, list)| {
            let hit = list.iter().take(5).find(|(id, _)| want.contains(id));
            hit.map(|(_, score)| *score)
        })
        .collect();
    let meaning: Vec<f32> = best
        .iter()
        .enumerate()
        .filter_map(|(i, score)| {
            let text_hit = text[i].iter().take(5).any(|id| wants[i].contains(id));
            let is_exact = exact.get(i).copied().unwrap_or(false);
            (!text_hit && !is_exact).then_some(*score).flatten()
        })
        .collect();
    (0..FLOORS)
        .map(|step| {
            let floor = step as f32 * 0.05;
            let rankings: Vec<Vec<String>> = (0..wants.len())
                .map(|i| {
                    hybrid(
                        exact.get(i).copied().unwrap_or(false),
                        &text[i],
                        &[above_floor(&dense[i], floor)],
                        fuse,
                    )
                })
                .collect();
            let lost = if meaning.is_empty() {
                0.0
            } else {
                meaning.iter().filter(|&&s| s < floor).count() as f64 / meaning.len() as f64
            };
            FloorRow {
                floor,
                r5: recall(&rankings, wants, 5),
                meaning_only: meaning.len(),
                lost,
            }
        })
        .collect()
}

#[cfg(test)]
mod eval_tests;
