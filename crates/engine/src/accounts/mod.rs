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

#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Secret {
        Secret(value.into())
    }

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

#[derive(Clone, Default, PartialEq)]
pub struct Login(pub Map<String, Value>);

impl Login {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(Value::as_str)
    }

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
