use super::*;

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

fn commands(list: &[&[&str]]) -> Option<Vec<Vec<String>>> {
    Some(list.iter().map(|words| strings(words)).collect())
}

#[test]
fn splits_at_operators_and_newlines() {
    assert_eq!(
        simple_commands("cd x && notchling ask -- 'a b'; echo y"),
        commands(&[
            &["cd", "x"],
            &["notchling", "ask", "--", "a b"],
            &["echo", "y"]
        ])
    );
    assert_eq!(
        simple_commands("a || b | c\nd\n\n"),
        commands(&[&["a"], &["b"], &["c"], &["d"]])
    );
    assert_eq!(simple_commands(""), commands(&[]));
    assert_eq!(simple_commands(" ; && "), commands(&[]));
}

#[test]
fn operators_inside_quotes_stay_in_the_word() {
    assert_eq!(
        simple_commands("notchling ask 'a && b; c | d' \"e\nf\""),
        commands(&[&["notchling", "ask", "a && b; c | d", "e\nf"]])
    );
}

#[test]
fn quotes_and_escapes_resolve() {
    assert_eq!(
        simple_commands(
            "\"/Applications/Notchling.app/Contents/MacOS/notchling\" ask --choice 'Keep it' -- \"Keep the \\\"old\\\" API?\""
        ),
        commands(&[&[
            "/Applications/Notchling.app/Contents/MacOS/notchling",
            "ask",
            "--choice",
            "Keep it",
            "--",
            "Keep the \"old\" API?"
        ]])
    );
    assert_eq!(
        simple_commands("echo a\\ b 'it'\\''s' \"a\\\\b\" \"a\\nb\" ''"),
        commands(&[&["echo", "a b", "it's", "a\\b", "a\\nb", ""]])
    );
    assert_eq!(
        simple_commands("echo \"\" a\"b\"'c'"),
        commands(&[&["echo", "", "abc"]])
    );
    assert_eq!(
        simple_commands("echo a \\\nb"),
        commands(&[&["echo", "a", "b"]])
    );
}

#[test]
fn substitutions_never_match() {
    assert_eq!(simple_commands("notchling ask $(x)"), None);
    assert_eq!(simple_commands("notchling ask `x`"), None);
    assert_eq!(simple_commands("notchling ask <<EOF"), None);
    assert_eq!(simple_commands("notchling ask \"$HOME\""), None);
    assert_eq!(simple_commands("notchling ask $HOME"), None);
    assert_eq!(simple_commands("notchling ask \"$(x)\""), None);
    assert_eq!(
        simple_commands("notchling ask '$5' '$(x)' '`y`'"),
        commands(&[&["notchling", "ask", "$5", "$(x)", "`y`"]])
    );
    assert_eq!(
        simple_commands("notchling ask \\$5"),
        commands(&[&["notchling", "ask", "$5"]])
    );
    assert_eq!(
        simple_commands("notchling ask \"\\$5\""),
        commands(&[&["notchling", "ask", "$5"]])
    );
}

#[test]
fn unterminated_quotes_never_match() {
    assert_eq!(simple_commands("notchling ask 'a"), None);
    assert_eq!(simple_commands("notchling ask \"a"), None);
    assert_eq!(simple_commands("notchling ask a\\"), None);
}

#[test]
fn same_command_needs_equal_words_after_the_program() {
    let argv = strings(&["notchling", "ask", "--", "Keep it?"]);
    let full = strings(&[
        "/Applications/Notchling.app/Contents/MacOS/notchling",
        "ask",
        "--",
        "Keep it?",
    ]);
    assert!(same_command(&full, &argv));
    assert!(same_command(&argv, &full));
    assert!(same_command(&argv, &argv));
    let other = strings(&["notchling", "ask", "--", "Keep it!"]);
    assert!(!same_command(&other, &argv));
    let shorter = strings(&["notchling", "ask", "--"]);
    assert!(!same_command(&shorter, &argv));
    let program_ask = strings(&["ask", "ask", "--", "Keep it?"]);
    assert!(!same_command(&program_ask, &argv));
    assert!(!same_command(&argv, &program_ask));
    assert!(!same_command(&[], &argv));
    assert!(!same_command(&argv, &[]));
    assert!(same_command(
        &strings(&["notchling"]),
        &strings(&["/usr/local/bin/notchling"])
    ));
}
