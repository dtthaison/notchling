//! Swift `SecretVaultTests`, ported: names and variables, `find`, the environment, notes, how-to,
//! expiry, request prefill and its changes, and the skill text (reminders and Renew are in
//! `tests_reminders.rs`). "Today" is a day number, so nothing depends on the clock or the time
//! zone.

use super::entry::*;
use super::expiry::{self, Status};
use super::list::{self, describe, environment, find, problem};
use super::skill::{self, SkillTool};
use super::url::host_of;

fn day(text: &str) -> i64 {
    expiry::day_number(text).unwrap()
}

fn token(name: &str, var: &str) -> VaultEntry {
    VaultEntry {
        secret_var: var.into(),
        ..VaultEntry::new(name, Kind::Token)
    }
}

fn request(name: &str) -> VaultRequest {
    VaultRequest {
        name: name.into(),
        ..VaultRequest::default()
    }
}

#[test]
fn default_variables() {
    assert_eq!(prefix("my-api.v2"), "MY_API_V2");
    assert_eq!(prefix("2fa"), "_2FA");
    assert_eq!(default_vars("github", Kind::Token).1, "GITHUB_TOKEN");
    assert_eq!(default_vars("npm", Kind::Login).0, "NPM_USERNAME");
    assert_eq!(default_vars("npm", Kind::Login).1, "NPM_PASSWORD");
    assert!(valid_name("gh.work-2"));
    assert!(!valid_name("2fa"));
    assert!(!valid_name(""));
    assert!(!valid_name(&"a".repeat(65)));
    assert!(valid_name(&"a".repeat(64)));
    assert!(!valid_var("A-B"));
    assert!(valid_var("_A1"));
}

#[test]
fn the_environment_and_what_to_hide() {
    let e = VaultEntry {
        user_var: "REG_USERNAME".into(),
        secret_var: "REG_PASSWORD".into(),
        ..VaultEntry::new("reg", Kind::Login)
    };
    let value = VaultValue {
        username: "alice".into(),
        secret: "hunter22".into(),
    };
    let (env, hide) = environment(&[(e, value)]);
    assert_eq!(env.get("REG_USERNAME").map(String::as_str), Some("alice"));
    assert_eq!(
        env.get("REG_PASSWORD").map(String::as_str),
        Some("hunter22")
    );
    assert_eq!(env.len(), 2);
    assert_eq!(
        hide.get("hunter22").map(String::as_str),
        Some("[secret:reg]")
    );
    assert_eq!(
        hide.get("alice").map(String::as_str),
        Some("[secret:reg username]")
    );
}

#[test]
fn a_value_never_shows_in_debug() {
    let value = VaultValue {
        username: "fake-user".into(),
        secret: "fake-secret-value".into(),
    };
    let text = format!("{value:?}");
    assert!(!text.contains("fake-secret-value") && !text.contains("fake-user"));
}

#[test]
fn find_ranks_by_host_and_words() {
    let list = vec![
        VaultEntry {
            note: "my repos".into(),
            url: Some("https://api.github.com".into()),
            ..token("github", "GH_TOKEN")
        },
        VaultEntry {
            url: Some("https://registry.npmjs.org".into()),
            ..token("npm", "NPM_TOKEN")
        },
        VaultEntry {
            note: "AWS console".into(),
            user_var: "A_U".into(),
            secret_var: "A_P".into(),
            ..VaultEntry::new("aws-prod", Kind::Login)
        },
    ];
    let first = |q: &str| find(q, &list).first().map(|e| e.name.clone());
    assert_eq!(
        first("https://api.github.com/repos/x/y").as_deref(),
        Some("github")
    );
    assert_eq!(first("uploads.github.com").as_deref(), Some("github"));
    assert_eq!(
        first("publish to registry.npmjs.org").as_deref(),
        Some("npm")
    );
    let names: Vec<String> = find("aws console", &list)
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, ["aws-prod"]);
    assert!(find("gitlab.com", &list).is_empty());
    assert_eq!(find("", &list).len(), 3);
    assert_eq!(
        host_of("https://www.Example.com/x").as_deref(),
        Some("example.com")
    );
    assert_eq!(host_of("github"), None);
}

#[test]
fn problems_before_a_save() {
    let a = token("a", "A_TOKEN");
    assert_eq!(problem(&a, &[], None), None);
    assert!(problem(&a, std::slice::from_ref(&a), None).is_some());
    assert_eq!(problem(&a, std::slice::from_ref(&a), Some(&a)), None);
    let b = token("b", "A_TOKEN");
    assert!(problem(&b, std::slice::from_ref(&a), None).is_some());
    let c = VaultEntry {
        url: Some("x.com".into()),
        ..token("c", "C_URL")
    };
    assert!(problem(&c, &[], None).is_some(), "the URL's own variable");
    assert!(problem(&token("2x", "X_TOKEN"), &[], None).is_some());
    assert!(problem(&token("x", "X-TOKEN"), &[], None).is_some());
}

