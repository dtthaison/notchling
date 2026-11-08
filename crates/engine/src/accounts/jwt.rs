//! The claims of a JWT, read without checking the signature: Codex's id token and Antigravity's
//! name an account, and the access token carries its expiry. Nothing here decides who may do
//! what, so an unverified claim is enough.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::Value;

/// The payload of `header.payload.signature`; `None` for anything that isn't one.
pub fn claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let data = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&data).ok()
}

/// A made-up token with these claims, for tests. The signature part is not a signature.
#[cfg(test)]
pub fn fake(claims: &Value) -> String {
    let body = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap_or_default());
    format!("eyJhbGciOiJSUzI1NiJ9.{body}.sig")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_claims_with_or_without_padding() {
        let token = fake(&json!({"sub": "1234", "email": "a@example.com"}));
        assert_eq!(claims(&token).unwrap()["sub"], "1234");
        let (head, rest) = token.split_once('.').unwrap();
        let (payload, sig) = rest.split_once('.').unwrap();
        let padded = format!("{head}.{payload}==.{sig}");
        assert!(claims(&padded).is_some(), "padding is tolerated");
        assert!(claims("nodots").is_none());
        assert!(claims("a.@@@.c").is_none());
        assert!(claims("a.bm90IGpzb24.c").is_none(), "base64 of 'not json'");
    }
}
