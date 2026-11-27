use super::evidence::{command_hash, output_hash, Evidence};
use super::model::{Asker, Found, FIND_MAX_FILES, FIND_WINDOW_MS};
use super::words::{is_notchling, same_command, simple_commands};
use crate::sessions::iso::parse_iso_ms;
use crate::transcript::{shells, subagents};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Main {
        name: Option<String>,
    },
    Subagent {
        id: String,
        description: String,
        agent_type: String,
        model: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub path: PathBuf,
    pub source: Source,
}

pub struct Asking<'a> {
    pub argv: &'a [String],
    pub stdout: Option<&'a Path>,
    pub opened_ms: i64,
    pub claimed: &'a HashSet<String>,
}

pub fn candidates(transcript: &Path, session_name: &str, now_ms: i64) -> Vec<Candidate> {
    let mut out = vec![Candidate {
        path: transcript.to_path_buf(),
        source: Source::Main {
            name: (!session_name.is_empty()).then(|| session_name.to_string()),
        },
    }];
    let agents = subagents::list_across_clears(
        &transcript.to_string_lossy(),
        session_name,
        &HashMap::new(),
        now_ms as f64 / 1000.0,
    );
    let mut recent: Vec<(i64, subagents::Subagent)> = agents
        .into_iter()
        .map(|agent| ((agent.updated * 1000.0) as i64, agent))
        .filter(|(written, _)| now_ms - written <= FIND_WINDOW_MS)
        .collect();
    recent.sort_by_key(|a| std::cmp::Reverse(a.0));
    recent.truncate(FIND_MAX_FILES.saturating_sub(1));
    out.extend(recent.into_iter().map(|(_, agent)| Candidate {
        path: PathBuf::from(&agent.path),
        source: Source::Subagent {
            id: agent.own_id().to_string(),
            description: agent.description,
            agent_type: agent.agent_type,
            model: agent.model,
        },
    }));
    out
}

pub fn delegate_tool(argv: &[String]) -> Option<Asker> {
    let program = Path::new(argv.first()?).file_name()?.to_str()?;
    if program != "notchling" || argv.get(1)?.as_str() != "delegate" {
        return None;
    }
    match argv.get(2)?.as_str() {
        "codex" => Some(Asker::Codex),
        "agy" => Some(Asker::Antigravity),
        _ => None,
    }
}

pub fn find(
    asking: &Asking,
    candidates: &[Candidate],
    tail: impl Fn(&Path) -> Option<String>,
) -> Option<Found> {
    let tails: Vec<Option<String>> = candidates.iter().map(|c| tail(&c.path)).collect();
    if let Some(stdout) = asking.stdout {
        let wanted = canon(stdout);
        let hits = hits(&tails, |text| {
            background_hit(text, &|path| canon(path) == wanted)
        });
        if let Some(found) = decide(candidates, &hits) {
            return Some(found);
        }
    }
    let hits = hits(&tails, |text| {
        command_hit(text, asking.opened_ms, asking.claimed, &|command| {
            runs(command, asking.argv)
        })
    });
    decide(candidates, &hits)
}

pub fn find_by_evidence(
    evidence: &Evidence,
    candidates: &[Candidate],
    tail: impl Fn(&Path) -> Option<String>,
) -> Option<Found> {
    let tails: Vec<Option<String>> = candidates.iter().map(|c| tail(&c.path)).collect();
    if let Some(wanted) = evidence.out_hash {
        let hits = hits(&tails, |text| {
            background_hit(text, &|path| output_hash(evidence.salt, path) == wanted)
        });
        if let Some(found) = decide(candidates, &hits) {
            return Some(found);
        }
    }
    let wanted = evidence.cmd_hash?;
    let none = HashSet::new();
    let hits = hits(&tails, |text| {
        command_hit(text, evidence.opened_ms, &none, &|command| {
            simple_commands(command).is_some_and(|all| {
                all.iter().any(|words| {
                    words.first().is_some_and(|w| is_notchling(w))
                        && command_hash(evidence.salt, words).as_deref() == Some(wanted)
                })
            })
        })
    });
    decide(candidates, &hits)
}

