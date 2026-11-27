use super::*;
use crate::accounts::fake::MemoryKeychain;
use std::sync::Arc;

fn keys(chain: &Arc<MemoryKeychain>) -> Keys {
    Keys {
        runner: chain.clone(),
        user: "me".into(),
    }
}

#[test]
fn a_value_goes_as_hex_on_stdin_never_on_argv() {
    let chain = Arc::new(MemoryKeychain::new());
    keys(&chain)
        .write("Notchling Accounts", "a:o", "{\"t\":\"test-secret\"}")
        .unwrap();
    let calls = chain.calls();
    assert!(calls.contains(&vec!["-i".to_string()]));
    for call in &calls {
        let joined = call.join(" ");
        assert!(!joined.contains("test-secret"), "{joined}");
        assert!(!joined.contains(&hex(b"test-secret")), "{joined}");
    }
    assert_eq!(
        chain.get("Notchling Accounts", "a:o").as_deref(),
        Some("{\"t\":\"test-secret\"}")
    );
}

#[test]
fn the_line_is_the_swift_apps() {
    let line = add_line("svc", "acct", "ab").unwrap();
    assert_eq!(
        line.expose(),
        "add-generic-password -U -a \"acct\" -s \"svc\" -X \"6162\"\n"
    );
    assert_eq!(format!("{line:?}"), "[secret]");
}

#[test]
fn a_value_too_long_for_the_line_is_refused_not_put_on_argv() {
    let chain = Arc::new(MemoryKeychain::new());
    let long = "x".repeat(MAX_LINE);
    assert_eq!(
        keys(&chain).write("svc", "acct", &long),
        Err(KeychainError::TooLong)
    );
    assert!(chain.calls().is_empty(), "nothing ran");
}

#[test]
fn names_that_could_break_the_line_are_refused() {
    let chain = Arc::new(MemoryKeychain::new());
    for bad in ["a\"b", "a\\b", "a\nb", ""] {
        assert_eq!(
            keys(&chain).write("svc", bad, "v"),
            Err(KeychainError::BadName)
        );
        assert_eq!(keys(&chain).read(bad, "a"), Err(KeychainError::BadName));
    }
    assert!(chain.calls().is_empty());
}

#[test]
fn reads_plain_text_or_hex_and_a_missing_item_is_none() {
    let chain = Arc::new(MemoryKeychain::new());
    assert!(keys(&chain).read("svc", "me").unwrap().is_none());
    chain.put(
        "svc",
        "me",
        "{\n  \"claudeAiOauth\": {\"accessToken\": \"test-a\"}\n}",
    );
    let obj = keys(&chain).read_json("svc", "me").unwrap().unwrap();
    assert!(obj.object("claudeAiOauth").is_some());
    let hexed = Arc::new(MemoryKeychain::printing_hex());
    hexed.put("svc", "me", "{\n\"k\": 1}");
    let obj = keys(&hexed).read_json("svc", "me").unwrap().unwrap();
    assert_eq!(obj.get("k"), Some(&serde_json::json!(1)));
    assert_eq!(
        keys(&hexed)
            .read_text("svc", "me")
            .unwrap()
            .unwrap()
            .expose(),
        "{\n\"k\": 1}"
    );
}

#[test]
fn delete_says_whether_there_was_an_item() {
    let chain = Arc::new(MemoryKeychain::new());
    chain.put("svc", "me", "v");
    assert_eq!(keys(&chain).delete("svc", "me"), Ok(true));
    assert_eq!(keys(&chain).delete("svc", "me"), Ok(false));
}

#[test]
fn a_write_that_didnt_stick_is_an_error() {
    struct Forgetful;
    impl SecurityRunner for Forgetful {
        fn run(&self, _a: &[String], _s: Option<&[u8]>) -> Result<SecurityOutput, KeychainError> {
            Ok(SecurityOutput {
                status: 0,
                stdout: b"something else\n".to_vec(),
            })
        }
    }
    let keys = Keys {
        runner: Arc::new(Forgetful),
        user: "me".into(),
    };
    assert_eq!(keys.write("svc", "me", "v"), Err(KeychainError::NotStored));
}

#[test]
fn errors_and_outputs_never_print_a_value() {
    let out = SecurityOutput {
        status: 0,
        stdout: b"test-secret".to_vec(),
    };
    assert!(!format!("{out:?}").contains("test-secret"));
    assert!(NoKeychain("fixture root")
        .run(&[], None)
        .unwrap_err()
        .to_string()
        .contains("fixture root"));
}

fn long_login() -> String {
    let note = r#"a \"q\" b\\c $HOME #x; é"#;
    format!(
        r#"{{"claudeAiOauth":{{"accessToken":"test-secret"}},"mcpOAuth":{{"note":"{note}","pad":"{}"}}}}"#,
        "p".repeat(2_800)
    )
}

#[test]
fn a_login_too_long_as_hex_goes_as_quoted_text_on_stdin() {
    let value = long_login();
    assert!(serde_json::from_str::<serde_json::Value>(&value).is_ok());
    assert!(
        add_line("Claude Code-credentials", "me", &value).is_err(),
        "hex is too long"
    );
    let chain = Arc::new(MemoryKeychain::new());
    keys(&chain)
        .write("Claude Code-credentials", "me", &value)
        .unwrap();
    assert_eq!(
        chain.get("Claude Code-credentials", "me").as_deref(),
        Some(value.as_str())
    );
    for call in chain.calls() {
        let joined = call.join(" ");
        assert!(!joined.contains("test-secret"), "never on argv: {joined}");
    }
}

#[test]
fn the_text_line_escapes_quotes_and_backslashes_only() {
    let line = text_line("svc", "acct", r#"a"b\c $d é"#).unwrap();
    assert_eq!(
        line.expose(),
        "add-generic-password -U -a \"acct\" -s \"svc\" -w \"a\\\"b\\\\c $d é\"\n"
    );
    assert_eq!(format!("{line:?}"), "[secret]");
}

#[test]
fn a_value_with_a_control_character_never_goes_as_text() {
    assert_eq!(
        text_line("svc", "acct", "a\nb").map(|_| ()),
        Err(KeychainError::TooLong)
    );
    let chain = Arc::new(MemoryKeychain::new());
    let long = format!("{}\n", "x".repeat(2_500));
    assert_eq!(
        keys(&chain).write("svc", "acct", &long),
        Err(KeychainError::TooLong)
    );
    assert!(chain.calls().is_empty(), "nothing ran");
}
