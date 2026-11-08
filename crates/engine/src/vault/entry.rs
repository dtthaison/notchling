//! One saved secret's public part (Swift `VaultEntry`), its value (`VaultValue`), what an agent's
//! `secret-request` asks for (`VaultRequest`'s form fields) and what that request changes in a
//! saved secret's form (`VaultPrefillChanges`).
//!
//! The field names on disk are the Swift app's (`howTo`, `userVar`, `secretVar`), so a list can
//! move between the two at handover.

use super::expiry;
use super::url;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// A login (username and password) or a token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Login,
    Token,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Login => "login",
            Kind::Token => "token",
        }
    }

    /// `login` or `token`; anything else is `None`.
    pub fn parse(text: &str) -> Option<Kind> {
        match text {
            "login" => Some(Kind::Login),
            "token" => Some(Kind::Token),
            _ => None,
        }
    }
}

/// The list's part of a secret: no value. The fields are in the order `vault.json` sorts them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultEntry {
    /// Ask the user before each use. Missing in lists saved before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<bool>,
    /// The last day it works, `YYYY-MM-DD` ([`expiry`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    /// How to get or renew it, multi-line, Markdown links allowed.
    #[serde(default, rename = "howTo", skip_serializing_if = "Option::is_none")]
    pub how_to: Option<String>,
    pub kind: Kind,
    pub name: String,
    /// Shown to agents by `secrets`: what it's for. May span lines.
    #[serde(default)]
    pub note: String,
    /// Variable for the password or token.
    #[serde(default, rename = "secretVar")]
    pub secret_var: String,
    /// When it was last saved, ISO 8601 (`2026-11-05T00:00:00Z`).
    #[serde(default)]
    pub updated: String,
    /// Where it's used (a site, an API, a registry).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Variable for the username (logins only).
    #[serde(default, rename = "userVar")]
    pub user_var: String,
}

/// The username and the password or token, as the Keychain item holds them. `Debug` never prints
/// them.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultValue {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub secret: String,
}

impl fmt::Debug for VaultValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VaultValue(<hidden>)")
    }
}

/// The form fields an agent's `secret-request` fills in. `None` is a field it didn't pass.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, rename = "how", skip_serializing_if = "Option::is_none")]
    pub how_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl VaultEntry {
    /// A new entry with the default variables for its name and kind.
    pub fn new(name: &str, kind: Kind) -> VaultEntry {
        let (user, secret) = default_vars(name, kind);
        VaultEntry {
            ask: None,
            expires: None,
            how_to: None,
            kind,
            name: name.to_string(),
            note: String::new(),
            secret_var: secret,
            updated: String::new(),
            url: None,
            user_var: if kind == Kind::Login {
                user
            } else {
                String::new()
            },
        }
    }

    /// The name in lower case: names are matched without case.
    pub fn id(&self) -> String {
        self.name.to_lowercase()
    }

    pub fn asks(&self) -> bool {
        self.ask.unwrap_or(false)
    }

    /// The value variables: the username's and the secret's for a login, the secret's for a token.
    pub fn variables(&self) -> Vec<String> {
        match self.kind {
            Kind::Login => vec![self.user_var.clone(), self.secret_var.clone()],
            Kind::Token => vec![self.secret_var.clone()],
        }
    }

    /// `$<NAME>_URL`, set when the entry has a URL.
    pub fn url_var(&self) -> Option<String> {
        match self.url.as_deref() {
            Some(u) if !u.is_empty() => Some(format!("{}_URL", prefix(&self.name))),
            _ => None,
        }
    }

    /// Every variable a command gets, the URL's included.
    pub fn all_variables(&self) -> Vec<String> {
        let mut all = self.variables();
        all.extend(self.url_var());
        all
    }

    /// The URL's host without `www.`.
    pub fn host(&self) -> Option<String> {
        self.url.as_deref().and_then(url::host_of)
    }

    /// This entry under a new name. Variables that were the old name's defaults follow the name;
    /// custom ones stay. The value is not here: the store's rename keeps it.
    pub fn renamed(&self, new_name: &str) -> VaultEntry {
        let mut copy = self.clone();
        copy.name = new_name.to_string();
        let old = default_vars(&self.name, self.kind);
        let new = default_vars(new_name, self.kind);
        if self.secret_var == old.1 {
            copy.secret_var = new.1;
        }
        if self.kind == Kind::Login && self.user_var == old.0 {
            copy.user_var = new.0;
        }
        copy
    }

    /// This saved entry with the fields an agent passed laid over it, for the form to open with.
    /// A field left out keeps its saved value; the name, kind and variables never change here. A
    /// date that isn't `YYYY-MM-DD` is ignored.
    pub fn prefilled(&self, request: &VaultRequest) -> VaultEntry {
        let mut merged = self.clone();
        if let Some(note) = &request.note {
            merged.note = note.clone();
        }
        if let Some(u) = &request.url {
            merged.url = Some(u.clone());
        }
        if let Some(how) = &request.how_to {
            merged.how_to = Some(how.clone());
        }
        if let Some(day) = request.expires.as_deref().and_then(expiry::parse) {
            merged.expires = Some(day);
        }
        merged
    }

    /// A new entry from a request for a name nobody saved yet: its kind (a token unless it says
    /// login), the default variables and the fields it passed.
    pub fn from_request(request: &VaultRequest) -> VaultEntry {
        let kind = request
            .kind
            .as_deref()
            .and_then(Kind::parse)
            .unwrap_or(Kind::Token);
        VaultEntry::new(&request.name, kind).prefilled(request)
    }
}

