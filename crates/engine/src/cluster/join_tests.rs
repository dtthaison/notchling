use super::join::{decode_token, encode_token, JoinToken, Tokens, TOKEN_TTL_MS};
use super::testkit::{fp, id};

fn token() -> JoinToken {
    JoinToken {
        cluster: "c1".to_string(),
        home: id("m5"),
        fingerprint: fp("m5"),
        addrs: vec!["100.64.0.5".to_string()],
        port: 47823,
        secret: "s3cret".to_string(),
    }
}

#[test]
fn token_round_trips() {
    let text = encode_token(&token());
    assert!(text.starts_with("nlj1."));
    assert!(!text.contains('='));
    assert_eq!(decode_token(&text).expect("token"), token());
}

#[test]
fn token_rejects_junk() {
    assert!(decode_token("nlj1.!!!").is_err());
    assert!(decode_token("abc").is_err());
    assert!(decode_token(&format!("nlj1.{}", "A".repeat(5000))).is_err());
    assert!(decode_token("nlj1.e30").is_err());
}

#[test]
fn token_is_one_time_and_expires() {
    let mut tokens = Tokens::default();
    tokens.issue("a".to_string(), 0);
    assert!(tokens.has_open(0));
    assert!(tokens.take("a", 0));
    assert!(!tokens.take("a", 0));
    assert!(!tokens.has_open(1));
    tokens.issue("b".to_string(), 0);
    assert!(tokens.take("b", TOKEN_TTL_MS));
    tokens.issue("c".to_string(), 0);
    assert!(!tokens.take("c", TOKEN_TTL_MS + 1));
    tokens.issue("d".to_string(), 0);
    assert!(!tokens.has_open(TOKEN_TTL_MS + 1));
    assert!(!tokens.take("nope", 0));
}
