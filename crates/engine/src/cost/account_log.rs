use crate::transcript::util::parse_date;
use serde_json::Value;
use std::path::Path;

const REFERENCE_DATE: f64 = 978_307_200.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub at: f64,
    pub id: String,
}

pub fn load(file: &Path) -> Vec<Entry> {
    let Ok(data) = std::fs::read(file) else {
        return Vec::new();
    };
    parse(&data)
}

pub fn parse(data: &[u8]) -> Vec<Entry> {
    let Ok(Value::Array(items)) = serde_json::from_slice::<Value>(data) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = items
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.to_string();
            let at = match item.get("at")? {
                Value::Number(n) => n.as_f64()? + REFERENCE_DATE,
                Value::String(s) => parse_date(s)?,
                _ => return None,
            };
            Some(Entry { at, id })
        })
        .collect();
    entries.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    entries
}

pub fn account_at(date: f64, entries: &[Entry]) -> String {
    entries
        .iter()
        .rev()
        .find(|e| e.at <= date)
        .map(|e| e.id.clone())
        .unwrap_or_default()
}