/// `my-api` → `MY_API`, the prefix of the default variable names.
pub fn prefix(name: &str) -> String {
    let text: String = name
        .to_uppercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    if text.chars().next().is_some_and(|c| c.is_numeric()) {
        format!("_{text}")
    } else {
        text
    }
}

/// The default (username, secret) variables: `<P>_USERNAME`, and `<P>_PASSWORD` for a login or
/// `<P>_TOKEN` for a token.
pub fn default_vars(name: &str, kind: Kind) -> (String, String) {
    let p = prefix(name);
    let secret = match kind {
        Kind::Login => format!("{p}_PASSWORD"),
        Kind::Token => format!("{p}_TOKEN"),
    };
    (format!("{p}_USERNAME"), secret)
}

/// `^[A-Za-z][A-Za-z0-9_.-]{0,63}$`.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// `^[A-Za-z_][A-Za-z0-9_]{0,127}$`.
pub fn valid_var(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.len() <= 128
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A field an agent's request can change in a saved secret's form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Field {
    Note,
    Url,
    HowTo,
    Expires,
}

impl Field {
    /// The badge text, naming the field so each marker sits under its own.
    pub fn marker(self) -> &'static str {
        match self {
            Field::Note => "Note changed by the request",
            Field::Url => "URL changed by the request",
            Field::HowTo => "How-to changed by the request",
            Field::Expires => "Expiry changed by the request",
        }
    }
}

/// The URL's origin before and after, when it differs: that is where the secret is sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostChange {
    pub from: Option<String>,
    pub to: Option<String>,
}

impl HostChange {
    pub fn warning(&self) -> String {
        format!(
            "The request changes where this secret is sent: {} → {}",
            self.from.as_deref().unwrap_or("none"),
            self.to.as_deref().unwrap_or("none")
        )
    }
}

/// What a request changes in a saved secret's form: the saved value of each field it changes
/// (`""` when there was none), and a change of the URL's origin.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefillChanges {
    pub old: BTreeMap<Field, String>,
    #[serde(rename = "hostChange")]
    pub host_change: Option<HostChange>,
}

impl PrefillChanges {
    pub fn is_empty(&self) -> bool {
        self.old.is_empty()
    }

    /// What `request` changes in `saved`'s form. Passing the saved text (give or take spaces)
    /// changes nothing.
    pub fn of(saved: &VaultEntry, request: &VaultRequest) -> PrefillChanges {
        let mut changes = PrefillChanges::default();
        let trimmed = |t: Option<&str>| t.unwrap_or("").trim().to_string();
        if let Some(note) = &request.note {
            if trimmed(Some(note)) != trimmed(Some(&saved.note)) {
                changes.old.insert(Field::Note, saved.note.clone());
            }
        }
        if let Some(u) = &request.url {
            if trimmed(Some(u)) != trimmed(saved.url.as_deref()) {
                changes
                    .old
                    .insert(Field::Url, saved.url.clone().unwrap_or_default());
                let from = saved.url.as_deref().and_then(url::origin_of);
                let to = url::origin_of(u);
                if from != to {
                    changes.host_change = Some(HostChange { from, to });
                }
            }
        }
        if let Some(how) = &request.how_to {
            if trimmed(Some(how)) != trimmed(saved.how_to.as_deref()) {
                changes
                    .old
                    .insert(Field::HowTo, saved.how_to.clone().unwrap_or_default());
            }
        }
        if let Some(day) = request.expires.as_deref().and_then(expiry::parse) {
            if Some(&day) != saved.expires.as_ref() {
                changes
                    .old
                    .insert(Field::Expires, saved.expires.clone().unwrap_or_default());
            }
        }
        changes
    }
}
