use super::*;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn parse_in(list: &[&str]) -> Result<Parsed, ParseError> {
    parse(
        &args(list),
        Path::new("/tmp/w"),
        Some(Path::new("/home/me")),
    )
}

#[test]
fn flags_and_prompt() {
    let p = parse_in(&[
        "agy",
        "--model",
        "gemini-4-argon",
        "--effort",
        "HIGH",
        "--folder",
        "src",
        "--out",
        "~/x",
        "--write",
        "--timeout",
        "60",
        "--json",
        "--",
        "review",
        "--this",
    ])
    .unwrap();
    let o = p.options;
    assert_eq!(o.tool, Tool::Agy);
    assert_eq!(o.model.as_deref(), Some("gemini-4-argon"));
    assert_eq!(o.effort.as_deref(), Some("high"));
    assert_eq!(o.folder, "/tmp/w/src");
    assert_eq!(o.out.as_deref(), Some("/home/me/x"));
    assert!(o.write && p.json);
    assert_eq!(o.timeout, Duration::from_secs(60));
    assert_eq!(
        o.prompt, "review --this",
        "everything after -- is the prompt, flags included"
    );
}

#[test]
fn defaults_are_read_only_in_the_current_folder_for_half_an_hour() {
    let p = parse_in(&["codex", "hello", "world"]).unwrap();
    assert_eq!(p.options.folder, "/tmp/w");
    assert!(!p.options.write && !p.json);
    assert_eq!(p.options.timeout, Duration::from_secs(1800));
    assert_eq!(p.options.prompt, "hello world");
    assert_eq!(p.options.session, None);
}

#[test]
fn errors() {
    assert!(parse_in(&["claude", "--", "x"]).is_err());
    assert!(parse_in(&[]).is_err());
    assert!(parse_in(&["codex"]).is_err(), "no prompt");
    assert!(parse_in(&["codex", "--", "  "]).is_err(), "blank prompt");
    assert!(
        parse_in(&["codex", "--model", "--", "x"]).is_err(),
        "flag without value"
    );
    assert!(parse_in(&["codex", "--model"]).is_err());
    assert!(parse_in(&["codex", "--nope", "x"]).is_err());
    for bad in ["0", "-3", "abc", "inf", "NaN"] {
        let e = parse_in(&["codex", "--timeout", bad, "x"]);
        assert_eq!(
            e.unwrap_err().message,
            "--timeout takes seconds.",
            "{bad:?}"
        );
    }
    assert_eq!(
        parse_in(&["Antigravity", "make", "it"])
            .unwrap()
            .options
            .tool,
        Tool::Agy
    );
    let none = parse_in(&["nope"]).unwrap_err().message;
    assert!(none.starts_with("Name the tool first: codex or agy.\nUsage:"));
}

#[test]
fn a_huge_timeout_is_capped_and_a_fraction_is_kept() {
    let o = parse_in(&["codex", "--timeout", "1e30", "x"])
        .unwrap()
        .options;
    assert_eq!(o.timeout, Duration::from_secs(315_360_000));
    let o = parse_in(&["codex", "--timeout", "0.25", "x"])
        .unwrap()
        .options;
    assert_eq!(o.timeout, Duration::from_millis(250));
}

#[test]
fn the_prompt_can_come_from_a_file() {
    let dir = std::env::temp_dir().join(format!("nl-delegate-opts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("p.txt"), "from file\n").unwrap();
    let p = parse(&args(&["agy", "--prompt-file", "p.txt"]), &dir, None).unwrap();
    assert_eq!(p.options.prompt, "from file");
    let missing = parse(&args(&["agy", "--prompt-file", "nope.txt"]), &dir, None);
    assert_eq!(missing.unwrap_err().message, "Couldn't read nope.txt.");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn paths_are_tidied_without_touching_the_disk() {
    let cwd = Path::new("/a/b");
    let home = Some(Path::new("/h"));
    assert_eq!(absolute("", cwd, home), "/a/b");
    assert_eq!(absolute("../c/./d", cwd, home), "/a/c/d");
    assert_eq!(absolute("~", cwd, home), "/h");
    assert_eq!(absolute("~/x/../y", cwd, home), "/h/y");
    assert_eq!(absolute("/etc//x/", cwd, home), "/etc/x");
    assert_eq!(
        absolute("~/x", cwd, None),
        "/a/b/~/x",
        "no home known: ~ is just a name"
    );
}

#[test]
fn nesting_stops_at_two() {
    assert_eq!(enter_depth(None), Ok(1));
    assert_eq!(enter_depth(Some("")), Ok(1));
    assert_eq!(enter_depth(Some("junk")), Ok(1));
    assert_eq!(enter_depth(Some("1")), Ok(2));
    let refused = enter_depth(Some("2")).unwrap_err();
    assert_eq!(
        refused,
        "This job was itself delegated 2 times; do it yourself instead of delegating again."
    );
    assert!(enter_depth(Some("9")).is_err());
}

#[test]
fn tool_names() {
    assert_eq!(Tool::Codex.raw(), "codex");
    assert_eq!(Tool::Agy.title(), "Antigravity CLI");
    assert_eq!(Tool::from_name("CODEX"), Some(Tool::Codex));
}
