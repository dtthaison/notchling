use super::*;
use serde_json::Value;

fn run(what: &str, args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = not_built_to(what, &args, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn a_command_not_built_yet_gives_one_json_error_on_stdout_and_nothing_on_stderr() {
    for args in [
        &["--json"][..],
        &["list", "--json"],
        &["add", "x", "--json"],
    ] {
        let (code, out, err) = run("machines", args);
        assert_eq!(code, 4, "{args:?}");
        assert!(err.is_empty(), "{args:?}: {err}");
        assert_eq!(out.lines().count(), 1, "{args:?}: {out}");
        let error = serde_json::from_str::<Value>(&out).unwrap()["error"].clone();
        assert_eq!(error["exit"], 4);
        assert_eq!(error["code"], "forbidden");
        assert_eq!(error["reason"], "not_yet");
        assert_eq!(error["never"], false);
        assert!(error["message"].as_str().unwrap().contains("machines"));
    }
}

#[test]
fn a_command_not_built_yet_says_so_on_stderr_with_its_exit_name_without_json() {
    let (code, out, err) = run("cost profile", &["set"]);
    assert_eq!(code, 4);
    assert!(out.is_empty(), "{out}");
    assert!(err.contains("cost profile"), "{err}");
    assert!(err.trim_end().ends_with("(refused)"), "{err}");
}

#[test]
fn a_json_flag_after_a_value_flag_is_not_taken_for_json() {
    let (_, out, err) = run("network", &["--reason", "--json"]);
    assert!(out.is_empty(), "{out}");
    assert!(!err.is_empty());
}
