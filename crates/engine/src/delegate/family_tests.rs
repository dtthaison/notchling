use super::*;

fn live(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_family_is_the_codename_and_the_version_is_numeric() {
    let f = parse("gemini-4.1-hydrogen").unwrap();
    assert_eq!(
        (f.prefix.as_str(), f.codename.as_str()),
        ("gemini", "hydrogen")
    );
    assert_eq!(f.version, [4, 1]);
    assert!(parse("gemini-4-hydrogen").unwrap().same(&f));
    assert!(parse("gpt-6.2-sol")
        .unwrap()
        .same(&parse("gpt-6.1-sol").unwrap()));
    assert!(!parse("gpt-6-sol")
        .unwrap()
        .same(&parse("gpt-6-astra").unwrap()));
    assert!(!parse("gemini-4-hydrogen-xhigh").unwrap().same(&f));
    assert_eq!(parse("gemini-4-hydrogen-xhigh").unwrap().variant, "xhigh");
    assert!(!parse("gpt-5.6-sol")
        .unwrap()
        .same(&parse("gemini-5.6-sol").unwrap()));
    assert_eq!(parse("gpt-reserve"), None);
    assert_eq!(parse("gemini-pro-orchestrator-g1-pro"), None);
    assert_eq!(parse("claude-opus-5-5").map(|f| f.codename), None);
}

#[test]
fn versions_compare_numerically() {
    let l = live(&["gpt-6.9-sol", "gpt-6.10-sol", "gpt-6-astra"]);
    match newest("gpt-6-sol", &l, |_| false) {
        Newest::One(id, newer) => assert_eq!((id, newer), ("gpt-6.10-sol", true)),
        _ => panic!("one newest"),
    }
}

#[test]
fn nothing_live_and_skips() {
    let l = live(&["gemini-4-argon", "gemini-3.8-flash"]);
    assert!(matches!(
        newest("gemini-4-hydrogen", &l, |_| false),
        Newest::Nothing
    ));
    let skipped = live(&["gpt-7-sol"]);
    assert!(matches!(
        newest("gpt-6-sol", &skipped, |_| true),
        Newest::Nothing
    ));
    let older = live(&["gpt-5-sol"]);
    match newest("gpt-6-sol", &older, |_| false) {
        Newest::One(id, newer) => assert_eq!((id, newer), ("gpt-5-sol", false)),
        _ => panic!("one"),
    }
}
