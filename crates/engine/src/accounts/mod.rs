//! E8: account logins. Claude Code's ([`claude`], E8a): saved logins, the live login, switching
//! and the browser sign-in; lanes (other config folders on other accounts) are [`crate::lanes`].
//! The other providers (E8b) are Codex ([`codex`]), Antigravity ([`antigravity`]) and Claude
//! Desktop ([`desktop`]), listed and switched through [`crate::providers`].
//!
//! Every outside effect sits behind a trait so tests never touch a real login:
//!
//! - the Keychain, through `/usr/bin/security` with the value as hex on stdin (never the Security
//!   framework: an ad hoc signed build would lose access to its own items on every rebuild). There
//!   are two faces over the one process runner, because the two halves need different shapes:
//!   - [`keychain::SecurityRunner`] runs `security` itself (the real one is
//!     [`keychain::SystemSecurity`], the one place that launches it). Claude Code's logins use
//!     [`keychain::Keys`] over it: typed errors, values as [`Secret`], a write read back, and a
//!     value too long for `security -i`'s line refused rather than put on argv.
//!   - [`generic_password::Keychain`] is the plain read/write/delete the E8b stores use, with the
//!     item shapes the Swift app wrote (Codex's base64 copies, Antigravity's go-keyring value). Its
//!     real form, [`generic_password::SecurityTool`], runs through [`keychain::SystemSecurity`] too;
//!     it keeps the Swift app's fallback of putting a too-long value's hex on argv (a known gap,
//!     documented there).
//! - processes and apps are [`procs::Procs`] and [`procs::Apps`];
//! - the network is [`http::HttpClient`] (Claude's profile and token endpoints) and
//!   [`crate::providers::HttpClient`] (the other providers' limits);
//! - the files are paths given by the caller, never found from the environment here.
//!
//! The rules from the Swift notes hold whatever the sources are: a token that is in use (the main
//! login's, a lane's, Codex's or Antigravity's live one) is never renewed, since refresh tokens
//! rotate; an account is live in one place; a switch replaces only `claudeAiOauth` in `Claude
//! Code-credentials` and only `oauthAccount` in `~/.claude.json`. No token reaches an error, a log
//! or a `Debug` print: tokens travel as [`Secret`], and login objects as [`Login`]. Built and
//! switched off: the daemon calls into these only while its `accounts` switch is on.

pub mod antigravity;
pub mod book;
pub mod chunked;
pub mod claude;
pub mod codex;
pub mod desktop;
pub mod fake;
pub mod generic_password;
pub mod http;
pub mod json_splice;
pub mod jwt;
pub mod keychain;
pub mod procs;

use serde_json::{Map, Value};
use std::fmt;

/// A token or other credential. Its `Debug` and `Display` say `[secret]`, so a stray `{:?}` in a
/// log or an error can't print it; [`Secret::expose`] is the one way to read it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Secret {
        Secret(value.into())
    }

    /// The value, for the one place that sends it (a request header, the Keychain's stdin).
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[secret]")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[secret]")
    }
}

/// A JSON object holding tokens (a `claudeAiOauth` login, a saved slot, a Keychain item's whole
/// value). Its `Debug` names only the keys.
#[derive(Clone, Default, PartialEq)]
pub struct Login(pub Map<String, Value>);

impl Login {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(Value::as_str)
    }

    /// A nested object (`claudeAiOauth`, `oauthAccount`).
    pub fn object(&self, key: &str) -> Option<Login> {
        match self.0.get(key) {
            Some(Value::Object(map)) => Some(Login(map.clone())),
            _ => None,
        }
    }

    pub fn into_value(self) -> Value {
        Value::Object(self.0)
    }
}

impl fmt::Debug for Login {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let keys: Vec<&String> = self.0.keys().collect();
        write!(f, "Login{keys:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn secrets_and_logins_never_print_their_values() {
        let secret = Secret::new("test-access-token");
        assert_eq!(format!("{secret:?} {secret}"), "[secret] [secret]");
        assert_eq!(secret.expose(), "test-access-token");
        let Value::Object(map) = json!({"accessToken": "test-access-token", "scopes": []}) else {
            unreachable!()
        };
        let login = Login(map);
        let printed = format!("{login:?}");
        assert!(!printed.contains("test-access-token"), "{printed}");
        assert!(printed.contains("accessToken"));
    }
}
