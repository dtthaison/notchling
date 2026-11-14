use crate::envelope::{write_body, Actor, ActorTier, Envelope, Via};
use crate::event::{Event, Kind, MAX_EVENT_BYTES};
use crate::field::{Count, Exe, FieldValue, Hex, List, Millis};
use crate::field_ref::{Ref, RefMode, Salt, Secret};
use crate::spec::{Outcome, Proc};
use crate::test_kinds::{TestKind, TestNames, TestRefs};
use crate::value::Fields;

fn json(fields: &Fields, mode: &RefMode) -> String {
    let mut out = String::new();
    fields.write_json(&mut out, mode);
    out
}

fn plain() -> RefMode {
    RefMode::Plain(Salt([1; 32]))
}

fn refs(session: &str, item: &str) -> TestRefs {
    TestRefs {
        session: Ref::new(session),
        item: Ref::new(item),
        exe: None,
        waited: None,
    }
}

#[test]
fn fields_write_in_name_order() {
    let kind = TestKind {
        zeta: Count(3),
        alpha: true,
        mid: None,
    };
    assert_eq!(json(&kind.fields(), &plain()), r#"{"alpha":true,"zeta":3}"#);
    let kind = TestKind {
        zeta: Count(3),
        alpha: false,
        mid: Some(Count(9)),
    };
    assert_eq!(
        json(&kind.fields(), &plain()),
        r#"{"alpha":false,"mid":9,"zeta":3}"#
    );
}

#[test]
fn output_is_valid_json_even_for_hostile_text() {
    let kind = refs("a\"b\\c\u{7}é", "x");
    let text = json(&kind.fields(), &plain());
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(parsed["session"], "a\"b\\c?é");
}

#[test]
fn refs_follow_the_mode() {
    let kind = refs("abc", "ticket-1");
    let salt = Salt([1; 32]);
    let plain = json(&kind.fields(), &RefMode::Plain(salt));
    let hashed = json(&kind.fields(), &RefMode::Hashed(salt));
    let plain: serde_json::Value = serde_json::from_str(&plain).expect("json");
    let hashed: serde_json::Value = serde_json::from_str(&hashed).expect("json");
    assert_eq!(plain["session"], "abc");
    let h = hashed["session"].as_str().expect("string");
    assert!(h.starts_with("h:") && h.len() == 10);
    let item = plain["item"].as_str().expect("string");
    assert!(item.starts_with("h:"));
    assert_eq!(hashed["item"], plain["item"]);
}

fn envelope(op_id: Option<Hex>, actor: Option<Actor>, spooled: bool) -> Envelope {
    Envelope {
        ts_rx: None,
        seq: 7,
        host: Hex::new("0123456789abcdef").expect("hex"),
        proc: Proc::Daemon,
        pid: 42,
        boot: Hex::new("deadbeef").expect("hex"),
        op_id,
        actor,
        repeat: None,
        spooled,
    }
}

fn body(env: &Envelope, event: &Event) -> String {
    let mut out = String::new();
    write_body(env, event, &plain(), &mut out);
    out
}

#[test]
fn body_has_the_fixed_key_order() {
    let actor = Actor {
        tier: ActorTier::Agent,
        via: Some(Via::Cli),
        exe: Some(Exe::named("notchling")),
        device: None,
        session: Some(Ref::new("s1")),
    };
    let event = Event::new(
        &TestKind {
            zeta: Count(1),
            alpha: true,
            mid: None,
        },
        Outcome::Ok,
        1000,
    );
    let text = body(&envelope(None, Some(actor), false), &event);
    assert!(text.starts_with(r#""v":1,"kind":"test.kind","kv":1,"ts":1000,"seq":7,"host":"#));
    assert!(!text.contains("op_id"));
    assert!(!text.contains("spooled"));
    assert!(!text.contains("ts_rx"));
    let actor_at = text
        .find(r#""actor":{"tier":"agent","via":"cli","exe":"notchling","session":"s1"}"#)
        .expect("actor");
    assert!(actor_at < text.find(r#""outcome":"ok""#).expect("outcome"));
    assert!(text.ends_with(r#""f":{"alpha":true,"zeta":1}"#));
    let parsed: serde_json::Value = serde_json::from_str(&format!("{{{text}}}")).expect("json");
    assert_eq!(parsed["actor"]["session"], "s1");
}

#[test]
fn body_writes_every_optional_part_in_order() {
    let mut env = envelope(Some(Hex::new("00112233aabbccdd").expect("hex")), None, true);
    env.ts_rx = Some(1001);
    env.repeat = Some(3);
    let event = Event::new(&refs("s", "i"), Outcome::Refused, 1000);
    let text = body(&env, &event);
    let order = [
        "\"v\"",
        "\"kind\"",
        "\"kv\"",
        "\"ts\"",
        "\"ts_rx\"",
        "\"seq\"",
        "\"host\"",
        "\"proc\"",
        "\"pid\"",
        "\"boot\"",
        "\"op_id\"",
        "\"outcome\"",
        "\"repeat\"",
        "\"spooled\"",
        "\"f\"",
    ];
    let mut last = 0;
    for key in order {
        let at = text.find(key).unwrap_or_else(|| panic!("{key} in {text}"));
        assert!(at >= last, "{key} out of order in {text}");
        last = at;
    }
    serde_json::from_str::<serde_json::Value>(&format!("{{{text}}}")).expect("json");
}

fn names(count: usize, chars: usize) -> TestNames {
    TestNames {
        names: List::new(
            (0..count)
                .map(|i| Ref::<Secret>::new(&format!("{i}{}", "n".repeat(chars - 1))))
                .collect(),
        ),
    }
}

#[test]
fn list_is_cut_at_16_and_event_under_4k() {
    let event = Event::new(&names(40, 200), Outcome::Ok, 1);
    let Some(FieldValue::List(items)) = event.fields.get("names") else {
        panic!("names is a list");
    };
    assert_eq!(items.len(), 16);
    assert!(event.json_len() <= MAX_EVENT_BYTES);
}

#[test]
fn oversized_lists_shrink_refs_then_drop_items() {
    let event = Event::new(&names(16, 256), Outcome::Ok, 1);
    assert!(event.json_len() <= MAX_EVENT_BYTES);
    let Some(FieldValue::List(items)) = event.fields.get("names") else {
        panic!("names is a list");
    };
    assert_eq!(items.len(), 16);
    assert!(matches!(&items[0], FieldValue::Ref { id, .. } if id.chars().count() == 64));
}

#[test]
fn unset_options_are_left_out_and_set_ones_kept() {
    let mut kind = refs("s", "i");
    kind.exe = Some(Exe::named("gh"));
    kind.waited = Some(Millis(5));
    let text = json(&kind.fields(), &plain());
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(parsed["exe"], "gh");
    assert_eq!(parsed["waited"], 5);
    assert!(json(&refs("s", "i").fields(), &plain())
        .find("exe")
        .is_none());
}
