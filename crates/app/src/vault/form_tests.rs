use super::*;
use notchling_engine::vault::entry::{HostChange, VaultRequest};
use std::collections::BTreeMap;

const TODAY: i64 = 20_765;

fn saved() -> VaultEntry {
    let mut e = VaultEntry::new("github", Kind::Token);
    e.url = Some("https://api.github.com".into());
    e.note = "GitHub API".into();
    e.how_to = Some("Settings > Developer settings > Tokens".into());
    e.expires = Some("2026-12-08".into());
    e
}

fn type_text(form: &mut Form, text: &str) {
    for c in text.chars() {
        let field = form.focus;
        let typed = format!("{}{c}", form.text(field));
        form.set_text(field, &typed);
    }
}

#[test]
fn a_new_form_types_its_name_and_the_variables_follow() {
    let mut f = Form::add();
    assert_eq!((f.title(), f.focus), ("Add Secret", Field::Name));
    assert!(!f.can_save());
    type_text(&mut f, "my-api");
    assert_eq!(f.secret_var, "MY_API_TOKEN");
    assert_eq!(f.vars_summary(), "$MY_API_TOKEN");
    f.set_kind(Kind::Login);
    assert_eq!(
        (f.user_var.as_str(), f.secret_var.as_str()),
        ("MY_API_USERNAME", "MY_API_PASSWORD")
    );
    assert_eq!(
        f.fields(),
        [
            Field::Name,
            Field::Url,
            Field::Username,
            Field::Secret,
            Field::Note,
            Field::HowTo
        ]
    );
    f.focus_next();
    assert_eq!(f.focus, Field::Url);
    f.focus_previous();
    assert_eq!(f.focus, Field::Name);
    f.focus_previous();
    assert_eq!(f.focus, Field::HowTo);
    f.focus_next();
    f.focus_next();
    assert_eq!(f.focus, Field::Url);
    type_text(&mut f, "https://x.example");
    assert_eq!(
        f.vars_summary(),
        "$MY_API_USERNAME $MY_API_PASSWORD $MY_API_URL"
    );
    assert_eq!(f.enter(Field::Note), Enter::Newline);
    assert_eq!(f.enter(Field::HowTo), Enter::Newline);
    assert_eq!(f.enter(Field::Secret), Enter::Save);
    assert_eq!(f.enter(Field::Url), Enter::Save);
    f.focus = Field::Secret;
    assert!(!f.can_save(), "a login needs a value");
    type_text(&mut f, "s3cret");
    assert!(f.can_save());
    assert!(!format!("{f:?}").contains("s3cret"));
    f.advanced = true;
    f.focus = Field::SecretVar;
    type_text(&mut f, "X");
    f.focus = Field::Name;
    type_text(&mut f, "2");
    assert_eq!(f.secret_var, "MY_API_PASSWORDX");
    let (entry, value, replacing) = f.to_save().unwrap();
    assert_eq!(entry.name, "my-api2");
    assert_eq!(entry.kind, Kind::Login);
    assert_eq!(entry.url.as_deref(), Some("https://x.example"));
    assert_eq!(entry.note, "");
    assert_eq!(value.unwrap().secret, "s3cret");
    assert_eq!(replacing, None);
    f.forget_value();
    assert!(f.secret.is_empty() && f.username.is_empty());
}

#[test]
fn editing_keeps_the_value_unless_one_is_typed_and_names_the_secret_it_replaces() {
    let mut f = Form::edit(&saved());
    assert_eq!((f.title(), f.focus), ("Edit Secret", Field::Secret));
    assert_eq!(f.secret_placeholder(), "Unchanged");
    assert!(!f.editing_how_to, "a saved how-to shows as the help box");
    assert!(f.can_save(), "editing needs no new value");
    let (entry, value, replacing) = f.to_save().unwrap();
    assert_eq!(value, None);
    assert_eq!(replacing.as_deref(), Some("github"));
    assert_eq!(entry.name, "github");
    assert_eq!(entry.expires.as_deref(), Some("2026-12-08"));
    assert_eq!(
        entry.how_to.as_deref(),
        Some("Settings > Developer settings > Tokens")
    );
    f.name = "gh".into();
    let (entry, _, replacing) = f.to_save().unwrap();
    assert_eq!(
        (entry.name.as_str(), entry.secret_var.as_str()),
        ("gh", "GH_TOKEN")
    );
    assert_eq!(replacing.as_deref(), Some("github"));
    f.no_expiry = true;
    f.ask = true;
    let (entry, _, _) = f.to_save().unwrap();
    assert_eq!((entry.expires, entry.ask), (None, Some(true)));
    f.no_expiry = false;
    f.expires = "soon".into();
    assert!(f.to_save().unwrap_err().contains("YYYY-MM-DD"));
    f.name = "9lives".into();
    assert!(f.to_save().is_err(), "a name starts with a letter");
    let login = Form::edit(&VaultEntry::new("npm", Kind::Login));
    assert_eq!(
        login.focus,
        Field::Username,
        "a login without a username asks for it"
    );
}

