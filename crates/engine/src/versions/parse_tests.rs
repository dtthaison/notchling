use super::{cli_output, Tool};

#[test]
fn reads_each_tool() {
    let read = |tool, text| cli_output(tool, text).map(|v| v.to_string());
    assert_eq!(
        read(Tool::Claude, "2.1.322 (Claude Code)\n").as_deref(),
        Some("2.1.322")
    );
    assert_eq!(
        read(Tool::Codex, "codex-cli 0.166.0").as_deref(),
        Some("0.166.0")
    );
    assert_eq!(read(Tool::Agy, "agy 1.4.2").as_deref(), Some("1.4.2"));
    assert_eq!(
        read(Tool::Notchling, "notchling 0.9.0\r\n").as_deref(),
        Some("0.9.0")
    );
}

#[test]
fn hostile_text_is_never_a_version() {
    let long = "9".repeat(10_240);
    for text in [
        "2.1.3; rm -rf ~ (Claude Code)",
        long.as_str(),
        "2.1.3\u{2028}9.9.9 (Claude Code)",
        "sk-ant-oat01-XXXX",
        "codex-cli 0.1.0;id",
    ] {
        for tool in Tool::ALL {
            assert!(cli_output(tool, text).is_none(), "{tool:?} {text:?}");
        }
    }
}

#[test]
fn reads_only_the_first_line() {
    assert!(cli_output(Tool::Agy, "banner\nagy 1.4.2").is_none());
    assert!(cli_output(Tool::Agy, "agy 1.4.2\nagy 9.9.9").is_some());
}
