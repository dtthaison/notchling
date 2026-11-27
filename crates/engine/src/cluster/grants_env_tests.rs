use std::collections::BTreeMap;

use super::grant_files::GrantValue;
use super::grants::{decode_env, encode_env};

#[test]
fn an_environment_survives_the_round_trip_without_showing_its_values() {
    let mut env = BTreeMap::new();
    env.insert("GH_TOKEN".to_string(), "NL-MARKER-1".to_string());
    env.insert("GH_USER".to_string(), "line\nbreak \"quoted\"".to_string());
    let value = encode_env(&env).expect("encoded");
    let shown = format!("{value:?} {value}");
    assert!(!shown.contains("NL-MARKER"), "the value is hidden");
    let back = decode_env(&value).expect("decoded");
    assert_eq!(back.len(), 2);
    assert!(back["GH_TOKEN"].expose() == "NL-MARKER-1");
    assert!(back["GH_USER"].expose() == "line\nbreak \"quoted\"");
    let printed = format!("{back:?}");
    assert!(!printed.contains("NL-MARKER"), "decoded values are hidden");
}

#[test]
fn a_malformed_environment_is_refused_without_quoting_it() {
    let bad = GrantValue::new("NL-MARKER-2 not json".to_string());
    let err = decode_env(&bad).expect_err("refused");
    assert!(!err.contains("NL-MARKER"), "the error quotes nothing");
    let not_text = GrantValue::new("{\"A\": 1}".to_string());
    assert!(decode_env(&not_text).is_err());
}
