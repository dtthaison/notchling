use notchling_engine::inbox::{ActionId, Item};
use notchling_local_api::ops;
use serde_json::{json, Value};

use super::feed::carried;
use super::model::ClusterModel;
use super::move_sheet::Button;
use super::promote::{MoveKind, PromotePhase, PromoteSheet};
use super::renders::{
    claim, names, side_copy, silent, status, stepped_down, waiting_entry, with_failover, ME,
    STANDBY,
};
use crate::inbox::model::InboxModel;

const NOW: i64 = 1_800_000_000_000;
const AFTERNOON: i64 = (20_000 * 86_400 + 15 * 3_600 + 30 * 60) * 1_000;

fn model_in(status: Value, me: &str) -> ClusterModel {
    ClusterModel::from_status(&status, me)
}

fn silent_model() -> ClusterModel {
    model_in(silent(), STANDBY)
}

fn sheet() -> PromoteSheet {
    PromoteSheet::new(STANDBY, "mac-mini", "MacBook")
}

fn running_sheet(model: &ClusterModel) -> PromoteSheet {
    let mut sheet = sheet();
    sheet.start(model).expect("a request");
    sheet.answered(Ok(json!({"step": "check"})));
    sheet
}

fn promoting(step: &str, started: i64) -> Value {
    let mut status = silent();
    status["promote"] = json!({"node": STANDBY, "step": step, "started": started});
    status
}

#[test]
fn no_failover_line_while_failover_is_unavailable() {
    let plain = model_in(silent(), STANDBY);
    for armed in [false, true] {
        let status = with_failover(silent(), "30m", armed, Some(AFTERNOON));
        let model = model_in(status, STANDBY);
        assert_eq!(model, plain, "nothing in the view reads the failover value");
        assert!(model.offers_promote(STANDBY), "moving by hand still works");
    }
}

#[test]
fn reachable_primary_move_runs_handover() {
    let model = model_in(status(), STANDBY);
    assert_eq!(model.move_kind(STANDBY), Some(MoveKind::Handover));
    let ask = model.move_home(STANDBY).expect("the handover");
    assert_eq!(ask.op, ops::CLUSTER_MOVE_HOME);
    assert_eq!(ask.args, json!({"to": STANDBY}));
    assert!(model.promote_request(STANDBY, false).is_none());
}

