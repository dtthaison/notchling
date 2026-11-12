//! The vault commands' arguments and text (Swift `VaultCLI.parseRequest` and the `--secrets`,
//! `--secret-request` output), with "today" a day number.

use super::*;
use notchling_engine::vault::expiry::day_number;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn request_parsing_keeps_newlines_and_checks_expiry() {
    let (r, socket) = parse_request(&args(&[
        "gh",
        "--kind",
        "token",
        "--note",
        "a\nb",
        "--how",
        "Settings > Tokens\nPick repo",
        "--expires",
        "2026-12-06",
        "--reason",
        "r",
        "--socket",
        "/tmp/x.sock",
    ]))
    .unwrap();
    assert_eq!(r.name, "gh");
    assert_eq!(r.note.as_deref(), Some("a\nb"));
    assert_eq!(r.how_to.as_deref(), Some("Settings > Tokens\nPick repo"));
    assert_eq!(r.expires.as_deref(), Some("2026-12-06"));
    assert_eq!(r.reason.as_deref(), Some("r"));
    assert_eq!(socket, Some(PathBuf::from("/tmp/x.sock")));
    let (bare, _) = parse_request(&args(&["gh"])).unwrap();
    assert_eq!((bare.how_to, bare.expires, bare.note), (None, None, None));
    let e = parse_request(&args(&["gh", "--expires", "2026-02-30"])).unwrap_err();
    assert!(e.contains("--expires") && e.contains("YYYY-MM-DD"), "{e}");
    let e = parse_request(&args(&["gh", "--expires"])).unwrap_err();
    assert!(e.contains("YYYY-MM-DD"), "{e}");
    assert!(parse_request(&args(&["gh", "--kind", "pin"])).is_err());
    assert!(parse_request(&args(&[])).is_err());
    assert!(parse_request(&args(&["--kind", "token"])).is_err());
    assert!(parse_request(&args(&["gh", "--colour", "x"])).is_err());
    // A value that looks like a flag is still the value, as the Swift command read it.
    let (odd, _) = parse_request(&args(&["gh", "--note", "--not-a-flag"])).unwrap();
    assert_eq!(odd.note.as_deref(), Some("--not-a-flag"));
}

#[test]
fn use_parsing() {
    let parsed = parse_use(&args(&["gh, npm", "--", "curl -s \"$GH_URL\""])).unwrap();
    assert_eq!(parsed.names, ["gh", "npm"]);
    assert_eq!(parsed.command, ["curl -s \"$GH_URL\""]);
    let parsed = parse_use(&args(&["gh", "--socket", "/s", "--", "make", "--socket"])).unwrap();
    assert_eq!(parsed.socket, Some(PathBuf::from("/s")));
    assert_eq!(
        parsed.command,
        ["make", "--socket"],
        "after -- is the command's"
    );
    for bad in [
        &["gh"][..],
        &["gh", "--"],
        &["--", "make"],
        &[",", "--", "make"],
        &["gh", "make"],
    ] {
        assert_eq!(parse_use(&args(bad)), Err(usage()), "{bad:?}");
    }
}

#[test]
fn the_shown_command_is_quoted_like_a_shell() {
    assert_eq!(shown(&args(&["echo $X | wc"])), "echo $X | wc");
    assert_eq!(
        shown(&args(&["curl", "-H", "a b", "it's", "https://x.io/p?q=1"])),
        "curl -H 'a b' 'it'\\''s' 'https://x.io/p?q=1'"
    );
    assert_eq!(shown(&args(&["printf", ""])), "printf ''");
}

#[test]
fn find_says_what_is_saved_or_what_fits() {
    let today = day_number("2026-11-06").unwrap();
    let (code, text) = find_report(&json!({"entries": [], "saved": []}), "x", today);
    assert_eq!(code, 1);
    assert!(text.starts_with("No secrets saved yet. Ask the user for one with secret-request"));
    let (code, text) = find_report(
        &json!({"entries": [], "saved": ["gh (api.github.com)", "npm"]}),
        "gitlab.com",
        today,
    );
    assert_eq!(code, 1);
    assert_eq!(
        text,
        "Nothing saved fits \"gitlab.com\". Saved: gh (api.github.com), npm.\nIf none of them is the one, ask the user with secret-request."
    );
    let gh = json!({"name": "gh", "kind": "token", "secretVar": "GH_TOKEN", "url": "https://api.github.com",
                    "note": "my repos", "expires": "2026-11-09", "ask": true, "userVar": "", "updated": ""});
    let (code, text) = find_report(&json!({"entries": [gh], "saved": ["gh"]}), "", today);
    assert_eq!(code, 0);
    assert_eq!(
        text,
        "gh  (token)  $GH_TOKEN $GH_URL\n    https://api.github.com\n    my repos\n    expires 2026-11-09 (in 3 days, renew soon)\n    The user approves each use."
    );
}

#[test]
fn a_saved_request_says_how_to_use_it() {
    let entry = VaultEntry {
        user_var: "REG_USERNAME".into(),
        secret_var: "REG_PASSWORD".into(),
        ..VaultEntry::new("reg", Kind::Login)
    };
    assert_eq!(
        saved_report(&entry, 0),
        "Saved.\nreg  (login)  $REG_USERNAME $REG_PASSWORD\nUse it: notchling secret reg -- '<command using $REG_USERNAME, $REG_PASSWORD>'"
    );
}

#[test]
fn find_words_socket_and_json() {
    assert_eq!(
        parse_find(&args(&["api", "github", "--json", "--socket", "/s"])),
        ("api github".into(), Some(PathBuf::from("/s")), true)
    );
    assert_eq!(parse_find(&args(&[])), (String::new(), None, false));
}

#[test]
fn find_json_lists_names_and_never_a_value() {
    let gh = json!({"name": "gh", "kind": "token", "secretVar": "GH_TOKEN", "url": "https://api.github.com",
                    "note": "my repos", "expires": "2026-11-09", "ask": true, "userVar": "", "updated": "",
                    // Fields no list entry has: whatever sits in a reply, none of it comes out.
                    "secret": "ghp_TOPSECRET", "value": "ghp_TOPSECRET", "password": "ghp_TOPSECRET"});
    let (code, value) = find_json(&json!({"entries": [gh], "saved": ["gh", "npm"]}), "github");
    assert_eq!(code, 0);
    assert_eq!(value["query"], "github");
    assert_eq!(value["saved"], json!(["gh", "npm"]));
    assert_eq!(value["entries"][0]["name"], "gh");
    assert_eq!(value["entries"][0]["secretVar"], "GH_TOKEN");
    let text = value.to_string();
    assert!(!text.contains("TOPSECRET"), "{text}");
    for key in ["secret", "value", "password"] {
        assert!(value["entries"][0].get(key).is_none(), "{key}");
    }

    let (code, value) = find_json(&json!({"entries": [], "saved": ["gh"]}), "gitlab");
    assert_eq!((code, value["entries"].clone()), (1, json!([])));
    let (code, value) = find_json(&json!({"entries": [], "saved": []}), "");
    assert_eq!((code, value["saved"].clone()), (1, json!([])));
}

#[test]
fn usage_names_all_three() {
    let text = usage();
    assert!(text.contains("notchling secret <name>"));
    assert!(text.contains("notchling secrets [url | host | words]"));
    assert!(text.contains("notchling secret-request <name>"));
}
