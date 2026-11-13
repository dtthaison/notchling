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
        let s = c.to_string();
        form.key(&s, Some(&s), false);
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
    f.key("tab", None, false);
    assert_eq!(f.focus, Field::Url);
    type_text(&mut f, "https://x.example");
    assert_eq!(
        f.vars_summary(),
        "$MY_API_USERNAME $MY_API_PASSWORD $MY_API_URL"
    );
    f.focus = Field::Note;
    assert_eq!(f.key("enter", Some("\r"), false), Typed::Changed);
    assert_eq!(f.note, "\n");
    f.focus = Field::Secret;
    assert_eq!(f.key("enter", Some("\r"), false), Typed::Submit);
    assert_eq!(f.key("v", Some("v"), true), Typed::Ignored, "⌘V");
    assert_eq!(f.key("escape", Some("\u{1b}"), false), Typed::Ignored);
    assert!(!f.can_save(), "a login needs a value");
    type_text(&mut f, "s3cret");
    assert!(f.can_save());
    assert_eq!(f.secret_mask(), "••••••");
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
fn editing_keeps_the_value_unless_one_is_typed_and_renames_replace() {
    let mut f = Form::edit(&saved());
    assert_eq!((f.title(), f.focus), ("Edit Secret", Field::Secret));
    assert_eq!(f.secret_placeholder(), "Unchanged");
    assert!(!f.editing_how_to, "a saved how-to shows as the help box");
    assert!(f.can_save(), "editing needs no new value");
    let (entry, value, replacing) = f.to_save().unwrap();
    assert_eq!(value, None);
    assert_eq!(replacing, None);
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
    };
    let f = Form::request(&pending);
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
    f.focus = Field::Url;
    assert_eq!(f.key("enter", Some("\r"), false), Typed::Ignored);
    f.focus = Field::Secret;
    assert_eq!(f.key("enter", Some("\r"), false), Typed::Ignored);
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
}

#[test]
fn paste_copy_and_cut_work_in_the_fields_but_the_value_is_never_copied() {
    let mut f = Form::add();
    assert_eq!(f.paste("my-api\n"), Typed::Changed);
    assert_eq!(f.name, "my-api", "a single-line field drops the line break");
    assert_eq!(
        f.secret_var, "MY_API_TOKEN",
        "the variables follow a pasted name"
    );
    assert_eq!(f.copy(), Some("my-api".to_owned()));
    assert_eq!(f.cut(), Some("my-api".to_owned()));
    assert_eq!(f.name, "");
    f.focus = Field::Note;
    f.paste("line one\nline two");
    assert_eq!(f.note, "line one\nline two", "a multi-line field keeps it");
    f.focus = Field::Secret;
    assert_eq!(f.paste("ghp_abc\r\n"), Typed::Changed);
    assert_eq!(f.secret, "ghp_abc", "the value's trailing line break goes");
    assert_eq!(f.copy(), None, "the value is never copied");
    assert_eq!(f.cut(), None, "nor cut");
    assert_eq!(f.secret, "ghp_abc");
    assert_eq!(f.paste(""), Typed::Ignored);
    f.focus = Field::Url;
    f.paste("https://x\u{7}.example");
    assert_eq!(f.url, "https://x.example");
}
