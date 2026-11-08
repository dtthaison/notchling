//! Swift `SecretVaultTests`' reminders and Renew, ported: which reminder is due, once per
//! threshold, reset by a new day or a new value, and what Renew may open.

use super::entry::{Kind, VaultEntry};
use super::expiry;
use super::reminders::{self, Due, Sent, Threshold};
use super::url::{renew_refusal, renew_url};

fn day(text: &str) -> i64 {
    expiry::day_number(text).unwrap()
}

fn token(name: &str, var: &str) -> VaultEntry {
    VaultEntry {
        secret_var: var.into(),
        ..VaultEntry::new(name, Kind::Token)
    }
}

fn remind(expires: Option<&str>, now: &str, sent: &Sent) -> Vec<Due> {
    let e = VaultEntry {
        expires: expires.map(str::to_string),
        ..token("sd-worker-m1", "SD_TOKEN")
    };
    reminders::due(&[e], sent, day(now))
}

#[test]
fn reminder_thresholds() {
    let none = Sent::new();
    let at = |d: &str| remind(Some(d), "2026-11-06", &none);
    assert!(at("2026-11-14").is_empty(), "8 days out is nothing");
    assert!(remind(None, "2026-11-06", &none).is_empty());
    let week = at("2026-11-13");
    assert_eq!(week[0].threshold, Threshold::Week);
    assert_eq!(
        week[0].message,
        "sd-worker-m1 expires in 7 days (2026-11-13)"
    );
    assert_eq!(
        at("2026-11-07")[0].message,
        "sd-worker-m1 expires in 1 day (2026-11-07)"
    );
    assert_eq!(at("2026-11-07")[0].threshold, Threshold::Day);
    assert_eq!(
        at("2026-11-06")[0].message,
        "sd-worker-m1 expires today (2026-11-06)"
    );
    assert_eq!(at("2026-11-06")[0].threshold, Threshold::Today);
    assert_eq!(at("2026-11-05")[0].threshold, Threshold::Expired);
    assert_eq!(
        at("2026-11-05")[0].message,
        "sd-worker-m1 expired on 2026-11-05"
    );
    assert_eq!(at("2026-11-05")[0].title(), "Secret expired");
    assert_eq!(at("2026-11-07")[0].title(), "Secret expiring");
    // Off for days: the nearest threshold fires once, with the real days left.
    assert_eq!(
        at("2026-11-09")[0].message,
        "sd-worker-m1 expires in 3 days (2026-11-09)"
    );
}

#[test]
fn a_reminder_fires_once_per_threshold() {
    let none = Sent::new();
    let due = remind(Some("2026-11-13"), "2026-11-06", &none).remove(0);
    let sent = reminders::mark_sent(&due, &none);
    assert!(remind(Some("2026-11-13"), "2026-11-06", &sent).is_empty());
    assert!(
        remind(Some("2026-11-13"), "2026-11-07", &sent).is_empty(),
        "6 days is still the 7-day reminder"
    );
    assert_eq!(
        remind(Some("2026-11-13"), "2026-11-12", &sent)[0].threshold,
        Threshold::Day
    );
    let expired = remind(Some("2026-11-01"), "2026-11-06", &sent).remove(0);
    let after = reminders::mark_sent(&expired, &sent);
    assert!(remind(Some("2026-11-01"), "2026-11-06", &after).is_empty());
    assert!(
        remind(Some("2026-11-01"), "2026-12-30", &after).is_empty(),
        "expired is announced once"
    );
}

#[test]
fn reminder_memory_resets_when_the_day_changes_or_on_renew() {
    let none = Sent::new();
    let due = remind(Some("2026-11-13"), "2026-11-06", &none).remove(0);
    let sent = reminders::mark_sent(&due, &none);
    assert_eq!(
        remind(Some("2026-11-10"), "2026-11-06", &sent)[0].threshold,
        Threshold::Week,
        "a new date starts again"
    );
    let cleared = reminders::clearing("sd-worker-m1", &sent);
    assert_eq!(remind(Some("2026-11-13"), "2026-11-06", &cleared).len(), 1);
    let npm = Due {
        name: "npm".into(),
        id: "npm".into(),
        day: "2026-11-13".into(),
        threshold: Threshold::Week,
        message: String::new(),
    };
    let other = reminders::mark_sent(&npm, &sent);
    let keys: Vec<String> = reminders::clearing("sd-worker-m1", &other)
        .into_keys()
        .collect();
    assert_eq!(keys, ["npm|2026-11-13"]);
    let entries = [VaultEntry {
        expires: Some("2026-11-13".into()),
        ..token("npm", "N")
    }];
    let kept: Vec<String> = reminders::pruned(&other, &entries).into_keys().collect();
    assert_eq!(kept, ["npm|2026-11-13"]);
}

#[test]
fn renew_opens_only_http_and_https() {
    let url = |u: &str| renew_url(Some(u));
    assert_eq!(
        url("https://api.github.com").as_deref(),
        Some("https://github.com/settings/personal-access-tokens")
    );
    assert_eq!(
        url("github.com/org/repo").as_deref(),
        Some("https://github.com/settings/personal-access-tokens")
    );
    assert_eq!(
        url("https://registry.npmjs.org/-/tokens").as_deref(),
        Some("https://registry.npmjs.org/-/tokens")
    );
    assert_eq!(
        url("example.com/keys").as_deref(),
        Some("https://example.com/keys")
    );
    assert_eq!(
        url("http://localhost:3000/tokens").as_deref(),
        Some("http://localhost:3000/tokens")
    );
    assert_eq!(renew_url(None), None);
    assert_eq!(url(""), None);
    for refused in [
        "file:///Users/me/evil.command",
        "smb://evil.example/share",
        "vscode://x/y",
        "javascript:alert(1)",
        "ssh://host",
        "x-apple.systempreferences:com.apple.x",
        "https://",
        "ftp://example.com/f",
    ] {
        assert_eq!(url(refused), None, "{refused}");
    }
    assert_eq!(
        renew_refusal(Some("smb://evil.example/share")).as_deref(),
        Some("smb://evil.example/share")
    );
    assert_eq!(renew_refusal(Some("https://example.com")), None);
    assert_eq!(renew_refusal(None), None);
}