#[test]
fn renewing_needs_a_new_value_and_an_expiry_choice() {
    let mut f = Form::renew(&saved());
    assert_eq!(
        (f.title(), f.expires_label()),
        ("Renew Secret", "New expiry")
    );
    assert_eq!(f.secret_placeholder(), "New value");
    assert!(!f.expiry_chosen && f.expires.is_empty());
    assert_eq!(f.blocker(), Some("Enter the new value."));
    assert!(!f.can_save());
    type_text(&mut f, "n3w");
    assert_eq!(f.blocker(), Some("Pick the new expiry date, or No expiry."));
    assert!(!f.can_save());
    f.choose_expiry(false, TODAY);
    assert_eq!(f.expires, "2026-12-08", "30 days out");
    assert!(f.can_save() && f.blocker().is_none());
    let (entry, value, replacing) = f.to_save().unwrap();
    assert_eq!(entry.name, "github");
    assert_eq!(value.unwrap().secret, "n3w");
    assert_eq!(replacing.as_deref(), Some("github"));
    assert!(f.fields().contains(&Field::Expires));
    f.choose_expiry(true, TODAY);
    assert!(!f.fields().contains(&Field::Expires));
    assert_eq!(f.renew_note(), None);
    let mut odd = Form::renew(&VaultEntry::new("x", Kind::Token));
    odd.url = "ftp://x".into();
    assert!(odd
        .renew_note()
        .is_some_and(|n| n.contains("isn't http or https")));
}

#[test]
fn an_agents_request_shows_its_reason_and_marks_what_it_changed() {
    let request = VaultRequest {
        name: "github".into(),
        url: Some("https://evil.example".into()),
        note: Some("new note".into()),
        reason: Some("to push the release".into()),
        ..VaultRequest::default()
    };
    let pending = PendingRequest {
        id: "r1".into(),
        entry: saved().prefilled(&request),
        existing: true,
        changes: PrefillChanges {
            old: BTreeMap::from([
                (Changed::Note, "GitHub API".to_owned()),
                (Changed::Url, "https://api.github.com".to_owned()),
                (Changed::Expires, String::new()),
            ]),
            host_change: Some(HostChange {
                from: Some("https://api.github.com".into()),
                to: Some("https://evil.example".into()),
            }),
        },
        request,
        session: None,
        verified: None,
        snapshot: None,
    };
    let f = Form::request(&pending);
    assert_eq!(f.to_save().unwrap().2.as_deref(), Some("github"));
    assert_eq!(f.title(), "Secret Vault");
    assert_eq!(f.request.as_deref(), Some("r1"));
    assert_eq!(
        f.reason_line().as_deref(),
        Some("An agent asks: to push the release")
    );
    assert_eq!(f.url, "https://evil.example");
    assert_eq!(
        f.marker(Changed::Note),
        Some(Marker {
            text: "Note changed by the request",
            was: "Was: GitHub API".into()
        })
    );
    assert_eq!(f.marker(Changed::Expires).unwrap().was, "Was: no expiry");
    assert_eq!(f.marker(Changed::HowTo), None);
    assert_eq!(
        f.host_warning().as_deref(),
        Some("The request changes where this secret is sent: https://api.github.com → https://evil.example")
    );
    let mut f = f;
    assert!(!f.can_save(), "the new address isn't confirmed");
    assert_eq!(f.blocker(), Some(CONFIRM_HOST_BLOCKER));
    f.confirm_host();
    assert!(f.can_save(), "an existing entry keeps its value");
    assert_eq!(f.blocker(), None);
    assert_eq!(f.enter(Field::Url), Enter::Ignore);
    assert_eq!(f.enter(Field::Secret), Enter::Ignore);
    assert_eq!(f.enter(Field::Note), Enter::Newline);
    let fresh = PendingRequest {
        existing: false,
        entry: VaultEntry::new("npm", Kind::Token),
        changes: PrefillChanges::default(),
        ..pending
    };
    let f = Form::request(&fresh);
    assert!(
        f.original.is_none() && !f.can_save(),
        "a new one needs a value"
    );
    assert_eq!(f.to_save().unwrap().2, None, "a new one replaces nothing");
}

