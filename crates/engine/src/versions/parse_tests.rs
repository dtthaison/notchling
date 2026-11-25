use super::{cli_output, Tool};

fn v(tool: Tool, text: &str) -> Option<String> {
    cli_output(tool, text).map(|v| v.as_str().to_string())
}

#[test]
fn reads_each_tool() {
    assert_eq!(
        v(Tool::Claude, "2.1.322 (Claude Code)\n").as_deref(),
        Some("2.1.322")
    );
    assert_eq!(
        v(Tool::Claude, "2.1.89 (Claude Code)").as_deref(),
        Some("2.1.89")
    );
    assert_eq!(
        v(Tool::Codex, "codex-cli 0.166.0\n").as_deref(),
        Some("0.166.0")
    );
    assert_eq!(v(Tool::Agy, "agy 1.4.2\n").as_deref(), Some("1.4.2"));
    assert_eq!(
        v(Tool::Notchling, "notchling 0.9.0\n").as_deref(),
        Some("0.9.0")
    );
    assert_eq!(
        v(Tool::Notchling, "notchling 1.3.0-beta.2").as_deref(),
        Some("1.3.0-beta.2")
    );
}

#[test]
fn another_tools_form_is_not_read() {
    assert_eq!(v(Tool::Codex, "2.1.322 (Claude Code)"), None);
    assert_eq!(v(Tool::Claude, "codex-cli 0.166.0"), None);
    assert_eq!(v(Tool::Agy, "notchling 0.9.0"), None);
}

#[test]
fn hostile_text_is_never_a_version() {
    let planted = "sk-ant-oat01-XXXX";
    let long = "1".repeat(10_000);
    for tool in Tool::ALL {
        for text in [
            "2.1.3; rm -rf ~ (Claude Code)",
            "codex-cli 2.1.3; rm -rf ~",
            "agy 1.2.3\u{2028}2.2.2",
            "notchling 1.2.3\u{2029}",
            planted,
            &long,
            &format!("codex-cli {long}"),
            &format!("2.1.3 (Claude Code) {planted}"),
            "agy 1.2.3 && id",
            "",
            "\n\n",
        ] {
            let got = cli_output(tool, text);
            assert!(got.is_none(), "{tool:?} {text:.40?} gave {got:?}");
        }
    }
}

#[test]
fn reads_only_the_first_line() {
    assert_eq!(
        v(Tool::Codex, "codex-cli 0.166.0\nsk-ant-oat01-XXXX\n").as_deref(),
        Some("0.166.0")
    );
    assert_eq!(
        v(
            Tool::Codex,
            "warning: update available\ncodex-cli 0.166.0\n"
        ),
        None
    );
}

#[test]
fn trailing_carriage_return_is_ignored() {
    assert_eq!(v(Tool::Agy, "agy 1.4.2\r\n").as_deref(), Some("1.4.2"));
}
