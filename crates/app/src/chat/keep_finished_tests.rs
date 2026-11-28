use notchling_engine::settings::{spec, Fallback, KeepFinished};
use serde_json::json;

use super::*;

#[test]
fn sheet_choices_match_the_engine_key_and_english_labels() {
    let key = spec(KEY).expect("the key is listed");
    assert_eq!(key.default, Fallback::Str("1d"));
    let values: Vec<&str> = CHOICES.iter().map(|(v, _)| *v).collect();
    assert_eq!(key.allowed, values.as_slice());
    let labels: Vec<&str> = CHOICES.iter().map(|(_, t)| t.english()).collect();
    assert_eq!(labels, ["1 Day", "7 Days", "30 Days"]);
}

#[test]
fn only_a_choice_for_this_key_is_a_change() {
    assert_eq!(chosen(KEY, &json!("1d")), Some(KeepFinished::Days1));
    assert_eq!(chosen(KEY, &json!("7d")), Some(KeepFinished::Days7));
    assert_eq!(chosen(KEY, &json!("30d")), Some(KeepFinished::Days30));
    assert_eq!(chosen(KEY, &json!("14d")), None);
    assert_eq!(chosen(KEY, &json!(7)), None);
    assert_eq!(chosen("chatHistory", &json!("7d")), None);
}