#[test]
fn an_edited_field_updates_the_form_once_and_the_variables_follow_a_name() {
    let mut f = Form::add();
    assert!(f.set_text(Field::Name, "my-api"));
    assert_eq!(f.name, "my-api");
    assert_eq!(
        f.secret_var, "MY_API_TOKEN",
        "the variables follow the name"
    );
    assert!(
        !f.set_text(Field::Name, "my-api"),
        "the same text is no edit"
    );
    f.error = Some("old".into());
    assert!(!f.set_text(Field::Name, "my-api"));
    assert_eq!(f.error.as_deref(), Some("old"), "no edit leaves the error");
    assert!(f.set_text(Field::Note, "line one\nline two"));
    assert_eq!(f.error, None, "an edit clears the error");
    assert!(f.set_text(Field::SecretVar, "MINE"));
    assert!(f.custom_vars, "an edited variable stops following the name");
    f.set_text(Field::Name, "other");
    assert_eq!(f.secret_var, "MINE");
    assert!(f.set_text(Field::Secret, "ghp_abc"));
    assert_eq!(f.secret, "ghp_abc");
    assert!(!format!("{f:?}").contains("ghp_abc"));
}

#[test]
fn only_a_name_with_letters_can_be_copied() {
    assert_eq!(copyable("github"), Some("github"));
    assert_eq!(copyable("  github \n"), Some("github"));
    assert_eq!(copyable(""), None);
    assert_eq!(copyable(" \t "), None);
    assert_eq!(copyable("my token"), Some("my token"));
}

const PEM_BODY: &str = "-----BEGIN PRIVATE KEY-----\nNLFAKE0aZk1q2Xx9fPzT0bYw4Lr8mNcV3uHj5Ka1dEo6sGtQ7wRv\nNLFAKE1pL0cT8yUeB4nHs2JdX6fMq9ZrW3gVk5Ao7iCtYb1DxSu\nNLFAKE2Hj4Kd8Qw1ErTy6UiOp3AsDf9GhJk2LzXc5VbNm7Qw0Er\n-----END PRIVATE KEY-----";

#[test]
fn a_multi_line_value_is_saved_byte_for_byte() {
    let variants = [
        PEM_BODY.to_owned(),
        format!("{PEM_BODY}\n"),
        format!("{PEM_BODY}\n\n"),
        PEM_BODY.replace('\n', "\r\n"),
        format!("{}\r\n", PEM_BODY.replace('\n', "\r\n")),
        format!("\n{PEM_BODY}\n"),
        format!("{PEM_BODY} \n  \n"),
        "line one\rline two".to_owned(),
    ];
    for value in variants {
        let mut f = Form::add();
        assert!(f.set_text(Field::Name, "deploy-key"));
        assert!(f.set_text(Field::Secret, &value));
        assert_eq!(f.text(Field::Secret), value);
        assert!(f.can_save());
        let (_, saved, _) = f.to_save().unwrap();
        let saved = saved.expect("a value to save");
        assert_eq!(saved.secret.as_bytes(), value.as_bytes());
        assert!(f.secret_has_lines());
        assert_eq!(f.enter(Field::Secret), Enter::Newline);
        assert!(!format!("{f:?}").contains("NLFAKE"));
    }
}

#[test]
fn enter_in_the_value_adds_a_line_only_once_it_has_one() {
    let mut f = Form::add();
    assert_eq!(f.enter(Field::Secret), Enter::Save);
    f.set_text(Field::Secret, "one-line-token");
    assert!(!f.secret_has_lines());
    assert_eq!(f.enter(Field::Secret), Enter::Save);
    f.set_text(Field::Secret, "first\nsecond");
    assert_eq!(f.enter(Field::Secret), Enter::Newline);
    f.set_text(Field::Secret, "first");
    assert_eq!(f.enter(Field::Secret), Enter::Save);
    assert!(Field::Secret.takes_lines());
    assert!(!Field::Secret.multiline());
    assert!(!Field::Url.takes_lines());
}
