use serde_json::json;

use super::members::Term;
use super::testkit::{id, versions};
use super::wire::{decode, encode, Msg, Origin, OriginTier, MAX_LINE};

fn term(n: u64, who: &str) -> Term {
    Term { n, node: id(who) }
}

#[test]
fn encodes_one_line_and_round_trips() {
    let msg = Msg::Call {
        id: 7,
        op: "session.new".to_string(),
        args: json!({"cwd": "/tmp"}),
        origin: Origin {
            node: id("m5"),
            tier: OriginTier::Node,
        },
        term: term(1, "m5"),
    };
    let line = encode(&msg).expect("line");
    assert_eq!(line.iter().filter(|b| **b == b'\n').count(), 1);
    assert_eq!(line.last(), Some(&b'\n'));
    assert_eq!(decode(&line).expect("msg"), msg);
}

#[test]
fn unknown_type_decodes_as_unknown() {
    let msg = decode(br#"{"type":"from_the_future","x":1}"#).expect("msg");
    assert_eq!(msg, Msg::Unknown);
}

#[test]
fn unknown_fields_are_ignored() {
    let line = br#"{"type":"ping","boot_ms":5,"extra":true}"#;
    assert_eq!(decode(line).expect("msg"), Msg::Ping { boot_ms: 5 });
}

#[test]
fn line_over_1mib_is_refused() {
    let msg = Msg::Event {
        topic: "t".to_string(),
        name: "n".to_string(),
        data: json!("x".repeat(MAX_LINE)),
    };
    assert!(encode(&msg).is_err());
    assert!(decode(&vec![b' '; MAX_LINE + 1]).is_err());
}

#[test]
fn bad_json_is_an_error() {
    assert!(decode(b"not json").is_err());
    assert!(decode(br#"{"no_type":1}"#).is_err());
}

#[test]
fn call_and_hello_carry_term() {
    let hello = Msg::Hello {
        node: id("w1"),
        cluster: Some("c1".to_string()),
        versions: versions("linux"),
        boot_id: "b".to_string(),
        term: Some(term(3, "m1")),
    };
    let text = String::from_utf8(encode(&hello).expect("line")).expect("utf8");
    assert!(text.contains("\"term\":{\"n\":3"));
    assert_eq!(decode(text.as_bytes()).expect("msg"), hello);
    let call = Msg::Call {
        id: 1,
        op: "node.load".to_string(),
        args: json!({}),
        origin: Origin {
            node: id("w1"),
            tier: OriginTier::Node,
        },
        term: term(3, "m1"),
    };
    let text = String::from_utf8(encode(&call).expect("line")).expect("utf8");
    assert!(text.contains("\"term\""));
}

#[test]
fn hello_without_a_term_still_decodes() {
    let hello = Msg::Hello {
        node: id("w1"),
        cluster: None,
        versions: versions("linux"),
        boot_id: "b".to_string(),
        term: None,
    };
    let mut value: serde_json::Value =
        serde_json::from_slice(&encode(&hello).expect("line")).expect("json");
    value.as_object_mut().expect("object").remove("term");
    value.as_object_mut().expect("object").remove("cluster");
    let again: Msg = serde_json::from_value(value).expect("msg");
    assert_eq!(again, hello);
}

#[test]
fn terms_order_by_n_then_node() {
    let a = term(2, "m5");
    let b = term(3, "m1");
    assert!(a < b);
    let (x, y) = (term(3, "m1"), term(3, "m5"));
    assert_eq!(x < y, id("m1") < id("m5"));
}
