//! Made-up Codex logins for tests. No real token, cookie or account is anywhere in them.

use crate::accounts::jwt;
use serde_json::json;

pub fn login(exp: f64) -> Vec<u8> {
    login_for("a@example.com", "user-1", "ws-1", exp, "rt")
}

/// A made-up `auth.json` for this person and workspace; `refresh` stands for the refresh token,
/// so two logins of one account can differ the way a renewed one does.
pub fn login_for(email: &str, user: &str, ws: &str, exp: f64, refresh: &str) -> Vec<u8> {
    let id = jwt::fake(&json!({
        "email": email, "sub": "auth0|x",
        "https://api.openai.com/auth": {
            "chatgpt_account_id": ws, "chatgpt_user_id": user, "chatgpt_plan_type": "plus"}}));
    let access = jwt::fake(&json!({"exp": exp}));
    serde_json::to_vec_pretty(&json!({
        "auth_mode": "chatgpt", "OPENAI_API_KEY": null, "last_refresh": "2026-11-05T07:02:51Z",
        "tokens": {"id_token": id, "access_token": access, "refresh_token": refresh, "account_id": ws}}))
    .unwrap_or_default()
}
