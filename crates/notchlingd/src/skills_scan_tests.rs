use super::*;

#[test]
fn examples_are_the_binary_lines_of_fenced_sh_blocks() {
    let text = "intro '/b' ignored\n```sh\n'/b' help --json\n# note\n$ '/b' orgs\n'/other' x\n```\n```text\n'/b' no\n```\n";
    assert_eq!(
        sh_examples("/b", text),
        vec!["'/b' help --json", "'/b' orgs"]
    );
}

#[test]
fn commands_are_found_after_the_quoted_binary_and_bare_name() {
    assert_eq!(
        commands_named("/b", "run `'/b' help --json` now"),
        vec!["help --json"]
    );
    assert!(commands_named("/b", "a notchling-sessions skill and Notchling's").is_empty());
}
