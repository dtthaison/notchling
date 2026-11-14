use super::stamped::{Stamp, Stamped};
use crate::transcript::util::{parse_object, str_of};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

const FIRST_LINE_BYTES: u64 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lineage {
    pub parent: String,
    pub depth: u8,
    pub nickname: Option<String>,
    pub role: Option<String>,
    pub agent_path: Option<String>,
}

pub(super) fn first_meta(path: &str) -> Option<Value> {
    let mut line = Vec::new();
    BufReader::new(File::open(path).ok()?.take(FIRST_LINE_BYTES))
        .read_until(b'\n', &mut line)
        .ok()?;
    let obj = Value::Object(parse_object(&line)?);
    if str_of(&obj, "type") != Some("session_meta") {
        return None;
    }
    obj.get("payload").filter(|p| p.is_object()).cloned()
}

fn named(value: Option<&Value>, key: &str) -> Option<String> {
    value
        .and_then(|v| str_of(v, key))
        .filter(|s| !s.is_empty())
        .map(String::from)
}

pub(super) fn of_payload(payload: &Value) -> Option<Lineage> {
    let subagent = payload.get("source").and_then(|s| s.get("subagent"));
    let spawn = subagent.and_then(|s| s.get("thread_spawn"));
    let parent =
        named(Some(payload), "parent_thread_id").or_else(|| named(spawn, "parent_thread_id"));
    let marked = str_of(payload, "thread_source") == Some("subagent") || subagent.is_some();
    if parent.is_none() && !marked {
        return None;
    }
    Some(Lineage {
        parent: parent.unwrap_or_default(),
        depth: spawn
            .and_then(|s| s.get("depth"))
            .and_then(Value::as_u64)
            .map_or(1, |d| u8::try_from(d).unwrap_or(u8::MAX)),
        nickname: named(Some(payload), "agent_nickname").or_else(|| named(spawn, "agent_nickname")),
        role: named(Some(payload), "agent_role").or_else(|| named(spawn, "agent_role")),
        agent_path: named(Some(payload), "agent_path").or_else(|| named(spawn, "agent_path")),
    })
}

pub fn lineage(path: &str) -> Option<Lineage> {
    static KEPT: Stamped<Option<Lineage>> = Stamped::new();
    let stamp = Stamp::of(Path::new(path))?;
    KEPT.get(path, stamp, || {
        first_meta(path).as_ref().and_then(of_payload)
    })
}
