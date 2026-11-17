use super::invite::Invite;
use super::join::{Tokens, MAX_TOKENS_PER_MINTER, TOKEN_TTL_MS};
use super::testkit::id;

fn invite(node: &str) -> Invite {
    Invite {
        node: id(node),
        name: node.to_string(),
        can_be_master: false,
        acting: true,
    }
}

#[test]
fn an_invite_is_taken_once_with_its_maker_and_node() {
    let mut tokens = Tokens::default();
    let m5 = id("m5");
    tokens.issue_invite("a".to_string(), m5.clone(), &m5, 0, invite("w1"));
    assert!(tokens.has_open(0));
    let (minter, taken) = tokens.take_open("a", 1).expect("open");
    assert_eq!(minter, m5);
    assert_eq!(taken, Some(invite("w1")));
    assert_eq!(tokens.take_open("a", 1), None);
}

#[test]
fn plain_token_has_no_invite() {
    let mut tokens = Tokens::default();
    let m5 = id("m5");
    tokens.issue("a".to_string(), m5.clone(), &m5, 0);
    assert_eq!(tokens.take_open("a", 1), Some((m5, None)));
}

#[test]
fn expired_invite_is_taken_as_nothing() {
    let mut tokens = Tokens::default();
    let m5 = id("m5");
    tokens.issue_invite("a".to_string(), m5.clone(), &m5, 0, invite("w1"));
    assert_eq!(tokens.take_open("a", TOKEN_TTL_MS + 1), None);
    tokens.issue_invite("b".to_string(), m5.clone(), &m5, 0, invite("w1"));
    assert!(!tokens.has_open(TOKEN_TTL_MS + 1));
}

#[test]
fn fifth_invite_drops_the_oldest() {
    let mut tokens = Tokens::default();
    let m5 = id("m5");
    for n in 0..=MAX_TOKENS_PER_MINTER {
        tokens.issue_invite(
            format!("s{n}"),
            m5.clone(),
            &m5,
            n as i64 * 10,
            invite("w1"),
        );
    }
    assert_eq!(tokens.take_open("s0", 100), None);
    for n in 1..=MAX_TOKENS_PER_MINTER {
        assert!(tokens.take_open(&format!("s{n}"), 100).is_some(), "s{n}");
    }
}

#[test]
fn one_makers_invites_do_not_drop_another_makers() {
    let mut tokens = Tokens::default();
    let (m5, m1) = (id("m5"), id("m1"));
    tokens.issue_invite("keep".to_string(), m1.clone(), &m5, 0, invite("w1"));
    for n in 0..=MAX_TOKENS_PER_MINTER {
        tokens.issue_invite(format!("s{n}"), m5.clone(), &m5, 1 + n as i64, invite("w2"));
    }
    assert!(tokens.take_open("keep", 100).is_some());
}

#[test]
fn restart_forgets_invites() {
    let mut tokens = Tokens::default();
    let m5 = id("m5");
    tokens.issue_invite("a".to_string(), m5.clone(), &m5, 0, invite("w1"));
    let mut restarted = Tokens::default();
    assert!(!restarted.has_open(1));
    assert_eq!(restarted.take_open("a", 1), None);
}

#[test]
fn debug_output_holds_no_secret() {
    let mut tokens = Tokens::default();
    let m5 = id("m5");
    tokens.issue_invite("hush-secret".to_string(), m5.clone(), &m5, 0, invite("w1"));
    tokens.issue("other-secret".to_string(), m5.clone(), &m5, 0);
    let text = format!("{tokens:?}");
    assert!(!text.contains("hush-secret"), "{text}");
    assert!(!text.contains("other-secret"), "{text}");
}
