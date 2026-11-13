use notchling_engine::vault::entry::{Kind, VaultEntry};
use notchling_engine::vault::expiry::{self, Status};

use crate::acting::Control;
use crate::switches::{Switch, Switches};

pub const TITLE: &str = "Secret Vault";
pub const EMPTY: &str = "Save a login or a token once. Agents use it by name, and its value never goes through a chat or into a transcript.";
pub const USAGE: &str = "Only Notchling can read the values. Agents run `notchling secret <name> -- <command>`: the command gets them as environment variables, and its output comes back with them hidden. An agent that needs one you haven't saved opens this form for you.";
pub const RENEW_TIP: &str = "Open the page for a new value, then enter it here";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpiryLabel {
    pub text: String,
    pub expired: bool,
}

pub fn expiry_label(day: &str, today: i64) -> ExpiryLabel {
    match expiry::status(day, today) {
        Some(Status::Expired) => ExpiryLabel {
            text: format!("Expired {day}"),
            expired: true,
        },
        Some(_) => ExpiryLabel {
            text: match expiry::phrase(day, today) {
                Some(phrase) => format!("Expires {day} ({phrase})"),
                None => format!("Expires {day}"),
            },
            expired: false,
        },
        None => ExpiryLabel {
            text: format!("Expires {day}"),
            expired: false,
        },
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub kind: &'static str,
    pub asks_first: bool,
    pub url: Option<String>,
    pub vars: String,
    pub note: Option<String>,
    pub expiry: Option<ExpiryLabel>,
    pub renew: Option<Control>,
    pub edit: Control,
    pub remove: Control,
    pub remove_tip: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct List {
    pub rows: Vec<Row>,
    pub empty: Option<(&'static str, Control)>,
    pub add: Option<Control>,
    pub usage: &'static str,
    pub off: Option<&'static str>,
    pub error: Option<String>,
}

pub fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Login => "Login",
        Kind::Token => "Token",
    }
}

pub fn list(
    entries: &[VaultEntry],
    switches: &Switches,
    today: i64,
    error: Option<String>,
) -> List {
    let control = switches.control(Switch::Vault);
    let rows = entries
        .iter()
        .map(|e| Row {
            name: e.name.clone(),
            kind: kind_label(e.kind),
            asks_first: e.asks(),
            url: e.url.clone().filter(|u| !u.is_empty()),
            vars: e
                .all_variables()
                .iter()
                .map(|v| format!("${v}"))
                .collect::<Vec<_>>()
                .join("  "),
            note: Some(e.note.clone()).filter(|n| !n.trim().is_empty()),
            expiry: e.expires.as_deref().map(|d| expiry_label(d, today)),
            renew: (e.expires.is_some()
                || e.how_to.as_deref().is_some_and(|h| !h.trim().is_empty()))
            .then_some(control),
            edit: control,
            remove: control,
            remove_tip: format!("Remove {}", e.name),
        })
        .collect::<Vec<_>>();
    List {
        empty: rows.is_empty().then_some((EMPTY, control)),
        add: (!rows.is_empty()).then_some(control),
        rows,
        usage: USAGE,
        off: switches.message(Switch::Vault),
        error,
    }
}

pub fn remove_alert(name: &str) -> (String, &'static str) {
    (
        format!("Remove {name}?"),
        "Its value is deleted from Keychain. Agents that use it will be told it's missing.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::switches::VAULT_OFF;

    const TODAY: i64 = 20_765;

    #[test]
    fn expiry_reads_like_the_swift_label() {
        assert_eq!(
            expiry_label("2026-11-01", TODAY),
            ExpiryLabel {
                text: "Expired 2026-11-01".into(),
                expired: true
            }
        );
        assert_eq!(
            expiry_label("2026-11-08", TODAY).text,
            "Expires 2026-11-08 (today)"
        );
        assert_eq!(
            expiry_label("2026-11-09", TODAY).text,
            "Expires 2026-11-09 (in 1 day)"
        );
        assert_eq!(
            expiry_label("2026-12-08", TODAY).text,
            "Expires 2026-12-08 (in 30 days)"
        );
        assert_eq!(expiry_label("someday", TODAY).text, "Expires someday");
    }

    #[test]
    fn rows_show_everything_but_the_value_and_off_says_why() {
        let mut github = VaultEntry::new("github", Kind::Token);
        github.url = Some("https://api.github.com".into());
        github.note = "GitHub API for my repos".into();
        github.expires = Some("2026-12-08".into());
        github.ask = Some(true);
        let mut npm = VaultEntry::new("npm", Kind::Login);
        npm.how_to = Some("npmjs.com > Access Tokens".into());
        let plain = VaultEntry::new("plain", Kind::Token);
        let l = list(&[github, npm, plain], &Switches::all_on(), TODAY, None);
        assert_eq!(l.rows.len(), 3);
        let g = &l.rows[0];
        assert_eq!((g.kind, g.asks_first), ("Token", true));
        assert_eq!(g.vars, "$GITHUB_TOKEN  $GITHUB_URL");
        assert_eq!(g.note.as_deref(), Some("GitHub API for my repos"));
        assert_eq!(
            g.expiry.as_ref().unwrap().text,
            "Expires 2026-12-08 (in 30 days)"
        );
        assert!(g.renew.is_some_and(|c| c.enabled));
        assert_eq!(g.remove_tip, "Remove github");
        let n = &l.rows[1];
        assert_eq!(
            (n.kind, n.vars.as_str()),
            ("Login", "$NPM_USERNAME  $NPM_PASSWORD")
        );
        assert!(n.renew.is_some(), "a how-to earns Renew");
        assert!(n.note.is_none() && n.expiry.is_none());
        assert!(l.rows[2].renew.is_none(), "nothing to renew with");
        assert!(l.empty.is_none() && l.add.is_some_and(|c| c.enabled));
        assert_eq!(l.off, None);

        let off = list(
            &[VaultEntry::new("x", Kind::Token)],
            &Switches::all_off(),
            TODAY,
            None,
        );
        assert_eq!(off.off, Some(VAULT_OFF));
        assert_eq!(off.rows[0].edit.tooltip, Some(VAULT_OFF));
        assert_eq!(off.add.unwrap().tooltip, Some(VAULT_OFF));
        let empty = list(&[], &Switches::all_on(), TODAY, Some("boom".into()));
        assert_eq!(empty.empty.unwrap().0, EMPTY);
        assert!(empty.add.is_none());
        assert_eq!(empty.error.as_deref(), Some("boom"));
        assert_eq!(remove_alert("github").0, "Remove github?");
    }
}