#[test]
fn urls_add_a_variable_and_a_label() {
    let e = VaultEntry {
        url: Some("https://api.github.com".into()),
        ..token("github", "GH_TOKEN")
    };
    assert_eq!(e.all_variables(), ["GH_TOKEN", "GITHUB_URL"]);
    let value = VaultValue {
        secret: "tok-1234".into(),
        ..VaultValue::default()
    };
    let (env, _) = environment(&[(e.clone(), value)]);
    assert_eq!(
        env.get("GITHUB_URL").map(String::as_str),
        Some("https://api.github.com")
    );
    assert_eq!(list::label(&e), "github (api.github.com)");
    let npm = VaultEntry {
        url: Some("registry.npmjs.org".into()),
        ..VaultEntry::new("npm", Kind::Token)
    };
    assert_eq!(list::label(&npm), "npm (registry.npmjs.org)");
    // Lists saved before URLs existed still load.
    let old = br#"[{"name":"a","kind":"token","note":"","userVar":"","secretVar":"A_TOKEN","updated":"2026-11-05T00:00:00Z"}]"#;
    assert_eq!(list::decode(old)[0].url_var(), None);
}

#[test]
fn a_multi_line_note_round_trips() {
    let note = "GitHub API for my repos.\nScopes: repo, read:org.\n\nRenew yearly.";
    let entry = VaultEntry {
        note: note.into(),
        ..token("github", "GITHUB_TOKEN")
    };
    let back = list::decode(&list::encode(&[entry]))[0].clone();
    assert_eq!(back.note, note);
    let text = describe(&back, day("2026-11-06"));
    assert!(
        text.contains("\n    GitHub API for my repos.\n    Scopes: repo, read:org.\n"),
        "{text}"
    );
    assert!(text.contains("\n    Renew yearly."), "{text}");
}

#[test]
fn the_how_to_is_printed_under_the_note() {
    let e = VaultEntry {
        note: "Publishing".into(),
        how_to: Some("1. Open npmjs.com\n2. Tokens > Generate".into()),
        expires: Some("2026-12-06".into()),
        ..token("npm", "NPM_TOKEN")
    };
    let text = describe(&e, day("2026-11-06"));
    let lines: Vec<&str> = text.split('\n').collect();
    let note = lines.iter().position(|l| *l == "    Publishing").unwrap();
    assert_eq!(lines[note + 1], "    How to get/renew:");
    assert_eq!(lines[note + 2], "      1. Open npmjs.com");
    assert_eq!(lines[note + 3], "      2. Tokens > Generate");
    assert!(
        lines.contains(&"    expires 2026-12-06 (in 30 days)"),
        "{text}"
    );
    assert_eq!(lines[0], "npm  (token)  $NPM_TOKEN");
}

#[test]
fn old_entries_decode_without_how_to_and_expires() {
    let old = br#"[{"name":"a","kind":"token","note":"one\ntwo","userVar":"","secretVar":"A_TOKEN","updated":"2026-11-05T00:00:00Z"}]"#;
    let e = list::decode(old)[0].clone();
    assert_eq!(e.how_to, None);
    assert_eq!(e.expires, None);
    assert_eq!(e.note, "one\ntwo");
    let text = describe(&e, day("2026-11-06"));
    assert!(!text.contains("How to get") && !text.contains("expires"));
}

#[test]
fn asking_first_is_said() {
    let e = VaultEntry {
        ask: Some(true),
        ..token("gh", "GH_TOKEN")
    };
    assert!(describe(&e, 0).ends_with("\n    The user approves each use."));
}

#[test]
fn expiry_parsing() {
    assert_eq!(expiry::parse("2026-12-06").as_deref(), Some("2026-12-06"));
    assert_eq!(expiry::parse(" 2028-02-29 ").as_deref(), Some("2028-02-29"));
    for bad in [
        "",
        "tomorrow",
        "2026-13-01",
        "2026-02-30",
        "2027-02-29",
        "26-12-06",
        "2026-1-6",
        "2026/12/06",
        "2026-12-06T10:00",
        "2026-00-10",
        "2026-01-00",
        "２０２６-12-06",
    ] {
        assert_eq!(expiry::parse(bad), None, "{bad}");
    }
}

