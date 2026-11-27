use super::filter::{Filter, SqlArg};
use serde_json::{json, Value};

fn list(items: &[&str]) -> Option<Vec<String>> {
    Some(items.iter().map(|s| s.to_string()).collect())
}

#[test]
fn and_across_names_or_within() {
    let filter = Filter {
        tool: list(&["codex", "agy"]),
        project: list(&["/w/app"]),
        ..Filter::default()
    };
    assert!(filter.matches(&[("tool", "codex"), ("project", "/w/app")], None));
    assert!(filter.matches(&[("tool", "agy"), ("project", "/w/app")], None));
    assert!(!filter.matches(&[("tool", "claude"), ("project", "/w/app")], None));
    assert!(!filter.matches(&[("tool", "codex"), ("project", "/w/other")], None));
}

#[test]
fn a_missing_facet_does_not_match() {
    let filter = Filter {
        account: list(&["work"]),
        ..Filter::default()
    };
    assert!(!filter.matches(&[("tool", "codex")], None));
    assert!(Filter::default().matches(&[], None));
    let timed = Filter {
        after: Some(10.0),
        ..Filter::default()
    };
    assert!(!timed.matches(&[], None));
}

#[test]
fn bounds_are_half_open() {
    let filter = Filter {
        after: Some(100.0),
        before: Some(200.0),
        ..Filter::default()
    };
    assert!(filter.matches(&[], Some(100.0)));
    assert!(filter.matches(&[], Some(199.9)));
    assert!(!filter.matches(&[], Some(200.0)));
    assert!(!filter.matches(&[], Some(99.9)));
}

#[test]
fn sql_goldens() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("filter_golden.json")).unwrap();
    assert!(cases.len() >= 5);
    for case in cases {
        let filter = Filter::from_value(&case["filter"]).unwrap();
        let (sql, args) = filter.sql("d.id", "d.modified");
        assert_eq!(sql, case["sql"].as_str().unwrap(), "{case}");
        let want: Vec<SqlArg> = case["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| match a {
                Value::String(s) => SqlArg::Text(s.clone()),
                other => SqlArg::Real(other.as_f64().unwrap()),
            })
            .collect();
        assert_eq!(args, want, "{case}");
    }
}

#[test]
fn serde_round_trips_and_ignores_unknown_fields() {
    let filter = Filter {
        tool: list(&["codex"]),
        after: Some(5.0),
        ..Filter::default()
    };
    let text = serde_json::to_string(&filter).unwrap();
    assert_eq!(text, r#"{"tool":["codex"],"after":5.0}"#);
    let back: Filter = serde_json::from_str(&text).unwrap();
    assert_eq!(back, filter);
    let loose: Filter = serde_json::from_value(json!({"tool": ["x"], "future": 1})).unwrap();
    assert_eq!(loose.tool, list(&["x"]));
}

#[test]
fn from_value_names_the_bad_field() {
    let err = Filter::from_value(&json!({"tool": "codex"})).unwrap_err();
    assert!(err.contains("tool"), "{err}");
    let err = Filter::from_value(&json!({"after": "soon"})).unwrap_err();
    assert!(err.contains("after"), "{err}");
    let err = Filter::from_value(&json!("tool:codex")).unwrap_err();
    assert!(err.contains("filter"), "{err}");
    assert!(Filter::from_value(&json!({})).unwrap().is_empty());
    assert!(Filter::from_value(&Value::Null).unwrap().is_empty());
}

#[test]
fn names_lists_only_the_set_ones_in_order() {
    let filter = Filter {
        state: list(&["open"]),
        tool: list(&["codex"]),
        ..Filter::default()
    };
    let names: Vec<&str> = filter.names().iter().map(|(n, _)| *n).collect();
    assert_eq!(names, ["tool", "state"]);
}
