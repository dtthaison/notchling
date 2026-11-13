use crate::accounts::jwt;
use serde_json::json;

pub fn login(exp: f64) -> Vec<u8> {
    login_for("a@example.com", "user-1", "ws-1", exp, "rt")
}

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

pub fn big_login_for(user: &str, ws: &str, refresh: &str, size: usize) -> Vec<u8> {
    let filler: String = "FAKE-claim-".chars().cycle().take(size).collect();
    let id = jwt::fake(&json!({
        "email": format!("{user}@example.com"), "sub": "auth0|x", "fake_padding": filler,
        "https://api.openai.com/auth": {
            "chatgpt_account_id": ws, "chatgpt_user_id": user, "chatgpt_plan_type": "pro"}}));
    let access = jwt::fake(&json!({"exp": 9.0e9}));
    serde_json::to_vec_pretty(&json!({
        "auth_mode": "chatgpt", "OPENAI_API_KEY": null, "last_refresh": "2026-11-05T07:02:51Z",
        "tokens": {"id_token": id, "access_token": access, "refresh_token": refresh, "account_id": ws}}))
    .unwrap_or_default()
}