#[test]
fn expiry_wording() {
    let today = day("2026-11-06");
    assert_eq!(
        expiry::line("2026-12-06", today),
        "expires 2026-12-06 (in 30 days)"
    );
    assert_eq!(
        expiry::line("2026-11-07", today),
        "expires 2026-11-07 (in 1 day, renew soon)"
    );
    assert_eq!(
        expiry::line("2026-11-06", today),
        "expires 2026-11-06 (today, renew soon)"
    );
    assert_eq!(expiry::line("2026-11-05", today), "EXPIRED 2026-11-05");
    assert_eq!(expiry::line("nonsense", today), "expires nonsense");
    assert_eq!(
        expiry::status("2026-12-06", today),
        Some(Status::InDays(30))
    );
    assert_eq!(expiry::status("2025-01-01", today), Some(Status::Expired));
    assert_eq!(expiry::status("nonsense", today), None);
    let e = VaultEntry {
        expires: Some("2026-11-01".into()),
        ..token("x", "X_TOKEN")
    };
    assert!(describe(&e, today).contains("\n    EXPIRED 2026-11-01"));
}

#[test]
fn renew_soon_shows_within_seven_days() {
    let today = day("2026-11-06");
    assert_eq!(
        expiry::line("2026-11-13", today),
        "expires 2026-11-13 (in 7 days, renew soon)"
    );
    assert_eq!(
        expiry::line("2026-11-14", today),
        "expires 2026-11-14 (in 8 days)"
    );
}

#[test]
fn expiry_day_conversion() {
    assert_eq!(expiry::day_text(day("2026-12-06")), "2026-12-06");
    assert_eq!(expiry::day_text(day("2028-02-29")), "2028-02-29");
    assert_eq!(expiry::day_text(0), "1970-01-01");
    // A local time near midnight UTC is still a day number.
    let noon = day("2026-12-06") as f64 * 86_400.0 + 43_200.0;
    assert!((expiry::today_local(noon) - day("2026-12-06")).abs() <= 1);
}

#[test]
fn edit_prefill_merges_passed_fields_over_saved_ones() {
    let saved = VaultEntry {
        note: "old note".into(),
        how_to: Some("old steps".into()),
        expires: Some("2026-01-01".into()),
        url: Some("https://api.github.com".into()),
        ..token("gh", "GH_TOKEN")
    };
    assert_eq!(saved.prefilled(&request("gh")), saved);
    let some = saved.prefilled(&VaultRequest {
        note: Some("new\nnote".into()),
        how_to: Some("new steps".into()),
        ..request("gh")
    });
    assert_eq!(some.note, "new\nnote");
    assert_eq!(some.how_to.as_deref(), Some("new steps"));
    assert_eq!(some.url.as_deref(), Some("https://api.github.com"));
    assert_eq!(some.expires.as_deref(), Some("2026-01-01"));
    let all = saved.prefilled(&VaultRequest {
        url: Some("https://x.io".into()),
        note: Some("n".into()),
        how_to: Some("h".into()),
        expires: Some("2027-03-04".into()),
        kind: Some("login".into()),
        ..request("other")
    });
    assert_eq!(
        (all.url.as_deref(), all.note.as_str(), all.how_to.as_deref()),
        (Some("https://x.io"), "n", Some("h"))
    );
    assert_eq!(all.expires.as_deref(), Some("2027-03-04"));
    // Identity, kind and variables never change, and a bad date is ignored.
    assert_eq!(
        (all.name.as_str(), all.secret_var.as_str()),
        ("gh", "GH_TOKEN")
    );
    assert_eq!(all.kind, Kind::Token);
    let soon = VaultRequest {
        expires: Some("soon".into()),
        ..request("gh")
    };
    assert_eq!(
        saved.prefilled(&soon).expires.as_deref(),
        Some("2026-01-01")
    );
}

#[test]
fn a_new_name_starts_from_the_request() {
    let e = VaultEntry::from_request(&VaultRequest {
        kind: Some("login".into()),
        url: Some("registry.example.com".into()),
        ..request("reg")
    });
    assert_eq!(e.kind, Kind::Login);
    assert_eq!(
        (e.user_var.as_str(), e.secret_var.as_str()),
        ("REG_USERNAME", "REG_PASSWORD")
    );
    assert_eq!(e.url.as_deref(), Some("registry.example.com"));
    assert_eq!(VaultEntry::from_request(&request("t")).kind, Kind::Token);
}