#[test]
fn unreachable_primary_move_runs_promote() {
    let model = silent_model();
    assert_eq!(model.move_kind(STANDBY), Some(MoveKind::Promote));
    assert!(model.move_home(STANDBY).is_none());
    let ask = model.promote_request(STANDBY, false).expect("the promote");
    assert_eq!(ask.op, ops::CLUSTER_PROMOTE);
    assert_eq!(ask.args, json!({"node": STANDBY}));

    assert_eq!(model.move_kind(ME), None);

    let mut off = silent();
    off["link"] = json!(false);
    assert_eq!(model_in(off, STANDBY).move_kind(STANDBY), None);

    let mut moving = silent();
    moving["move"] = json!({"to": STANDBY, "step": "freeze"});
    assert_eq!(model_in(moving, STANDBY).move_kind(STANDBY), None);

    let running = model_in(promoting("bump", 5), STANDBY);
    assert_eq!(running.move_kind(STANDBY), None);

    let mut not_ready = silent();
    not_ready["members"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter(|m| m["id"] == STANDBY)
        .for_each(|m| m["ready"] = json!({"ok": false, "reason": "no disk encryption"}));
    assert_eq!(model_in(not_ready, STANDBY).move_kind(STANDBY), None);
}

#[test]
fn the_promote_sheet_asks_before_it_sends_anything() {
    let model = silent_model();
    let mut ask = sheet();
    assert_eq!(ask.phase, PromotePhase::Confirm);
    let view = ask.view(&model);
    assert_eq!(view.title, "Move the Primary to mac-mini");
    assert!(view.intro.unwrap().starts_with("MacBook can't be reached."));
    assert_eq!(view.buttons, [Button::Start, Button::Cancel]);
    let request = ask.start(&model).expect("a request");
    assert_eq!(request.args, json!({"node": STANDBY}));
    assert_eq!(ask.phase, PromotePhase::Starting);
    assert!(ask.start(&model).is_none());

    let mut late = sheet();
    let back = model_in(status(), STANDBY);
    assert!(late.start(&back).is_none());
    assert!(matches!(late.phase, PromotePhase::Refused(_)));
}

#[test]
fn risk_confirm_only_when_asked() {
    let model = silent_model();
    let mut quiet = sheet();
    quiet.start(&model).unwrap();
    quiet.answered(Ok(json!({"step": "check"})));
    assert_eq!(quiet.phase, PromotePhase::Running);
    assert!(!quiet.view(&model).buttons.contains(&Button::Anyway));

    let mut risky = sheet();
    let first = risky.start(&model).unwrap();
    assert!(first.args.get("confirmed").is_none());
    risky.answered(Ok(
        json!({"needsConfirm": "MacBook may still be running its sessions."}),
    ));
    assert_eq!(
        risky.phase,
        PromotePhase::Risk("MacBook may still be running its sessions.".into())
    );
    let view = risky.view(&model);
    assert_eq!(view.buttons, [Button::Anyway, Button::Cancel]);
    assert!(view
        .notes
        .iter()
        .any(|n| n.problem && n.text == "MacBook may still be running its sessions."));
    let second = risky.start(&model).expect("asks again");
    assert_eq!(second.op, ops::CLUSTER_PROMOTE);
    assert_eq!(second.args, json!({"node": STANDBY, "confirmed": true}));
    assert_eq!(risky.phase, PromotePhase::Starting);
    assert!(risky.start(&model).is_none());

    let mut refused = sheet();
    refused.start(&model).unwrap();
    refused.answered(Err("A promote is already running".into()));
    let view = refused.view(&model);
    assert_eq!(view.buttons, [Button::Close]);
    assert!(view.notes[0].problem);
    assert_eq!(view.notes[0].text, "A promote is already running");
}

#[test]
fn relayed_answer_says_nothing_moved() {
    let model = silent_model();
    let mut relayed = sheet();
    relayed.start(&model).unwrap();
    relayed.answered(Ok(json!({
        "relayed": true,
        "text": "MacBook is reachable through mac-studio; nothing moved"
    })));
    let view = relayed.view(&model);
    assert!(view.steps.is_empty());
    assert_eq!(view.buttons, [Button::Close]);
    assert_eq!(
        view.notes[0].text,
        "MacBook is reachable through mac-studio; nothing moved"
    );
    assert!(!view.notes[0].problem);

    let mut bare = sheet();
    bare.start(&model).unwrap();
    bare.answered(Ok(json!({"relayed": true})));
    assert_eq!(bare.view(&model).notes[0].text, "Nothing moved.");
}

#[test]
fn promote_progress_follows_status() {
    let before = silent_model();
    let sheet = running_sheet(&before);
    let at = |step: &str| {
        let model = model_in(promoting(step, 7), STANDBY);
        sheet.view(&model)
    };
    let bump = at("bump");
    let current: Vec<&str> = bump
        .steps
        .iter()
        .filter(|s| s.state == super::move_sheet::StepState::Current)
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(current, ["Making mac-mini the primary"]);
    assert_eq!(bump.buttons, [Button::Close]);
    assert!(bump.steps[0].state == super::move_sheet::StepState::Done);
    let last = bump.steps.last().unwrap();
    assert_eq!(last.text, "mac-mini is the primary now");
    assert!(last.state == super::move_sheet::StepState::Waiting);

    let done = at("done");
    assert!(done
        .steps
        .iter()
        .all(|s| s.state == super::move_sheet::StepState::Done));

    let mut failed = promoting("failed", 7);
    failed["promote"]["error"] = json!("the restore couldn't read the set");
    let view = sheet.view(&model_in(failed, STANDBY));
    assert!(view.steps.is_empty());
    assert_eq!(
        view.notes[0].text,
        "Couldn't move the primary: the restore couldn't read the set"
    );
    assert!(view.notes[0].problem);

    let mut elsewhere = stepped_down();
    elsewhere["me"] = json!(ME);
    let view = sheet.view(&model_in(elsewhere, ME));
    assert!(view
        .steps
        .iter()
        .all(|s| s.state == super::move_sheet::StepState::Done));
}

#[test]
fn a_promote_from_before_the_sheet_is_not_its_progress() {
    let old = model_in(promoting("done", 3), STANDBY);
    let mut sheet = sheet();
    sheet.start(&old).unwrap();
    sheet.answered(Ok(json!({"step": "check"})));
    let view = sheet.view(&old);
    assert!(view.steps.is_empty());
    assert_eq!(view.intro.as_deref(), Some("Moving the primary…"));
    let fresh = model_in(promoting("restore", 9), STANDBY);
    assert!(!sheet.view(&fresh).steps.is_empty());
}

#[test]
fn a_promote_is_running_until_it_ends() {
    for step in [
        "check", "reach", "bump", "restore", "resume", "held", "items", "announce",
    ] {
        assert!(
            model_in(promoting(step, 1), STANDBY).promote_running(),
            "{step}"
        );
    }
    for step in ["done", "relayed", "failed", "confirm"] {
        assert!(
            !model_in(promoting(step, 1), STANDBY).promote_running(),
            "{step}"
        );
    }
    assert!(!silent_model().promote_running());
}

#[test]
fn take_back_on_old_primary_row() {
    let mut back = stepped_down();
    back["sideCopies"] = json!([side_copy("j-parser", ME, Some("/a/b.jsonl"), NOW, false)]);
    let model = model_in(back, ME);
    assert!(model.offers_take_back());
    let ask = model.take_back_request().expect("take back");
    assert_eq!(ask.op, ops::CLUSTER_TAKE_BACK);
    assert_eq!(ask.args, json!({}));
    let words = model.take_back_ask();
    assert_eq!(words.title, "Take Back the Primary?");
    assert_eq!(words.message, "Sessions come home after their turns end.");
    assert_eq!(words.confirm, "Take Back");

    let mut claimed = stepped_down();
    claimed["claims"] = json!([claim("j-parser", STANDBY)]);
    assert!(model_in(claimed, ME).offers_take_back());

    assert!(!model_in(stepped_down(), ME).offers_take_back());

    let mut moving = stepped_down();
    moving["sideCopies"] = json!([side_copy("j", ME, None, NOW, false)]);
    moving["move"] = json!({"to": ME, "step": "freeze"});
    assert!(!model_in(moving, ME).offers_take_back());

    let mut primary = status();
    primary["sideCopies"] = json!([side_copy("j", ME, None, NOW, false)]);
    assert!(!model_in(primary, ME).offers_take_back());

    let mut gone = stepped_down();
    gone["sideCopies"] = json!([side_copy("j", ME, None, NOW, false)]);
    gone["link"] = json!(false);
    assert!(!model_in(gone, ME).offers_take_back());
}

#[test]
fn side_copy_rows_open_and_hide_never_delete() {
    let mut with = silent();
    with["sideCopies"] = json!([
        side_copy(
            "j-parser",
            STANDBY,
            Some("/x/parser.jsonl"),
            NOW - 5 * 60_000,
            false
        ),
        side_copy(
            "j-bench",
            ME,
            Some("/y/bench.jsonl"),
            NOW - 3 * 3_600_000,
            false
        ),
        side_copy("j-fork", STANDBY, None, NOW - 10_000, false),
    ]);
    let model = model_in(with, STANDBY);
    let rows = model.side_copy_rows(&names(), NOW);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].session, "j-fork");
    assert_eq!(rows[0].title, "Try the fork");
    assert_eq!(rows[0].line, "On mac-mini · just now");
    assert_eq!(rows[0].open, None);
    assert_eq!(rows[1].title, "Fix the parser");
    assert_eq!(rows[1].line, "On mac-mini · 5 minutes ago");
    assert_eq!(rows[1].open.as_deref(), Some("/x/parser.jsonl"));
    assert_eq!(rows[2].title, "Benchmark the sync");
    assert_eq!(rows[2].line, "On MacBook · 3 hours ago");
    assert_eq!(rows[2].open, None);

    let hide = model
        .hide_side_copy(&rows[1].session, &rows[1].node)
        .expect("hide");
    assert_eq!(hide.op, ops::CLUSTER_SIDE_COPY_HIDE);
    assert_eq!(hide.args, json!({"session": "j-parser", "node": STANDBY}));
    assert!(model.hide_side_copy("j-parser", ME).is_none());
    assert!(model.hide_side_copy("unknown", STANDBY).is_none());
}

