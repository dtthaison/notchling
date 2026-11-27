use notchling_events::doc::{golden_json, markdown};
use notchling_events::registry;
use std::path::PathBuf;

fn updating() -> bool {
    std::env::var("NOTCHLING_UPDATE_GOLDEN").is_ok_and(|v| v == "1")
}

fn compare(path: PathBuf, want: String) {
    if updating() {
        std::fs::write(&path, &want).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        return;
    }
    let have = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(
        have == want,
        "{} is out of date: run NOTCHLING_UPDATE_GOLDEN=1 cargo test -p notchling-events --test golden and read the diff",
        path.display()
    );
}

#[test]
fn kinds_match_the_golden() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/log-kinds.json");
    compare(path, golden_json());
}

#[test]
fn golden_is_valid_json_with_every_kind_once() {
    let parsed: serde_json::Value = serde_json::from_str(&golden_json()).expect("valid json");
    let kinds = parsed.as_array().expect("an array");
    assert_eq!(kinds.len(), registry::all().len());
    let names: Vec<&str> = kinds.iter().filter_map(|k| k["name"].as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len());
    assert!(golden_json().ends_with("}\n]\n"));
}

#[test]
fn docs_list_every_kind() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/log-kinds.md");
    let text = markdown();
    for spec in registry::all() {
        assert!(text.contains(&format!("`{}`", spec.name)), "{}", spec.name);
    }
    compare(path, text);
}
