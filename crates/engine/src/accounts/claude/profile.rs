//! The pure parts of `AccountStore`: which account a profile or `oauthAccount` block names, the
//! `oauthAccount` Claude Code would write for a profile, the plan's name, and whether a login has
//! expired.

use crate::accounts::Login;
use serde_json::{json, Map, Value};

/// `<accountUuid>:<organizationUuid>`: one person can be in several organizations.
pub fn account_id_from_config(config: &Login) -> Option<String> {
    let a = config.str("accountUuid")?;
    let o = config.str("organizationUuid")?;
    Some(format!("{a}:{o}"))
}

/// The id from `GET /api/oauth/profile`'s `account.uuid` and `organization.uuid`.
pub fn account_id_from_profile(profile: &Login) -> Option<String> {
    let a = profile.object("account")?;
    let o = profile.object("organization")?;
    Some(format!("{}:{}", a.str("uuid")?, o.str("uuid")?))
}

/// Whether `id` is shaped like an account id: two UUID-ish parts (hex digits and dashes) around
/// one colon. Anything else from a caller is refused before it names a Keychain item.
pub fn valid_id(id: &str) -> bool {
    let part = |p: &str| {
        !p.is_empty() && p.len() <= 64 && p.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
    };
    matches!(id.split_once(':'), Some((a, o)) if part(a) && part(o))
}

/// The `oauthAccount` block Claude Code would write for this profile.
pub fn oauth_account_from_profile(profile: &Login) -> Value {
    let a = profile.object("account").unwrap_or_default();
    let o = profile.object("organization").unwrap_or_default();
    let or_null = |m: &Login, k: &str| m.get(k).cloned().unwrap_or(Value::Null);
    let mut out = Map::new();
    out.insert(
        "accountUuid".into(),
        a.get("uuid").cloned().unwrap_or(json!("")),
    );
    out.insert(
        "emailAddress".into(),
        a.get("email").cloned().unwrap_or(json!("")),
    );
    out.insert(
        "organizationUuid".into(),
        o.get("uuid").cloned().unwrap_or(json!("")),
    );
    out.insert(
        "organizationName".into(),
        o.get("name").cloned().unwrap_or(json!("")),
    );
    out.insert("organizationType".into(), or_null(&o, "organization_type"));
    out.insert("billingType".into(), or_null(&o, "billing_type"));
    out.insert(
        "organizationRateLimitTier".into(),
        or_null(&o, "rate_limit_tier"),
    );
    out.insert(
        "hasExtraUsageEnabled".into(),
        o.get("has_extra_usage_enabled")
            .cloned()
            .unwrap_or(json!(false)),
    );
    if let Some(name) = a.get("display_name") {
        out.insert("displayName".into(), name.clone());
    }
    if let Some(name) = a.get("full_name") {
        out.insert("fullName".into(), name.clone());
    }
    Value::Object(out)
}

/// "Max 5x", "Pro", "Team"… from the organization's type and rate limit tier.
pub fn plan_name(account: &Login, oauth: &Login) -> String {
    let kind = account
        .str("organizationType")
        .or_else(|| oauth.str("subscriptionType"))
        .unwrap_or("")
        .replace("claude_", "");
    let tier = account
        .str("organizationRateLimitTier")
        .or_else(|| oauth.str("rateLimitTier"))
        .unwrap_or("");
    let mut name = if kind.is_empty() {
        "Claude".to_string()
    } else {
        let mut chars = kind.chars();
        chars
            .next()
            .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
            .unwrap_or_default()
    };
    if let Some(mult) = multiplier(tier) {
        name = format!("{name} {mult}");
    }
    name
}

/// The `\d+x` at the end of a tier (`default_claude_max_20x` → `20x`).
fn multiplier(tier: &str) -> Option<&str> {
    let body = tier.strip_suffix('x')?;
    let digits = body.len() - body.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    (digits > 0).then(|| &tier[tier.len() - digits - 1..])
}

/// Expired, or within a minute of it (`expiresAt` is milliseconds). A login without the field
/// counts as live.
pub fn is_expired(oauth: &Login, now_ms: i64) -> bool {
    match oauth.get("expiresAt").and_then(Value::as_f64) {
        Some(ms) => ms / 1000.0 < now_ms as f64 / 1000.0 + 60.0,
        None => false,
    }
}

/// The access token, when there is a non-empty one.
pub fn access_token(oauth: &Login) -> Option<crate::accounts::Secret> {
    oauth
        .str("accessToken")
        .filter(|t| !t.is_empty())
        .map(crate::accounts::Secret::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login(v: Value) -> Login {
        match v {
            Value::Object(m) => Login(m),
            _ => Login::default(),
        }
    }

    #[test]
    fn ids_come_from_the_config_or_the_profile() {
        let config = login(json!({"accountUuid": "a1", "organizationUuid": "o1"}));
        assert_eq!(account_id_from_config(&config).as_deref(), Some("a1:o1"));
        let profile = login(json!({"account": {"uuid": "a2"}, "organization": {"uuid": "o2"}}));
        assert_eq!(account_id_from_profile(&profile).as_deref(), Some("a2:o2"));
        assert_eq!(
            account_id_from_profile(&login(json!({"account": {}}))),
            None
        );
    }

    #[test]
    fn only_uuid_shaped_ids_are_valid() {
        assert!(valid_id("0f3c-a1:9e-ff"));
        for bad in ["", "a", "a:", ":b", "a:b:c", "a\":b", "../x:y", "a b:c"] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn the_oauth_account_is_claude_codes_shape() {
        let profile = login(json!({
            "account": {"uuid": "a", "email": "me@example.com", "display_name": "Me"},
            "organization": {"uuid": "o", "name": "Org", "organization_type": "claude_max",
                             "rate_limit_tier": "default_claude_max_20x"}
        }));
        let out = oauth_account_from_profile(&profile);
        assert_eq!(out["accountUuid"], "a");
        assert_eq!(out["emailAddress"], "me@example.com");
        assert_eq!(out["organizationType"], "claude_max");
        assert_eq!(out["billingType"], Value::Null);
        assert_eq!(out["hasExtraUsageEnabled"], false);
        assert_eq!(out["displayName"], "Me");
        assert!(out.get("fullName").is_none());
        assert_eq!(plan_name(&login(out), &Login::default()), "Max 20x");
    }

    #[test]
    fn plan_names() {
        let none = Login::default();
        assert_eq!(plan_name(&none, &none), "Claude");
        assert_eq!(
            plan_name(&login(json!({"organizationType": "claude_pro"})), &none),
            "Pro"
        );
        let oauth = login(json!({"subscriptionType": "team", "rateLimitTier": "tier_5x"}));
        assert_eq!(plan_name(&none, &oauth), "Team 5x");
        let odd = login(json!({"organizationType": "max", "organizationRateLimitTier": "x"}));
        assert_eq!(plan_name(&odd, &none), "Max");
    }

    #[test]
    fn expiry_has_a_minute_of_margin() {
        let now = 1_000_000_000_000;
        assert!(!is_expired(&Login::default(), now));
        assert!(is_expired(&login(json!({"expiresAt": now + 30_000})), now));
        assert!(!is_expired(
            &login(json!({"expiresAt": now + 120_000})),
            now
        ));
        assert!(is_expired(&login(json!({"expiresAt": now - 1})), now));
    }
}
