use crate::acting_ops_tests::app;
use crate::op_group::OpGroup;
use crate::outbox_testkit::{rig, Rig, IDLE, T0};
use notchling_engine::outbox::{State, DONE_STOPS_AFTER_TRIES, UNREADABLE_AFTER_MS};
use notchling_engine::sessions::fresh::Ended;
use serde_json::{json, Value};
use std::sync::Arc;

fn call(r: &Rig, op: &str, args: Value) -> Value {
    r.ops.call(&app(), op, &args).unwrap()
}

fn shown(r: &Rig, id: &str) -> Option<String> {
    let list = call(r, "outbox.list", json!({"session": "job-idle"}));
    let wire = list
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == json!(id))?
        .clone();
    wire["why"].as_str().map(String::from)
}

fn done_with_no_process(r: &Rig) {
    r.files.vanish(IDLE);
    r.files.set_state(IDLE, "done");
}

fn due(r: &Rig, id: &str) -> i64 {
    r.row(id).unwrap().next_ms.unwrap()
}

#[test]
fn a_done_job_keeps_its_failed_tries_across_a_daemon_restart() {
    let r = rig();
    let row = r.send("job-idle", "hi");
    done_with_no_process(&r);
    let mut p = r.postmen.postman(IDLE);
    let mut at = T0;
    for _ in 1..DONE_STOPS_AFTER_TRIES {
        p.pass(r.at(at));
        at = due(&r, &row.id);
    }
    assert_eq!(r.row(&row.id).unwrap().tries, DONE_STOPS_AFTER_TRIES - 1);
    let restarted = r.restarted();
    let mut p = restarted.postman(IDLE);
    p.pass(r.at(at));
    assert_eq!(r.row(&row.id).unwrap().state, State::Waiting);
    p.pass(r.at(due(&r, &row.id)));
    let stopped = r.row(&row.id).unwrap();
    assert_eq!(stopped.state, State::Stopped);
    assert_eq!(stopped.tries, DONE_STOPS_AFTER_TRIES);
}

#[test]
fn a_done_job_stops_after_the_tries_on_it_fail_and_resume_and_send_tries_it_again() {
    let r = rig();
    let row = r.send("job-idle", "hi");
    r.ops.store().run_sql_for_tests(&format!(
        "UPDATE outbox SET tries = 5 WHERE id = '{}'",
        row.id
    ));
    done_with_no_process(&r);
    let mut p = r.postmen.postman(IDLE);
    let mut at = T0;
    for n in 1..=DONE_STOPS_AFTER_TRIES {
        p.pass(r.at(at));
        let tried = r.row(&row.id).unwrap();
        assert_eq!(
            (tried.state, tried.tries, tried.done_from),
            (State::Waiting, 5 + n, Some(5))
        );
        at = due(&r, &row.id);
    }
    p.pass(r.at(at));
    assert_eq!(r.row(&row.id).unwrap().state, State::Stopped);
    r.ops.set_resume(Arc::new(|_| Ok(())));
    call(&r, "outbox.resend", json!({"id": row.id, "resume": true}));
    let resumed = r.row(&row.id).unwrap();
    assert_eq!((resumed.tries, resumed.done_from), (0, None));
    p.pass(r.at(at + 1_000));
    let again = r.row(&row.id).unwrap();
    assert_eq!(
        (again.state, again.tries, again.done_from),
        (State::Waiting, 1, Some(0))
    );
}

#[test]
fn the_done_count_is_dropped_while_the_job_lives_and_kept_while_it_cannot_be_read() {
    let r = rig();
    let row = r.send("job-idle", "hi");
    done_with_no_process(&r);
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(r.row(&row.id).unwrap().done_from, Some(0));
    r.files.read_as(IDLE, Ended::Unknown);
    p.pass(r.at(due(&r, &row.id)));
    assert_eq!(r.row(&row.id).unwrap().done_from, Some(0));
    r.files.ended.lock().unwrap().clear();
    r.files
        .processes
        .lock()
        .unwrap()
        .insert(IDLE.into(), (4242, Some(1)));
    p.pass(r.at(due(&r, &row.id) + 60_000));
    let lived = r.row(&row.id).unwrap();
    assert_eq!((lived.tries, lived.done_from), (2, None));
    r.files.processes.lock().unwrap().clear();
    p.pass(r.at(due(&r, &row.id)));
    let done = r.row(&row.id).unwrap();
    assert_eq!((done.tries, done.done_from), (3, Some(2)));
}

#[test]
fn a_person_sending_again_gives_a_done_job_its_tries_afresh() {
    for (state, why, args) in [
        ("unsure", "not_found", json!({})),
        ("box", "not_cleared", json!({"clear_box": true})),
        ("waiting", "retry", json!({})),
    ] {
        let r = rig();
        let row = r.send("job-idle", "hi");
        done_with_no_process(&r);
        let mut p = r.postmen.postman(IDLE);
        p.pass(r.at(T0));
        assert_eq!(r.row(&row.id).unwrap().tries, 1, "{state}");
        r.ops.store().run_sql_for_tests(&format!(
            "UPDATE outbox SET state = '{state}', why = '{why}', next_ms = NULL, tries = 9
             WHERE id = '{}'",
            row.id
        ));
        let mut resend = args.clone();
        resend["id"] = json!(row.id);
        call(&r, "outbox.resend", resend);
        p.pass(r.at(T0 + 1_000));
        let after = r.row(&row.id).unwrap();
        assert_eq!(after.state, State::Waiting, "{state}: tried again");
        assert_eq!(after.tries, 10, "{state}");
    }
}

#[test]
fn a_new_row_after_a_long_unreadable_one_starts_at_checking() {
    let r = rig();
    let first = r.send("job-idle", "one");
    r.files.read_as(IDLE, Ended::Unknown);
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    p.pass(r.at(T0 + UNREADABLE_AFTER_MS));
    assert_eq!(shown(&r, &first.id), Some("unreadable".into()));
    call(&r, "outbox.remove", json!({"id": first.id}));
    let second = r.send("job-idle", "two");
    p.pass(r.at(T0 + UNREADABLE_AFTER_MS + 10_000));
    assert_eq!(shown(&r, &second.id), Some("checking".into()));
}
