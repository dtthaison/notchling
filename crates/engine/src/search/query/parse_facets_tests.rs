use super::filter::Filter;
use super::parse::{filter, parse};
use serde_json::Value;

const NOW: f64 = 1_794_000_000.0;

#[test]
fn facet_goldens() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("parse_facets_golden.json")).unwrap();
    assert!(cases.len() >= 10);
    for case in cases {
        let parsed = parse(case["input"].as_str().unwrap());
        let chips: Vec<String> = parsed.chips.iter().map(|c| c.token()).collect();
        let want_chips: Vec<String> = case["chips"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap().to_string())
            .collect();
        assert_eq!(chips, want_chips, "{case}");
        let want_words: Vec<String> = case["words"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap().to_string())
            .collect();
        assert_eq!(parsed.words, want_words, "{case}");
        assert_eq!(parsed.rest, case["rest"].as_str().unwrap(), "{case}");
    }
}

#[test]
fn filter_from_chips() {
    let parsed = parse("tool:codex tool:claude in:app deploy");
    let f = filter(&parsed, NOW, 0);
    assert_eq!(f.tool, Some(vec!["codex".into(), "claude".into()]));
    assert_eq!(f.project, Some(vec!["app".into()]));
    assert_eq!(f.account, None);
    assert_eq!(f.after, None);
    assert_eq!(parse("deploy").chips.len(), 0);
    assert_eq!(filter(&parse("deploy"), NOW, 0), Filter::default());
}

#[test]
fn a_when_chip_sets_the_bounds() {
    let f = filter(&parse("when:7d"), NOW, 0);
    assert_eq!(f.after, Some(NOW - 7.0 * 86_400.0));
    assert_eq!(f.before, None);
    let past = filter(&parse("when:2026-10"), NOW, 0);
    assert_eq!(past.after, Some(1_790_812_800.0));
    assert_eq!(past.before, Some(1_793_491_200.0));
}

#[test]
fn the_same_value_twice_is_once() {
    let f = filter(&parse("tool:codex tool:codex"), NOW, 0);
    assert_eq!(f.tool, Some(vec!["codex".into()]));
}
