use super::{Call, TOOL_ANTIGRAVITY, TOOL_CODEX};
use crate::catalog::{Catalog, Tool};
use crate::cost::other::{antigravity, codex, OtherPrices};
use crate::cost::paths::{hash, is_uuid};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    pub model: Option<String>,
    pub total: Option<f64>,
}

impl Reading {
    pub fn load(text: &str) -> Reading {
        let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        Reading {
            model: v.get("model").and_then(Value::as_str).map(String::from),
            total: v.get("total").and_then(Value::as_f64),
        }
    }

    pub fn save(&self) -> String {
        json!({"model": self.model, "total": self.total}).to_string()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ctx {
    pub session: String,
    pub sub: bool,
    pub agent_type: Option<String>,
    pub file_id: String,
}

pub fn codex_ctx(path: &Path, first: Option<&Value>, file_id: &str) -> Ctx {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tail = stem
        .get(stem.len().saturating_sub(36)..)
        .filter(|t| is_uuid(t))
        .map(String::from);
    let payload = first.and_then(|f| f.get("payload"));
    let id = payload
        .and_then(|p| p.get("id"))
        .and_then(Value::as_str)
        .map(String::from);
    let sub = payload.is_some_and(|p| {
        p.get("source")
            .and_then(Value::as_object)
            .is_some_and(|s| s.contains_key("subagent"))
            || p.get("subagent_history_start_ordinal").is_some()
    });
    let agent_type = payload
        .and_then(|p| p.get("agent_role"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from);
    Ctx {
        session: id.or(tail).unwrap_or(stem),
        sub,
        agent_type,
        file_id: file_id.to_string(),
    }
}

pub fn antigravity_ctx(path: &Path, file_id: &str) -> Ctx {
    let parts: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let session = parts
        .iter()
        .position(|p| p == "brain")
        .and_then(|i| parts.get(i + 1))
        .cloned()
        .unwrap_or_else(|| {
            path.parent()
                .and_then(Path::file_name)
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    Ctx {
        session,
        sub: false,
        agent_type: None,
        file_id: file_id.to_string(),
    }
}

fn row(ctx: &Ctx, tool: &str, ts: f64, model: String, counts: [f64; 4], cost: Option<f64>) -> Call {
    Call {
        ts,
        tool: tool.into(),
        session: ctx.session.clone(),
        agent: if ctx.sub {
            ctx.session.clone()
        } else {
            String::new()
        },
        sub: ctx.sub,
        agent_type: ctx.agent_type.clone(),
        model,
        effort: None,
        input: counts[0] as u64,
        output: counts[1] as u64,
        cache_read: counts[2] as u64,
        write_5m: counts[3] as u64,
        write_1h: 0,
        split: false,
        cost,
    }
}

pub fn codex_call(
    object: &Value,
    ctx: &Ctx,
    reading: &mut Reading,
    catalog: &Catalog,
) -> Option<(u64, Call)> {
    match codex::parse(object)? {
        codex::Event::Session { .. } => None,
        codex::Event::Turn { model, .. } => {
            if let Some(model) = model {
                reading.model = Some(model);
            }
            None
        }
        codex::Event::Tokens(t) => {
            if t.total >= 0.0 {
                if reading.total == Some(t.total) {
                    return None;
                }
                reading.total = Some(t.total);
            }
            let counts = codex::counts(&t);
            if counts.iter().all(|c| *c == 0.0) {
                return None;
            }
            let id = match t.ordinal {
                Some(o) => format!("{o}"),
                None => format!("{}", t.date),
            };
            let model = reading.model.clone().unwrap_or_else(|| "codex".into());
            let cost = OtherPrices::cost(catalog, Tool::Codex, &model, &counts);
            Some((
                hash(&format!("codex|{}|{id}", ctx.file_id)),
                row(ctx, TOOL_CODEX, t.date, model, counts, cost),
            ))
        }
    }
}

pub fn antigravity_call(
    object: &Value,
    ctx: &Ctx,
    reading: &mut Reading,
    catalog: &Catalog,
) -> Option<(u64, Call)> {
    match antigravity::parse(object)? {
        antigravity::Event::Model(name) => {
            reading.model = Some(name);
            None
        }
        antigravity::Event::Tokens(t) => {
            let counts = antigravity::counts(&t);
            if counts.iter().all(|c| *c == 0.0) {
                return None;
            }
            let model = reading
                .model
                .clone()
                .unwrap_or_else(|| "Antigravity".into());
            let cost = OtherPrices::cost(catalog, Tool::Antigravity, &model, &counts);
            Some((
                hash(&format!("agy|{}|{}", ctx.file_id, t.step)),
                row(ctx, TOOL_ANTIGRAVITY, t.date, model, counts, cost),
            ))
        }
    }
}
