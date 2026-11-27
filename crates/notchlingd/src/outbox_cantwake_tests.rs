use crate::acting_ops_tests::app;
use crate::op_group::OpGroup;
use crate::outbox_testkit::{rig_with, Options, Rig, IDLE, T0};
use notchling_engine::outbox::{State, Why};
use serde_json::json;

const ELSEWHERE: &[&str] = &[
    "Waking session 5de74ffd…",
    "Couldn't wake 5de74ffd — Can't open — this session is running in another terminal",
];

const REASON: &str = "Can't open — this session is running in another terminal";

#[test]
fn a_session_that_cannot_wake_is_tried_again_and_the_chat_row_says_why() {
    let r = rig_with(Options {
        screen: ELSEWHERE,
        ..Options::default()
    });
    let row = r.send("job-idle", "hi");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    let now = r.row(&row.id).expect("the message is kept");
    assert_eq!((now.state, now.why), (State::Waiting, Some(Why::Retry)));
    assert_eq!(
        r.typed(),
        "",
        "nothing was typed into a session that is not there"
    );
    let wire = r
        .room
        .named("outbox.changed")
        .into_iter()
        .rev()
        .find(|event| event["row"]["id"] == row.id.as_str())
        .expect("the row was published");
    assert_eq!(wire["row"]["why"], "wake");
    assert_eq!(wire["row"]["error"], REASON);
}

fn listed(r: &Rig, id: &str) -> serde_json::Value {
    let list = r
        .ops
        .call(&app(), "outbox.list", &json!({"session": "job-idle"}))
        .unwrap();
    list.as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == json!(id))
        .expect("the row is listed")
        .clone()
}

#[test]
fn the_reason_stays_shown_through_the_wait_until_the_next_try() {
    let r = rig_with(Options {
        screen: ELSEWHERE,
        ..Options::default()
    });
    let row = r.send("job-idle", "hi");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(listed(&r, &row.id)["why"], "wake");
    for later in [1_000, 2_000, 5_000, 20_000] {
        p.pass(r.at(T0 + later));
        let wire = listed(&r, &row.id);
        assert_eq!(wire["why"], "wake", "{later}ms in");
        assert_eq!(wire["error"], REASON, "{later}ms in");
    }
    let now = r.row(&row.id).expect("the message is kept");
    assert_eq!((now.state, now.why), (State::Waiting, Some(Why::Retry)));
    assert_eq!(r.typed(), "");
}
