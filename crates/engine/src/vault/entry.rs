use super::expiry;
use super::url;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

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

    pub fn parse(text: &str) -> Option<Kind> {
        match text {
            "login" => Some(Kind::Login),
            "token" => Some(Kind::Token),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    #[serde(default, rename = "howTo", skip_serializing_if = "Option::is_none")]
    pub how_to: Option<String>,
    pub kind: Kind,
    pub name: String,
    #[serde(default)]
    pub note: String,
    #[serde(default, rename = "secretVar")]
    pub secret_var: String,
    #[serde(default)]
    pub updated: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, rename = "userVar")]
    pub user_var: String,
}

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

    pub fn id(&self) -> String {
        self.name.to_lowercase()
    }

    pub fn asks(&self) -> bool {
        self.ask.unwrap_or(false)
    }

    pub fn variables(&self) -> Vec<String> {
        match self.kind {
            Kind::Login => vec![self.user_var.clone(), self.secret_var.clone()],
            Kind::Token => vec![self.secret_var.clone()],
        }
    }

    pub fn url_var(&self) -> Option<String> {
        match self.url.as_deref() {
            Some(u) if !u.is_empty() => Some(format!("{}_URL", prefix(&self.name))),
            _ => None,
        }
    }

    pub fn all_variables(&self) -> Vec<String> {
        let mut all = self.variables();
        all.extend(self.url_var());
        all
    }

    pub fn host(&self) -> Option<String> {
        self.url.as_deref().and_then(url::host_of)
    }

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

    pub fn from_request(request: &VaultRequest) -> VaultEntry {
        let kind = request
            .kind
            .as_deref()
            .and_then(Kind::parse)
            .unwrap_or(Kind::Token);
        VaultEntry::new(&request.name, kind).prefilled(request)
    }
}

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

pub fn default_vars(name: &str, kind: Kind) -> (String, String) {
    let p = prefix(name);
    let secret = match kind {
        Kind::Login => format!("{p}_PASSWORD"),
        Kind::Token => format!("{p}_TOKEN"),
    };
    (format!("{p}_USERNAME"), secret)
}

pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

pub fn valid_var(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.len() <= 128
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Field {
    Note,
    Url,
    HowTo,
    Expires,
}

impl Field {
    pub fn marker(self) -> &'static str {
        match self {
            Field::Note => "Note changed by the request",
            Field::Url => "URL changed by the request",
            Field::HowTo => "How-to changed by the request",
            Field::Expires => "Expiry changed by the request",
        }
    }
}

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