#[test]
fn a_side_copy_without_a_known_title_shows_its_id() {
    let mut with = silent();
    with["sideCopies"] = json!([side_copy("0123456789abcdef", STANDBY, None, NOW, false)]);
    let rows = model_in(with, STANDBY).side_copy_rows(&Default::default(), NOW);
    assert_eq!(rows[0].title, "01234567");
}

#[test]
fn hidden_side_copies_are_not_listed() {
    let mut with = silent();
    with["sideCopies"] = json!([
        side_copy("j-parser", STANDBY, None, NOW, true),
        side_copy("j-bench", STANDBY, None, NOW, false),
    ]);
    let model = model_in(with, STANDBY);
    let rows = model.side_copy_rows(&names(), NOW);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].session, "j-bench");
    assert!(model.hide_side_copy("j-parser", STANDBY).is_none());

    let mut all_hidden = silent();
    all_hidden["sideCopies"] = json!([side_copy("j-parser", STANDBY, None, NOW, true)]);
    assert!(model_in(all_hidden, STANDBY)
        .side_copy_rows(&names(), NOW)
        .is_empty());
    assert!(silent_model().side_copy_rows(&names(), NOW).is_empty());
}

#[test]
fn waiting_session_offers_wait_and_continue() {
    let mut with = silent();
    with["waiting"] = json!([
        waiting_entry("j-parser", ME, "MacBook", None),
        waiting_entry("j-docs", ME, "MacBook", Some("Write the docs")),
    ]);
    let model = model_in(with, STANDBY);
    let rows = model.waiting_rows(&names());
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].title, "Fix the parser");
    assert_eq!(
        rows[0].text,
        "Fix the parser waits for MacBook, which has newer work in it."
    );
    assert_eq!(rows[1].title, "Write the docs");
    assert_eq!(rows[0].wait, "Wait");
    assert_eq!(rows[0].continue_here, "Continue From This Copy");
    assert!(silent_model().waiting_rows(&names()).is_empty());
}

