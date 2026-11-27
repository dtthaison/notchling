use notchling_engine::search::models::manifest::{manifest, ModelEntry};
use notchling_local_api::ops;
use notchling_locale::{self as t, lookup};
use serde_json::{json, Value};

use crate::search::embed_feed::{EmbedState, Meaning};

pub const KEY: &str = "semanticSearch";
pub const PAUSED_KEY: &str = "semanticPaused";

const MB: u64 = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelRow {
    Absent,
    Downloading { bytes: u64, total: u64 },
    Ready,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct SemanticModel {
    pub on: bool,
    pub rows: Vec<(&'static ModelEntry, ModelRow)>,
    pub indexing: Option<(u64, u64)>,
    pub paused: bool,
}

impl PartialEq for SemanticModel {
    fn eq(&self, other: &Self) -> bool {
        self.on == other.on
            && self.indexing == other.indexing
            && self.paused == other.paused
            && self.rows.len() == other.rows.len()
            && self
                .rows
                .iter()
                .zip(&other.rows)
                .all(|(a, b)| a.0.id == b.0.id && a.1 == b.1)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Download(String),
    Cancel(String),
    Remove(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub id: &'static str,
    pub title: &'static str,
    pub about: &'static str,
    pub recommended: bool,
    pub status: Option<String>,
    pub fraction: Option<f32>,
    pub button: (&'static str, &'static str),
}

pub fn is_semantic_key(key: &str) -> bool {
    key == KEY || key == PAUSED_KEY
}

fn fetch_row(status: &Value, id: &str) -> Option<ModelRow> {
    let row = status
        .get("fetch")?
        .as_array()?
        .iter()
        .find(|r| r.get("model").and_then(Value::as_str) == Some(id))?;
    let count = |k: &str| row.get(k).and_then(Value::as_u64).unwrap_or(0);
    match row.get("state").and_then(Value::as_str)? {
        "downloading" => Some(ModelRow::Downloading {
            bytes: count("bytes"),
            total: count("total"),
        }),
        "failed" => Some(ModelRow::Failed(
            row.get("error")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        )),
        _ => None,
    }
}

fn present(status: &Value, id: &str) -> bool {
    status
        .get("models")
        .and_then(Value::as_array)
        .is_some_and(|models| {
            models.iter().any(|m| {
                m.get("id").and_then(Value::as_str) == Some(id)
                    && m.get("present").and_then(Value::as_bool) == Some(true)
            })
        })
}

fn row_of(status: &Value, id: &str) -> ModelRow {
    fetch_row(status, id).unwrap_or(if present(status, id) {
        ModelRow::Ready
    } else {
        ModelRow::Absent
    })
}

pub fn model(on: bool, paused: bool, status: &Value) -> SemanticModel {
    if !on {
        return SemanticModel {
            on,
            rows: Vec::new(),
            indexing: None,
            paused,
        };
    }
    let mut listed: Vec<&'static ModelEntry> =
        manifest().models.iter().filter(|m| m.listed).collect();
    listed.sort_by_key(|m| !m.recommended);
    let meaning = Meaning::from_value(status);
    SemanticModel {
        on,
        rows: listed
            .into_iter()
            .map(|entry| (entry, row_of(status, &entry.id)))
            .collect(),
        indexing: meaning
            .filter(|m| m.state == EmbedState::Indexing && m.chunks > 0)
            .map(|m| (m.embedded as u64, m.chunks as u64)),
        paused,
    }
}

fn words(key: &str) -> &'static str {
    lookup(key).map_or("", |text| text.get())
}

fn about_key(message: &str) -> String {
    format!("{message}-about")
}

pub fn lines(model: &SemanticModel) -> Vec<Line> {
    model
        .rows
        .iter()
        .map(|(entry, row)| {
            let (status, fraction, button) = match row {
                ModelRow::Absent => (None, None, ("download", t::settings::model_download())),
                ModelRow::Downloading { bytes, total } => (
                    Some(t::settings::model_downloading(bytes / MB, total / MB)),
                    (*total > 0).then(|| (*bytes as f64 / *total as f64).clamp(0.0, 1.0) as f32),
                    ("cancel", t::settings::model_cancel()),
                ),
                ModelRow::Ready => (
                    Some(t::settings::model_ready().to_owned()),
                    None,
                    ("remove", t::settings::model_remove()),
                ),
                ModelRow::Failed(why) => (
                    Some(t::settings::model_failed(why)),
                    None,
                    ("download", t::settings::model_download()),
                ),
            };
            Line {
                id: entry.id.as_str(),
                title: words(&entry.message),
                about: words(&about_key(&entry.message)),
                recommended: entry.recommended,
                status,
                fraction,
                button,
            }
        })
        .collect()
}

pub fn indexing_text(model: &SemanticModel) -> Option<String> {
    model
        .indexing
        .map(|(done, total)| t::settings::semantic_indexing(done, total))
}

pub fn pause_button(model: &SemanticModel) -> (&'static str, &'static str) {
    if model.paused {
        ("resume", t::settings::semantic_resume())
    } else {
        ("pause", t::settings::semantic_pause())
    }
}

pub fn change(model: &SemanticModel, option: &str) -> Option<(&'static str, Value)> {
    match option {
        "" => Some((KEY, json!(!model.on))),
        "pause" => Some((PAUSED_KEY, json!(true))),
        "resume" => Some((PAUSED_KEY, json!(false))),
        _ => None,
    }
}

pub fn action(model: &SemanticModel, option: &str) -> Option<Action> {
    let (verb, id) = option.split_once(':')?;
    let id = model
        .rows
        .iter()
        .find(|(entry, _)| entry.id == id)
        .map(|(entry, _)| entry.id.clone())?;
    match verb {
        "download" => Some(Action::Download(id)),
        "cancel" => Some(Action::Cancel(id)),
        "remove" => Some(Action::Remove(id)),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Step {
    pub op: &'static str,
    pub model: String,
    pub remove: bool,
}

pub fn step(action: &Action) -> Step {
    let (op, model) = match action {
        Action::Download(id) => (ops::EMBED_FETCH, id.clone()),
        Action::Cancel(id) => (ops::EMBED_FETCH_CANCEL, id.clone()),
        Action::Remove(id) => (ops::EMBED_REMOVE, id.clone()),
    };
    Step {
        op,
        model,
        remove: matches!(action, Action::Remove(_)),
    }
}

#[cfg(test)]
#[path = "semantic_tests.rs"]
mod tests;