#[test]
fn request_changes_are_listed_with_their_old_values() {
    let saved = VaultEntry {
        note: "old note".into(),
        url: Some("https://api.github.com/v3".into()),
        how_to: Some("old steps".into()),
        expires: Some("2026-01-01".into()),
        ..token("gh", "GH_TOKEN")
    };
    let same = PrefillChanges::of(
        &saved,
        &VaultRequest {
            url: Some("https://api.github.com/v3".into()),
            note: Some("old note".into()),
            expires: Some("2026-01-01".into()),
            ..request("gh")
        },
    );
    assert_eq!(same, PrefillChanges::default());
    let c = PrefillChanges::of(
        &saved,
        &VaultRequest {
            url: Some("https://api.github-cdn.io/v3".into()),
            note: Some("n".into()),
            how_to: Some("h".into()),
            expires: Some("2027-03-04".into()),
            ..request("gh")
        },
    );
    assert_eq!(c.old[&Field::Note], "old note");
    assert_eq!(c.old[&Field::Url], "https://api.github.com/v3");
    assert_eq!(c.old[&Field::HowTo], "old steps");
    assert_eq!(c.old[&Field::Expires], "2026-01-01");
    assert_eq!(
        c.host_change,
        Some(HostChange {
            from: Some("https://api.github.com".into()),
            to: Some("https://api.github-cdn.io".into())
        })
    );
    let with_url = |u: &str| VaultRequest {
        url: Some(u.into()),
        ..request("gh")
    };
    let path = PrefillChanges::of(&saved, &with_url("https://api.github.com/v4"));
    assert!(path.old.contains_key(&Field::Url));
    assert_eq!(
        path.host_change, None,
        "the path alone moving is not a host change"
    );
    assert_eq!(
        PrefillChanges::of(&saved, &with_url("HTTPS://API.GitHub.com")).host_change,
        None
    );
    let bare = token("gh", "GH_TOKEN");
    let from = PrefillChanges::of(&bare, &with_url("https://evil.io"));
    assert_eq!(
        from.host_change,
        Some(HostChange {
            from: None,
            to: Some("https://evil.io".into())
        })
    );
    assert_eq!(from.old[&Field::Url], "");
}

#[test]
fn a_scheme_downgrade_or_another_port_moves_the_secret() {
    let saved = VaultEntry {
        url: Some("https://github.com".into()),
        ..token("gh", "GH_TOKEN")
    };
    let change = |u: &str| {
        PrefillChanges::of(
            &saved,
            &VaultRequest {
                url: Some(u.into()),
                ..request("gh")
            },
        )
        .host_change
    };
    let down = change("http://github.com").unwrap();
    assert_eq!(down.to.as_deref(), Some("http://github.com"));
    assert_eq!(
        down.warning(),
        "The request changes where this secret is sent: https://github.com → http://github.com"
    );
    assert_eq!(
        change("https://github.com:8080").unwrap().to.as_deref(),
        Some("https://github.com:8080")
    );
    assert_eq!(change("https://github.com:443"), None);
    assert_eq!(change("github.com/v3"), None, "no scheme is https");
    assert_eq!(change("https://GitHub.com/other/path"), None);
}

#[test]
fn each_change_marker_names_its_field() {
    assert_eq!(Field::Note.marker(), "Note changed by the request");
    assert_eq!(Field::HowTo.marker(), "How-to changed by the request");
    assert_eq!(Field::Url.marker(), "URL changed by the request");
    assert_eq!(Field::Expires.marker(), "Expiry changed by the request");
}

#[test]
fn the_skill_documents_how_and_expires() {
    for tool in SkillTool::ALL {
        let text = skill::content("/x/notchling", &[], tool);
        for needle in [
            "reminds the user",
            "--how",
            "--expires",
            "YYYY-MM-DD",
            "none yet",
        ] {
            assert!(text.contains(needle), "{tool:?}: {needle}");
        }
        assert!(text.contains("'/x/notchling' secret github -- "));
        for slot in ["{saved}", "{binary}", "{sandbox_note}"] {
            assert!(!text.contains(slot), "{slot} left in the text");
        }
    }
    let names = skill::names(&[
        VaultEntry {
            url: Some("https://api.github.com".into()),
            ..token("github", "GH_TOKEN")
        },
        token("aws", "AWS_TOKEN"),
    ]);
    let text = skill::content("/x/notchling", &names, SkillTool::Codex);
    assert!(text.contains("saved now: aws, github (api.github.com)."));
    assert!(text.contains("## In Codex's sandbox"));
}

#[test]
fn a_rename_moves_default_variables_only() {
    let e = token("sd-claude", "SD_CLAUDE_TOKEN");
    assert_eq!(e.renamed("claude-worker").secret_var, "CLAUDE_WORKER_TOKEN");
    let custom = token("sd-claude", "MY_TOKEN");
    assert_eq!(custom.renamed("claude-worker").secret_var, "MY_TOKEN");
}