#[test]
fn continue_from_this_copy_confirms_first() {
    let mut with = silent();
    with["waiting"] = json!([waiting_entry("j-parser", ME, "MacBook", None)]);
    let model = model_in(with, STANDBY);
    let ask = model
        .continue_ask("j-parser", &names())
        .expect("a question");
    assert_eq!(ask.title, "Continue From This Copy?");
    assert_eq!(
        ask.message,
        "Continue Fix the parser here? The newer copy on MacBook is kept beside it."
    );
    assert_eq!(ask.confirm, "Continue From This Copy");
    let request = model
        .continue_request("j-parser")
        .expect("after the answer");
    assert_eq!(request.op, ops::CLUSTER_CONTINUE_HERE);
    assert_eq!(request.args, json!({"id": "j-parser"}));
    assert!(model.continue_ask("j-other", &names()).is_none());
    assert!(model.continue_request("j-other").is_none());
}

#[test]
fn paused_step_item_shows_continue_and_done() {
    let item: Item = serde_json::from_value(json!({
        "id": "paused-step:j-parser:0",
        "kind": "paused_step",
        "title": "MacBook dropped while git push was running. Check, then Continue.",
        "detail": "It may have changed something outside this machine, so it was not run again.",
        "source": {"node": null, "session": "j-parser", "job": null},
        "created_ms": 1,
        "expires_ms": null,
        "actions": [
            {"id": "continue", "label": "Continue", "primary": true},
            {"id": "done", "label": "Done", "primary": false}
        ],
        "state": "open",
        "answer": null,
        "answered_by": null,
        "seen": false,
        "open": null
    }))
    .unwrap();
    let model = InboxModel::from_items(vec![item], 2);
    let (needs, done) = model.sections();
    assert!(done.is_empty());
    assert_eq!(needs.len(), 1);
    assert!(needs[0].enabled);
    assert_eq!(
        needs[0].actions,
        [
            (ActionId::Continue, "Continue".to_owned(), true),
            (ActionId::Done, "Done".to_owned(), false)
        ]
    );
}

#[test]
fn events_keep_the_promote_and_waiting() {
    let mut last = with_failover(promoting("bump", 4), "30m", true, Some(AFTERNOON));
    last["waiting"] = json!([waiting_entry("j-parser", ME, "MacBook", None)]);
    let mut event = last.clone();
    for key in ["promote", "waiting"] {
        event.as_object_mut().unwrap().remove(key);
    }
    let merged = carried(&last, &event);
    assert_eq!(merged["promote"], last["promote"]);
    assert_eq!(merged["waiting"], last["waiting"]);

    let fresh = json!({"promote": null});
    let merged = carried(&last, &fresh);
    assert_eq!(merged["promote"], Value::Null);
}
