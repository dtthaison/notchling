use super::*;
use serde_json::json;
use std::path::PathBuf;

const FIXTURE: &str = include_str!("../../tests/fixtures/ask/agy-permission.json");
const BINARY: &str = "/Applications/Notchling.app/Contents/MacOS/notchling";

fn binary() -> PathBuf {
    PathBuf::from(BINARY)
}

fn request(command: &str) -> Value {
    json!({"params": {"toolCall": {"kind": "execute", "rawInput": {"CommandLine": command}}}})
}

fn allowed(command: &str) -> bool {
    allows(&request(command), &binary())
}

fn ask(rest: &str) -> String {
    format!("{BINARY} ask --wait{rest}")
}

#[test]
fn allows_exact_ask_wait() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    assert!(allows(&fixture, &binary()));
    assert!(allowed(&ask(" -- 'Keep it?'")));
    assert!(allowed(&ask("")));
    assert!(allowed(&format!(
        "'{BINARY}' ask --wait --choice 'A b' -- \"Why, now?\""
    )));
    assert!(allowed(&ask(
        " --timeout 600 --choice a/b-c_d.e,f:g@h+i%j -- x"
    )));
    let args_only = json!({"params": {"toolCall": {"args": {"CommandLine": ask(" -- x")}}}});
    assert!(allows(&args_only, &binary()));
}

#[test]
fn declines_ask_without_wait() {
    assert!(!allowed(&format!("{BINARY} ask -- 'x'")));
    assert!(!allowed(&format!("{BINARY} ask --json --wait -- x")));
    assert!(!allowed(&format!("{BINARY} asks --wait")));
    assert!(!allowed(&format!("{BINARY} answer --wait")));
    assert!(!allowed(BINARY));
    assert!(!allowed(""));
}

#[test]
fn declines_ask_after_an_operator() {
    assert!(!allowed(&ask(" -- x; rm y")));
    assert!(!allowed(&ask(" -- x && rm y")));
    assert!(!allowed(&format!("rm y; {}", ask(" -- x"))));
    assert!(!allowed(&ask(" -- x | cat")));
    assert!(!allowed(&ask(" -- x &")));
    assert!(!allowed(&ask(" -- x\nrm y")));
    assert!(!allowed(&ask(" -- $(rm y)")));
    assert!(!allowed(&ask(" -- 'x\ny'")));
}

#[test]
fn declines_a_notchling_from_another_folder() {
    assert!(!allowed("/tmp/notchling ask --wait -- x"));
    assert!(!allowed("./notchling ask --wait -- x"));
    assert!(!allowed(
        "/Applications/Other.app/Contents/MacOS/notchling ask --wait -- x"
    ));
    assert!(!allowed(&format!("{BINARY}/../notchling ask --wait -- x")));
    assert!(!allowed(&format!("{BINARY}/ ask --wait -- x")));
}

#[test]
fn declines_the_bare_word_notchling() {
    assert!(!allowed("notchling ask --wait -- x"));
    assert!(!allowed("notchling ask --wait"));
}

#[test]
fn declines_an_empty_or_relative_binary() {
    let command = request("'' ask --wait -- x");
    assert!(!allows(&command, Path::new("")));
    assert!(!allows(&request(" ask --wait -- x"), Path::new("")));
    assert!(!allows(
        &request("notchling ask --wait -- x"),
        Path::new("notchling")
    ));
    assert!(!allows(
        &request("./notchling ask --wait -- x"),
        Path::new("./notchling")
    ));
    assert!(!allows(
        &request("target/notchling ask --wait -- x"),
        Path::new("target/notchling")
    ));
}

#[test]
fn declines_a_redirection() {
    for tail in [
        " -- x > /tmp/a",
        " -- x >> /tmp/a",
        " -- x < /tmp/a",
        " -- x 2>&1",
        " -- x &> /tmp/a",
        " -- x 2> /tmp/a",
        " -- x>/tmp/a",
        " -- x & rm y",
        " -- x <(rm y)",
    ] {
        assert!(!allowed(&ask(tail)), "{tail}");
    }
    assert!(allowed(&ask(" -- 'a > b & c'")));
    assert!(allowed(&ask(" -- \"a -> b\"")));
}

#[test]
fn declines_a_zsh_expansion() {
    for tail in [
        " -- =(touch /tmp/x)",
        " -- *(e:'id>/tmp/x':)",
        " -- *",
        " -- ?",
        " -- [a-z]",
        " -- {a,b}",
        " -- ~",
        " -- ~root",
        " -- =ls",
        " -- x=y",
        " -- #",
        " -- (rm y)",
        " -- a\tb",
        " -- ^x",
        " -- x!",
    ] {
        assert!(!allowed(&ask(tail)), "{tail}");
    }
}

#[test]
fn quotes_keep_what_they_may_and_double_quotes_keep_less() {
    assert!(allowed(&ask(" -- 'a $b `c` !d *e ~f =g {h,i} (j) #k'")));
    assert!(allowed(&ask(" -- \"a *b ~c =d {e,f} (g) #h\"")));
    for tail in [
        " -- \"a $b\"",
        " -- \"a `b`\"",
        " -- \"a !b\"",
        " -- \"a $(b)\"",
        " -- \"a \\$b\"",
        " -- \"a\nb\"",
        " -- 'a",
        " -- \"a",
    ] {
        assert!(!allowed(&ask(tail)), "{tail}");
    }
    assert!(allowed(&ask(" -- a\\ b")));
    assert!(!allowed(&ask(" -- a\\\nb")));
}

#[test]
fn reads_raw_input_not_title() {
    let title_only = json!({"params": {"toolCall": {"title": ask(" -- x")}}});
    assert!(!allows(&title_only, &binary()));
    let differing = json!({"params": {"toolCall": {
        "title": ask(" -- x"),
        "rawInput": {"CommandLine": "rm -rf /"}}}});
    assert!(!allows(&differing, &binary()));
    let other_way = json!({"params": {"toolCall": {
        "title": "rm -rf /",
        "rawInput": {"CommandLine": ask(" -- x")}}}});
    assert!(allows(&other_way, &binary()));
}
