use notchling_protocol::generated::roundtrip;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn read(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn fixtures() -> Vec<(String, String, Value)> {
    let mut found = Vec::new();
    for entry in fs::read_dir(repo_file("protocol/fixtures")).expect("protocol/fixtures") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let file = path
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        let name = file.split('.').next().expect("name").to_string();
        found.push((file, name, read(&path)));
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

fn schema_defaults(node: &Value, into: &mut BTreeMap<String, Value>) {
    match node {
        Value::Object(map) => {
            if let Some(Value::Object(properties)) = map.get("properties") {
                for (name, property) in properties {
                    if let Some(default) = property.get("default") {
                        let old = into.insert(name.clone(), default.clone());
                        assert!(
                            old.is_none() || old.as_ref() == Some(default),
                            "{name} has two different defaults in the schema"
                        );
                    }
                }
            }
            map.values().for_each(|v| schema_defaults(v, into));
        }
        Value::Array(items) => items.iter().for_each(|v| schema_defaults(v, into)),
        _ => {}
    }
}

fn compare(
    input: &Value,
    output: &Value,
    defaults: &BTreeMap<String, Value>,
    path: &str,
    problems: &mut Vec<String>,
) {
    match (input, output) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                match b.get(key) {
                    Some(other) => {
                        compare(value, other, defaults, &format!("{path}.{key}"), problems)
                    }
                    None => problems.push(format!("{path}.{key} was dropped")),
                }
            }
            for (key, value) in b {
                if a.contains_key(key) {
                    continue;
                }
                if defaults.get(key) != Some(value) {
                    problems.push(format!(
                        "{path}.{key} = {value} appeared and is not a schema default"
                    ));
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                problems.push(format!(
                    "{path} has {} items, wrote back {}",
                    a.len(),
                    b.len()
                ));
                return;
            }
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                compare(x, y, defaults, &format!("{path}[{index}]"), problems);
            }
        }
        (Value::Number(a), Value::Number(b)) => {
            if a.as_f64() != b.as_f64() {
                problems.push(format!("{path}: {a} became {b}"));
            }
        }
        (a, b) => {
            if a != b {
                problems.push(format!("{path}: {a} became {b}"));
            }
        }
    }
}

#[test]
fn every_fixture_round_trips_through_its_generated_type() {
    let mut defaults = BTreeMap::new();
    schema_defaults(
        &read(&repo_file("protocol/agentconsole.v1.schema.json")),
        &mut defaults,
    );
    assert!(!defaults.is_empty(), "the schema should name some defaults");

    let all = fixtures();
    assert!(all.len() > 60, "found only {} fixtures", all.len());
    for (file, name, input) in all {
        let output = roundtrip(&name, &input)
            .unwrap_or_else(|| panic!("{file}: the generated code has no type named {name}"))
            .unwrap_or_else(|e| panic!("{file} does not parse as {name}: {e}"));
        let mut problems = Vec::new();
        compare(&input, &output, &defaults, "$", &mut problems);
        assert!(problems.is_empty(), "{file}: {problems:#?}");
        let again = roundtrip(&name, &output).expect("known").expect("parses");
        assert_eq!(output, again, "{file}: a second round trip differs");
    }
}

#[test]
fn a_field_the_schema_does_not_know_does_not_break_parsing() {
    let mut input = read(&repo_file("protocol/fixtures/SessionsListResponse.json"));
    input["data"]["sessions"][0]["somethingNew"] = Value::Bool(true);
    input["somethingElse"] = Value::from(3);
    let output = roundtrip("SessionsListResponse", &input)
        .expect("known")
        .expect("parses");
    assert_eq!(
        output["data"]["sessions"][0]["id"],
        input["data"]["sessions"][0]["id"]
    );
}

#[test]
fn an_older_mac_without_the_additive_fields_still_parses() {
    let mut input = read(&repo_file("protocol/fixtures/SessionsListResponse.json"));
    let first = input["data"]["sessions"][0]
        .as_object_mut()
        .expect("session");
    for key in [
        "tool",
        "subagents",
        "facts",
        "activity",
        "lastMessage",
        "pending",
        "takeover",
    ] {
        first.remove(key);
    }
    let output = roundtrip("SessionsListResponse", &input)
        .expect("known")
        .expect("parses");
    assert_eq!(output["data"]["sessions"][0]["tool"], "claude");
    assert_eq!(
        output["data"]["sessions"][0]["subagents"],
        Value::Array(vec![])
    );
    assert!(output["data"]["sessions"][0].get("facts").is_none());
}

#[test]
fn a_required_field_missing_is_an_error() {
    let mut input = read(&repo_file("protocol/fixtures/PingResponse.json"));
    input.as_object_mut().expect("object").remove("ok");
    assert!(roundtrip("PingResponse", &input).expect("known").is_err());
}

#[test]
fn an_unknown_enum_value_parses_as_unknown() {
    let mut input = read(&repo_file("protocol/fixtures/ErrorResponse.busy.json"));
    input["error"]["code"] = Value::from("a_code_from_the_future");
    let output = roundtrip("ErrorResponse", &input)
        .expect("known")
        .expect("parses");
    assert_eq!(output["error"]["code"], "Unknown");
}
