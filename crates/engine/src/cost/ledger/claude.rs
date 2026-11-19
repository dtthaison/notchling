use super::{Call, TOOL_CLAUDE};
use crate::catalog::{Catalog, Tool};
use crate::cost::paths::hash;
use crate::cost::prices;
use crate::transcript::util::parse_date;
use serde_json::Value;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ctx {
    pub session: String,
    pub agent: String,
    pub sub: bool,
    pub agent_type: Option<String>,
    pub effort: Option<String>,
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn ctx_of(path: &Path) -> Ctx {
    let parts: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let n = parts.len();
    if n >= 3 && parts[n - 2] == "subagents" {
        let meta = std::fs::read(path.with_extension("meta.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .unwrap_or(Value::Null);
        return Ctx {
            session: parts[n - 3].clone(),
            agent: stem,
            sub: true,
            agent_type: text(&meta, "agentType"),
            effort: text(&meta, "effort"),
        };
    }
    Ctx {
        session: stem,
        ..Ctx::default()
    }
}

pub fn parse(object: &Value, ctx: &Ctx, catalog: &Catalog) -> Option<(u64, Call)> {
    if object.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let message = object.get("message")?;
    let model = message.get("model").and_then(Value::as_str)?;
    let usage = message.get("usage").filter(|u| u.is_object())?;
    let id = message.get("id").and_then(Value::as_str)?;
    let ts = object
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_date)?;
    if model == "<synthetic>" {
        return None;
    }
    let request = object
        .get("requestId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let [input, output, read, w5, w1] = prices::tokens(usage);
    if input + output + read + w5 + w1 == 0.0 {
        return None;
    }
    let key = prices::model_key(model);
    let cost = catalog
        .price(Tool::Claude, &key)
        .map(|p| prices::cost_at(&p, usage));
    let sidechain = object.get("isSidechain").and_then(Value::as_bool) == Some(true);
    let sub = ctx.sub || sidechain;
    let session = if ctx.session.is_empty() {
        text(object, "sessionId").unwrap_or_default()
    } else {
        ctx.session.clone()
    };
    let agent = if !sub {
        String::new()
    } else if ctx.agent.is_empty() {
        text(object, "agentId").unwrap_or_else(|| "sidechain".into())
    } else {
        ctx.agent.clone()
    };
    let effort = text(object, "effort").or_else(|| ctx.effort.clone());
    Some((
        hash(&format!("{id}|{request}")),
        Call {
            ts,
            tool: TOOL_CLAUDE.into(),
            session,
            agent,
            sub,
            agent_type: ctx.agent_type.clone(),
            model: key,
            effort,
            input: input as u64,
            output: output as u64,
            cache_read: read as u64,
            write_5m: w5 as u64,
            write_1h: w1 as u64,
            split: usage.get("cache_creation").is_some_and(|c| {
                c.get("ephemeral_5m_input_tokens").is_some()
                    || c.get("ephemeral_1h_input_tokens").is_some()
            }),
            cost,
        },
    ))
}
