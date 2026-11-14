use crate::field::{choice_or_other, ErrorCode, Exe, Hex, List, Op, Percent, Seal, Tool, Version, ERROR_CODES};
use crate::field::{Count, FieldValue, LogField};
use crate::field_ref::{hash_ref, Ref, Salt, Session};
use std::path::Path;

log_choice! {
    pub enum Probe {
        First = "first",
        Second = "second",
    }
}

#[test]
fn choice_from_unknown_name_is_other() {
    assert!(Probe::from_name("nope").is_none());
    assert_eq!(choice_or_other::<Probe>("nope").name(), "other");
    assert_eq!(choice_or_other::<Probe>("second").name(), "second");
    assert_eq!(Probe::VALUES.last(), Some(&"other"));
    assert_eq!(Probe::VALUES, &["first", "second", "other"]);
}

#[test]
fn ref_drops_control_characters_and_cuts_at_256() {
    assert_eq!(Ref::<Session>::new("a\nb").as_str(), "a?b");
    let long: String = "é".repeat(300);
    let cut = Ref::<Session>::new(&long);
    assert_eq!(cut.as_str().chars().count(), 256);
    assert!(cut.as_str().chars().all(|c| c == 'é'));
}

#[test]
fn exe_keeps_file_name_and_charset_only() {
    assert_eq!(Exe::from_path(Path::new("/usr/local/bin/gh")).as_str(), "gh");
    assert_eq!(Exe::from_path(Path::new("/tmp/a b/evil;rm")).as_str(), "unknown");
    assert_eq!(Exe::named("a b").as_str(), "unknown");
    assert_eq!(Exe::named(&"x".repeat(65)).as_str(), "unknown");
    assert_eq!(Exe::named(&"x".repeat(64)).as_str().len(), 64);
}

#[test]
fn version_tool_hex_seal_validate() {
    assert_eq!(Version::new("2.1.322").as_str(), "2.1.322");
    assert_eq!(Version::new("1 2").as_str(), "unknown");
    assert_eq!(Version::new(&"1".repeat(33)).as_str(), "unknown");
    assert_eq!(Tool::new("mcp__github__create_issue").as_str(), "mcp__github__create_issue");
    assert_eq!(Tool::new("a b").as_str(), "unknown");
    assert!(Hex::new("ABCD1234").is_none());
    assert!(Hex::new("abcd123").is_none());
    assert_eq!(Hex::new("abcd1234").map(|h| h.as_str().to_string()), Some("abcd1234".to_string()));
    let seal = Seal::from_bytes([0; 32]);
    assert_eq!(seal.as_str().len(), 44);
    assert_eq!(Seal::parse(seal.as_str()).map(|s| s.as_str().to_string()), Some(seal.as_str().to_string()));
    assert!(Seal::parse("short").is_none());
}

#[test]
fn op_and_error_code_parse_against_lists() {
    assert_eq!(Op::parse("vault.use", &["vault.use"]).as_str(), "vault.use");
    assert_eq!(Op::parse("x.y", &[]).as_str(), "unknown");
    assert_eq!(ErrorCode::parse("forbidden").as_str(), "forbidden");
    assert_eq!(ErrorCode::parse("boom").as_str(), "unknown");
    assert_eq!(ERROR_CODES.len(), 7);
}

#[test]
fn percent_clamps_and_list_cuts() {
    assert!(matches!(Percent::new(130).value(), FieldValue::UInt(100)));
    let list = List::new((0..40).map(Count).collect());
    assert_eq!(list.len(), 16);
    assert!(matches!(list.value(), FieldValue::List(v) if v.len() == 16));
}

#[test]
fn hash_ref_is_stable_and_salted() {
    let a = hash_ref(&Salt([1; 32]), "session", "abc");
    assert_eq!(a, hash_ref(&Salt([1; 32]), "session", "abc"));
    assert!(a.starts_with("h:"));
    assert_eq!(a.len(), 10);
    assert!(a[2..].chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
    assert_ne!(a, hash_ref(&Salt([2; 32]), "session", "abc"));
    assert_ne!(a, hash_ref(&Salt([1; 32]), "folder", "abc"));
    let id = Salt([7; 32]).id();
    assert_eq!(id.as_str().len(), 8);
    assert!(id.as_str().chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
}