fn hits(tails: &[Option<String>], look: impl Fn(&str) -> Option<String>) -> Vec<(usize, String)> {
    tails
        .iter()
        .enumerate()
        .filter_map(|(at, text)| Some((at, look(text.as_deref()?)?)))
        .collect()
}

fn decide(candidates: &[Candidate], hits: &[(usize, String)]) -> Option<Found> {
    match hits {
        [] => None,
        [(at, tool_use)] => Some(found(&candidates[*at], tool_use)),
        _ => Some(unknown()),
    }
}

fn found(candidate: &Candidate, tool_use_id: &str) -> Found {
    let tool_use_id = Some(tool_use_id.to_string());
    match &candidate.source {
        Source::Main { name } => Found {
            asker: Asker::Main,
            agent_id: None,
            label: name.clone(),
            agent_type: None,
            model: None,
            tool_use_id,
        },
        Source::Subagent {
            id,
            description,
            agent_type,
            model,
        } => Found {
            asker: Asker::Subagent,
            agent_id: Some(id.clone()),
            label: Some(description.clone()),
            agent_type: Some(agent_type.clone()),
            model: model.clone(),
            tool_use_id,
        },
    }
}

fn unknown() -> Found {
    Found {
        asker: Asker::Unknown,
        agent_id: None,
        label: None,
        agent_type: None,
        model: None,
        tool_use_id: None,
    }
}

pub(super) fn canon(path: &Path) -> PathBuf {
    if let Ok(done) = path.canonicalize() {
        return done;
    }
    for ancestor in path.ancestors().skip(1) {
        if let (Ok(done), Ok(rest)) = (ancestor.canonicalize(), path.strip_prefix(ancestor)) {
            return done.join(rest);
        }
    }
    path.to_path_buf()
}

fn parsed<'a>(tail: &'a str, needle: &'a str) -> impl Iterator<Item = Value> + 'a {
    tail.lines()
        .filter(move |line| line.contains(needle))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
}

fn parts(line: &Value) -> impl Iterator<Item = &Value> {
    line.pointer("/message/content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn result_text(part: &Value) -> String {
    match part.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn background_hit(tail: &str, wanted: &dyn Fn(&Path) -> bool) -> Option<String> {
    let mut hit = None;
    for line in parsed(tail, "Output is being written to") {
        for part in parts(&line) {
            if part.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let Some(launch) = shells::launch(&result_text(part)) else {
                continue;
            };
            if wanted(Path::new(&launch.path)) {
                hit = part
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
        }
    }
    hit
}

fn command_hit(
    tail: &str,
    opened_ms: i64,
    claimed: &HashSet<String>,
    matches: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let mut best: Option<(i64, String)> = None;
    for line in parsed(tail, "tool_use") {
        let Some(stamp) = line
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_iso_ms)
        else {
            continue;
        };
        if stamp > opened_ms || opened_ms - stamp > FIND_WINDOW_MS {
            continue;
        }
        for part in parts(&line) {
            if part.get("type").and_then(Value::as_str) != Some("tool_use")
                || part.get("name").and_then(Value::as_str) != Some("Bash")
            {
                continue;
            }
            let Some(id) = part.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(command) = part.pointer("/input/command").and_then(Value::as_str) else {
                continue;
            };
            if claimed.contains(id) || !matches(command) {
                continue;
            }
            if best.as_ref().is_none_or(|(newest, _)| stamp >= *newest) {
                best = Some((stamp, id.to_string()));
            }
        }
    }
    best.map(|(_, id)| id)
}

fn runs(command: &str, argv: &[String]) -> bool {
    simple_commands(command).is_some_and(|all| all.iter().any(|words| same_command(words, argv)))
}
