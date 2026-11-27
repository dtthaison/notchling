use notchling_events::kinds::settings::{SettingKey, SettingValue};
use std::path::PathBuf;

fn keys_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine/src/settings/keys.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn quoted(text: &str) -> Vec<String> {
    text.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

fn table(source: &str) -> &str {
    let start = source.find("pub const KEYS").expect("the key table");
    &source[start..]
}

fn key_names(source: &str) -> Vec<String> {
    table(source)
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let rest = line
                .strip_prefix("key(\"")
                .or_else(|| line.strip_prefix("choice(\""))?;
            Some(rest.split('"').next()?.to_string())
        })
        .collect()
}

fn allowed_values(source: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in table(source).lines() {
        let line = line.trim_start();
        if !line.starts_with("choice(\"") {
            continue;
        }
        let Some(open) = line.find("&[") else {
            continue;
        };
        let Some(close) = line[open..].find(']') else {
            continue;
        };
        for value in quoted(&line[open..open + close]) {
            if !out.contains(&value) {
                out.push(value);
            }
        }
    }
    out
}

#[test]
fn setting_key_lists_every_key_in_keys_rs() {
    let source = keys_source();
    let mut want = key_names(&source);
    assert!(want.len() > 30, "parsed {} keys", want.len());
    let mut got: Vec<String> = SettingKey::VALUES
        .iter()
        .filter(|v| !matches!(**v, "unknown" | "other"))
        .map(|v| v.to_string())
        .collect();
    want.sort();
    got.sort();
    assert_eq!(got, want);
}

#[test]
fn setting_value_lists_every_allowed_choice() {
    let source = keys_source();
    let mut want = allowed_values(&source);
    assert!(want.len() >= 10, "parsed {} values", want.len());
    let mut got: Vec<String> = SettingValue::VALUES
        .iter()
        .filter(|v| **v != "other")
        .map(|v| v.to_string())
        .collect();
    want.sort();
    got.sort();
    assert_eq!(got, want);
}
