use super::*;


fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn no_word_or_check_reads_and_update_writes() {
    let read = parse(&words(&[])).unwrap();
    assert!(!read.update && !read.json && read.skills);
    assert_eq!(request(&read).0, ops::MODELS_CHECK);
    assert!(parse(&words(&["check", "--json"])).unwrap().json);

    let write = parse(&words(&["update", "--no-skills", "--json"])).unwrap();
    assert!(write.update && write.json && !write.skills);
    let (op, args) = request(&write);
    assert_eq!(op, ops::MODELS_UPDATE);
    assert_eq!(args, json!({"skills": false}));
}

#[test]
fn a_wrong_command_gets_the_usage() {
    for bad in [
        vec!["refresh"],
        vec!["update", "now"],
        vec!["--nope"],
        vec!["--socket"],
    ] {
        let e = parse(&words(&bad)).unwrap_err();
        assert!(e.contains("Usage: notchling models"), "{bad:?}: {e}");
    }
    assert_eq!(
        parse(&words(&["--socket", "/tmp/s"])).unwrap().socket,
        Some(PathBuf::from("/tmp/s"))
    );
}

#[test]
fn the_text_is_the_report_text_and_json_is_the_whole_reply() {
    let reply = json!({"text": "Codex (x): 2 models", "applied": true});
    assert_eq!(shown(&reply, false), "Codex (x): 2 models");
    assert!(shown(&reply, true).contains("\"applied\": true"));
    assert!(shown(&json!({"applied": true}), false).contains("applied"));
}

#[test]
fn force_is_sent_only_when_asked_for() {
    let plain = parse(&words(&["update"])).unwrap();
    assert_eq!(request(&plain).1, json!({"skills": true}));
    let forced = parse(&words(&["update", "--force"])).unwrap();
    assert!(forced.force);
    assert_eq!(request(&forced).1, json!({"skills": true, "force": true}));
}
