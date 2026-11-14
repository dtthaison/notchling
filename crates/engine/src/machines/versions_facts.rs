use crate::store;
use crate::versions::WorkerTools;
use serde_json::{Map, Value};
use std::path::Path;

pub const DOC: &str = "versions/workers";

pub fn same(a: &WorkerTools, b: &WorkerTools) -> bool {
    a.tools == b.tools
}

fn parse(old: Option<String>) -> Map<String, Value> {
    old.and_then(|text| serde_json::from_str::<Map<String, Value>>(&text).ok())
        .unwrap_or_default()
}

fn text(map: &Map<String, Value>) -> Result<Option<String>, String> {
    serde_json::to_string(map)
        .map(Some)
        .map_err(|e| e.to_string())
}

pub fn save(data: &Path, machine: &str, tools: &WorkerTools) -> Result<(), String> {
    let value = serde_json::to_value(tools).map_err(|e| e.to_string())?;
    store::update_doc(data, DOC, |old| {
        let mut map = parse(old);
        map.insert(machine.to_string(), value);
        text(&map)
    })
    .map(|_| ())
}

pub fn prune(data: &Path, names: &[String]) -> Result<(), String> {
    let Some(current) = store::doc(data, DOC)? else {
        return Ok(());
    };
    let map = parse(Some(current));
    if map.keys().all(|k| names.contains(k)) {
        return Ok(());
    }
    store::update_doc(data, DOC, |old| {
        let mut map = parse(old);
        map.retain(|k, _| names.contains(k));
        text(&map)
    })
    .map(|_| ())
}
