use super::{cli_output, Tool};

fn v(tool: Tool, text: &str) -> Option<String> {
    cli_output(tool, text).map(|v| v.as_str().to_string())
}

#[test]
fn agy_prints_a_bare_version() {
    assert_eq!(v(Tool::Agy, "1.5.9\n").as_deref(), Some("1.5.9"));
    assert_eq!(v(Tool::Agy, "1.5.9").as_deref(), Some("1.5.9"));
    assert_eq!(v(Tool::Agy, "1.5.9\r\n").as_deref(), Some("1.5.9"));
    assert_eq!(
        v(Tool::Agy, "1.6.0-beta.2\n").as_deref(),
        Some("1.6.0-beta.2")
    );
}

#[test]
fn agy_still_reads_the_prefixed_form() {
    assert_eq!(v(Tool::Agy, "agy 1.5.9\n").as_deref(), Some("1.5.9"));
    assert_eq!(v(Tool::Agy, "agy 1.4.2\r\n").as_deref(), Some("1.4.2"));
}

#[test]
fn codex_and_notchling_still_need_their_own_prefix() {
    assert_eq!(v(Tool::Codex, "0.166.0\n"), None);
    assert_eq!(v(Tool::Notchling, "0.9.0\n"), None);
}

#[test]
fn a_bare_agy_line_with_anything_else_is_not_a_version() {
    let planted = "sk-ant-oat01-XXXX";
    let long = "1".repeat(10_000);
    for text in [
        "1.5.9\u{2028}",
        "1.5.9\u{2029}2.0.0",
        "1.5.9\u{7}",
        "1.5.9\t",
        " 1.5.9",
        "1.5.9 && id",
        "1.5.9; rm -rf ~",
        "v1.5.9",
        "1.5",
        "1.5.9.1",
        "agy",
        "agy ",
        planted,
        &format!("agy {planted}"),
        &long,
        &format!("agy {long}"),
        "warning: update available\n1.5.9\n",
        "",
        "\n",
    ] {
        let got = cli_output(Tool::Agy, text);
        assert!(got.is_none(), "{text:.40?} gave {got:?}");
    }
}

#[test]
fn a_bare_agy_version_only_reads_the_first_line() {
    assert_eq!(
        v(Tool::Agy, "1.5.9\nsk-ant-oat01-XXXX\n").as_deref(),
        Some("1.5.9")
    );
}
