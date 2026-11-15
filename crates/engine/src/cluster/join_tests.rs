use super::join::{decode_token, encode_token, JoinToken, Tokens, MAX_OPEN_TOKENS, TOKEN_TTL_MS};
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
    let m5 = id("m5");
    tokens.issue("a".to_string(), m5.clone(), 0);
    assert!(tokens.has_open(0));
    assert_eq!(tokens.take("a", 0), Some(m5.clone()));
    assert_eq!(tokens.take("a", 0), None);
    assert!(!tokens.has_open(1));
    tokens.issue("b".to_string(), m5.clone(), 0);
    assert_eq!(tokens.take("b", TOKEN_TTL_MS), Some(m5.clone()));
    tokens.issue("c".to_string(), m5.clone(), 0);
    assert_eq!(tokens.take("c", TOKEN_TTL_MS + 1), None);
    tokens.issue("d".to_string(), m5, 0);
    assert!(!tokens.has_open(TOKEN_TTL_MS + 1));
    assert_eq!(tokens.take("nope", 0), None);
}

#[test]
fn a_token_remembers_who_made_it() {
    let mut tokens = Tokens::default();
    tokens.issue("a".to_string(), id("m5"), 0);
    tokens.issue("b".to_string(), id("w1"), 0);
    assert_eq!(tokens.take("b", 1), Some(id("w1")));
    assert_eq!(tokens.take("a", 1), Some(id("m5")));
}

#[test]
fn open_tokens_are_capped_and_the_oldest_goes_first() {
    let mut tokens = Tokens::default();
    let extra = 5;
    for i in 0..(MAX_OPEN_TOKENS + extra) {
        tokens.issue(format!("s{i}"), id("m5"), i as i64);
    }
    for i in 0..extra {
        assert!(
            tokens.take(&format!("s{i}"), 100).is_none(),
            "s{i} should be gone"
        );
    }
    for i in extra..(MAX_OPEN_TOKENS + extra) {
        assert!(
            tokens.take(&format!("s{i}"), 100).is_some(),
            "s{i} should be open"
        );
    }
    assert!(!tokens.has_open(100));
}

#[test]
fn issuing_the_same_secret_again_evicts_nobody() {
    let mut tokens = Tokens::default();
    for i in 0..MAX_OPEN_TOKENS {
        tokens.issue(format!("s{i}"), id("m5"), i as i64);
    }
    tokens.issue("s0".to_string(), id("m5"), 50);
    for i in 0..MAX_OPEN_TOKENS {
        assert!(tokens.take(&format!("s{i}"), 60).is_some(), "s{i}");
    }
}

#[test]
fn a_flood_at_one_instant_still_keeps_only_the_cap() {
    let mut tokens = Tokens::default();
    for i in 0..(MAX_OPEN_TOKENS * 3) {
        tokens.issue(format!("t{i:03}"), id("m5"), 7);
    }
    let open = (0..(MAX_OPEN_TOKENS * 3))
        .filter(|i| tokens.take(&format!("t{i:03}"), 8).is_some())
        .count();
    assert_eq!(open, MAX_OPEN_TOKENS);
}
