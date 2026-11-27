use super::*;
use crate::accounts::fake::ScriptedHttp;

fn sign_in() -> SignIn {
    let mut random = [0u8; 64];
    for (i, b) in random.iter_mut().enumerate() {
        *b = i as u8;
    }
    SignIn::new(random, 5123)
}

#[test]
fn the_challenge_is_rfc_7636s() {
    assert_eq!(
        challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn the_page_asks_for_claude_codes_login() {
    let s = sign_in();
    let url = s.page_url();
    assert!(url.starts_with("https://claude.com/cai/oauth/authorize?code=true&client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e&response_type=code&"));
    assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A5123%2Fcallback"));
    assert!(url.contains("scope=org%3Acreate_api_key%20user%3Aprofile%20"));
    assert!(url.contains(&format!(
        "code_challenge={}",
        challenge(&base64url(&(0u8..32).collect::<Vec<_>>()))
    )));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains(&format!(
        "state={}",
        base64url(&(32u8..64).collect::<Vec<_>>())
    )));
    assert_eq!(format!("{s:?}"), "SignIn { port: 5123 }");
}

#[test]
fn only_our_callback_with_our_state_carries_a_code() {
    let s = sign_in();
    let state = base64url(&(32u8..64).collect::<Vec<_>>());
    let ok = format!("GET /callback?code=test%2Dcode&state={state} HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(s.callback(&ok), Callback::Code(Secret::new("test-code")));
    for other in [
        "GET /callback?code=c&state=wrong HTTP/1.1".to_string(),
        format!("GET /callback?state={state} HTTP/1.1"),
        format!("GET /callback?code=&state={state} HTTP/1.1"),
        format!("GET /favicon.ico?code=c&state={state} HTTP/1.1"),
        "junk".to_string(),
    ] {
        assert_eq!(s.callback(&other), Callback::NotOurs, "{other}");
    }
    assert!(reply(true)
        .contains("Location: https://platform.claude.com/oauth/code/success?app=claude-code"));
    assert!(reply(false).starts_with("HTTP/1.1 404"));
}

#[test]
fn the_exchange_turns_the_answer_into_claude_codes_login() {
    let s = sign_in();
    let http = ScriptedHttp::new();
    http.answer(
        TOKEN_URL,
        200,
        r#"{"access_token":"test-access","refresh_token":"test-refresh","expires_in":100,"scope":"a b","refresh_token_expires_in":1000}"#,
    );
    let login = s
        .exchange(&http, &Secret::new("test-code"), 1_000_000)
        .unwrap();
    assert_eq!(login.str("accessToken"), Some("test-access"));
    assert_eq!(login.str("refreshToken"), Some("test-refresh"));
    assert_eq!(login.get("expiresAt"), Some(&json!(1_100_000)));
    assert_eq!(login.get("refreshTokenExpiresAt"), Some(&json!(2_000_000)));
    assert_eq!(login.get("scopes"), Some(&json!(["a", "b"])));
    let body = s.exchange_body(&Secret::new("test-code"));
    assert_eq!(body["grant_type"], "authorization_code");
    assert_eq!(body["redirect_uri"], "http://localhost:5123/callback");
    assert_eq!(
        body["code_verifier"],
        base64url(&(0u8..32).collect::<Vec<_>>())
    );
}

#[test]
fn a_refused_or_missing_answer_says_so_without_tokens() {
    let s = sign_in();
    let http = ScriptedHttp::new();
    http.answer(TOKEN_URL, 400, r#"{"error":"invalid_grant"}"#);
    let refused = s.exchange(&http, &Secret::new("test-code"), 0).unwrap_err();
    assert_eq!(refused, "Sign-in was refused (400). Try again.");
    let none = s.exchange(&http, &Secret::new("test-code"), 0).unwrap_err();
    assert!(none.contains("Couldn't reach"));
    http.answer(TOKEN_URL, 200, r#"{"access_token":"test-only-access"}"#);
    let half = s.exchange(&http, &Secret::new("test-code"), 0).unwrap_err();
    assert!(!half.contains("test-only-access"));
}
