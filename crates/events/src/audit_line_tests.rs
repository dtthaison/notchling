use crate::audit_line::{read_head, seal_of, sealed_line, signed_part, unsealed_line, UnsealedWhy};
use crate::envelope::Envelope;
use crate::event::Event;
use crate::field::{Count, Hex, Seal};
use crate::field_ref::{RefMode, Salt};
use crate::spec::{Outcome, Proc};
use crate::test_kinds::TestKind;

pub(crate) fn envelope(seq: u64) -> Envelope {
    Envelope {
        ts_rx: None,
        seq,
        host: Hex::new("3f9a0c1b7d2e4a56").expect("hex"),
        proc: Proc::Daemon,
        pid: 812,
        boot: Hex::new("b7e1c2d4").expect("hex"),
        op_id: None,
        actor: None,
        repeat: None,
        spooled: false,
    }
}

pub(crate) fn event() -> Event {
    Event::new(
        &TestKind {
            zeta: Count(3),
            alpha: true,
            mid: None,
        },
        Outcome::Ok,
        1_794_560_523_117,
    )
}

pub(crate) fn mode() -> RefMode {
    RefMode::Plain(Salt([1; 32]))
}

#[test]
fn line_shape_matches_the_spec() {
    let key = [9u8; 32];
    let (line, seal) = sealed_line(&key, 7, None, &envelope(5), &event(), &mode());
    assert!(line.starts_with("{\"n\":7,\"v\":1,\"kind\":"), "{line}");
    assert!(line.ends_with(&format!(",\"prev\":null,\"seal\":\"{}\"}}", seal.as_str())));
    assert_eq!(seal.as_str().len(), 44);
    let (second, _) = sealed_line(&key, 8, Some(&seal), &envelope(6), &event(), &mode());
    assert!(second.contains(&format!(",\"prev\":\"{}\",\"seal\":\"", seal.as_str())));
    let parsed: serde_json::Value = serde_json::from_str(&second).expect("valid json");
    assert_eq!(parsed["n"], 8);
}

#[test]
fn seal_covers_bytes_up_to_seal_key() {
    let key = [9u8; 32];
    let (line, seal) = sealed_line(&key, 7, None, &envelope(5), &event(), &mode());
    let at = line.find(",\"seal\":").expect("seal key");
    assert_eq!(seal_of(&key, &line.as_bytes()[..at]), seal);
    assert_eq!(signed_part(&line), Some(&line[..at]));
    assert_ne!(seal_of(&[8u8; 32], &line.as_bytes()[..at]), seal);
}

#[test]
fn unsealed_line_has_null_seal_and_reason() {
    let line = unsealed_line(3, UnsealedWhy::KeyWaiting, &envelope(1), &event(), &mode());
    assert!(line.ends_with(",\"prev\":null,\"unsealed\":\"key_waiting\",\"seal\":null}"));
    let head = read_head(&line).expect("head");
    assert_eq!(head.n, 3);
    assert_eq!(head.seal, None);
    assert_eq!(head.prev, None);
    assert_eq!(head.unsealed, Some(UnsealedWhy::KeyWaiting));
}

#[test]
fn read_head_round_trips() {
    let key = [9u8; 32];
    let (first, first_seal) = sealed_line(&key, 1, None, &envelope(1), &event(), &mode());
    let (second, second_seal) =
        sealed_line(&key, 2, Some(&first_seal), &envelope(2), &event(), &mode());
    let head = read_head(&second).expect("head");
    assert_eq!(head.n, 2);
    assert_eq!(head.kind, "test.kind");
    assert_eq!(head.prev, Some(first_seal));
    assert_eq!(head.seal, Some(second_seal));
    assert_eq!(head.unsealed, None);
    assert_eq!(read_head(&first).expect("head").prev, None);
    assert!(read_head("not json").is_none());
    assert!(Seal::parse("short").is_none());
}
