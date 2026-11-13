use crate::accounts::book::Saved;
use crate::accounts::jwt;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::Value;

pub const KEYRING_PREFIX: &str = "go-keyring-base64:";

pub const EMAIL_ID_PREFIX: &str = "email:";

pub fn encode_keyring(login: &str) -> String {
    format!("{KEYRING_PREFIX}{}", STANDARD.encode(login.as_bytes()))
}

pub fn decode_keyring(value: &str) -> Option<String> {
    let data = STANDARD.decode(value.strip_prefix(KEYRING_PREFIX)?).ok()?;
    String::from_utf8(data).ok()
}

pub fn has_id_token(login: &str) -> bool {
    serde_json::from_str::<Value>(login)
        .ok()
        .and_then(|v| v.get("id_token").map(Value::is_string))
        .unwrap_or(false)
}

pub fn account_in(login: &str) -> Option<Saved> {
    let json: Value = serde_json::from_str(login).ok()?;
    let Some(token) = json.get("id_token").and_then(Value::as_str) else {
        let email = json
            .get("id_token_email")
            .and_then(Value::as_str)
            .filter(|e| !e.is_empty())?;
        return Some(Saved {
            id: format!("{EMAIL_ID_PREFIX}{}", email.to_lowercase()),
            email: email.to_string(),
            plan: None,
        });
    };
    let claims = jwt::claims(token)?;
    let sub = claims.get("sub").and_then(Value::as_str)?;
    Some(Saved {
        id: sub.to_string(),
        email: claims
            .get("email")
            .and_then(Value::as_str)
            .unwrap_or("Google Account")
            .to_string(),
        plan: None,
    })
}

#[cfg(test)]
pub fn fake_login(sub: &str, email: &str, access: &str) -> String {
    serde_json::json!({
        "auth_method": "oauth",
        "id_token": jwt::fake(&serde_json::json!({"sub": sub, "email": email})),
        "token": {"access_token": access},
    })
    .to_string()
}

#[cfg(test)]
pub fn fake_email_login(email: &str, access: &str) -> String {
    serde_json::json!({
        "token": {"access_token": access, "refresh_token": "r"},
        "auth_method": "keyring",
        "id_token_email": email,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_token_login_is_the_sub() {
        let a = account_in(&fake_login("1234", "a@example.com", "t")).unwrap();
        assert_eq!((a.id.as_str(), a.email.as_str()), ("1234", "a@example.com"));
    }

    #[test]
    fn the_newer_format_names_only_the_email() {
        let a = account_in(&fake_email_login("B@example.com", "t")).unwrap();
        assert_eq!(a.id, format!("{EMAIL_ID_PREFIX}b@example.com"));
        assert_eq!(a.email, "B@example.com");
    }

    #[test]
    fn an_unknown_login_is_nothing() {
        assert!(account_in(r#"{"token":{}}"#).is_none());
        assert!(account_in("not json").is_none());
        assert!(account_in(r#"{"id_token_email":""}"#).is_none());
        assert!(account_in(r#"{"id_token":"x.y.z"}"#).is_none());
    }

    #[test]
    fn go_keyring_round_trips_and_refuses_other_values() {
        let value = encode_keyring(r#"{"a":1}"#);
        assert!(value.starts_with("go-keyring-base64:"));
        assert_eq!(decode_keyring(&value).as_deref(), Some(r#"{"a":1}"#));
        assert_eq!(decode_keyring("plain"), None);
        assert_eq!(decode_keyring("go-keyring-base64:@@@"), None);
    }

    #[test]
    fn only_the_older_format_has_an_id_token_for_the_mirror() {
        assert!(has_id_token(&fake_login("1", "a@x.com", "t")));
        assert!(!has_id_token(&fake_email_login("a@x.com", "t")));
        assert!(!has_id_token("nope"));
    }
}
