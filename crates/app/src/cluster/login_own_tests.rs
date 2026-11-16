use serde_json::{json, Value};

use super::model::{ClusterModel, Login};
use super::renders::{status, ME, STANDBY, WORKER};

fn model_of(status: &Value) -> ClusterModel {
    ClusterModel::from_status(status, ME)
}

fn report(status: &mut Value, id: &str, key: &str, value: Value) {
    let member = status["members"]
        .as_array_mut()
        .and_then(|members| members.iter_mut().find(|m| m["id"] == id))
        .expect("member");
    member["versions"][key] = value;
}

fn claude_login(model: &ClusterModel, id: &str) -> Login {
    model.row(id).expect("row").logins[0].1
}

#[test]
fn a_nodes_own_claude_login_shows_as_signed_in() {
    let mut reported = status();
    report(&mut reported, ME, "own_logins", json!({"claude": true}));
    let model = model_of(&reported);
    assert_eq!(claude_login(&model, ME), Login::SignedIn);
    let texts: Vec<String> = model
        .row(ME)
        .expect("row")
        .login_texts()
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    assert_eq!(texts[0], "Claude Code: Signed In");
}

#[test]
fn a_node_that_says_nothing_about_its_own_login_stays_unknown() {
    let model = model_of(&status());
    assert_eq!(claude_login(&model, ME), Login::Unknown);
    assert_eq!(claude_login(&model, STANDBY), Login::Unknown);
}

#[test]
fn an_own_login_never_makes_a_missing_tool_installed() {
    let mut reported = status();
    report(&mut reported, WORKER, "own_logins", json!({"claude": true}));
    let model = model_of(&reported);
    assert_eq!(claude_login(&model, WORKER), Login::NotInstalled);
}

#[test]
fn a_machine_that_runs_jobs_says_whether_the_worker_login_is_on() {
    let off = model_of(&status());
    assert_eq!(
        off.row(STANDBY).expect("row").worker_login_text(),
        Some(("Worker Login: Off".to_owned(), false))
    );
    let mut sent = status();
    report(
        &mut sent,
        STANDBY,
        "logins",
        json!({"claude": true, "codex": true, "antigravity": true}),
    );
    let on = model_of(&sent);
    assert_eq!(
        on.row(STANDBY).expect("row").worker_login_text(),
        Some(("Worker Login: On".to_owned(), true))
    );
}

#[test]
fn the_worker_login_is_kept_apart_from_the_claude_word() {
    let mut own_only = status();
    report(
        &mut own_only,
        STANDBY,
        "own_logins",
        json!({"claude": true}),
    );
    let model = model_of(&own_only);
    assert_eq!(claude_login(&model, STANDBY), Login::SignedIn);
    assert_eq!(
        model.row(STANDBY).expect("row").worker_login_text(),
        Some(("Worker Login: Off".to_owned(), false))
    );
    let mut worker_only = status();
    report(
        &mut worker_only,
        STANDBY,
        "logins",
        json!({"claude": true, "codex": true, "antigravity": true}),
    );
    assert_eq!(
        claude_login(&model_of(&worker_only), STANDBY),
        Login::SignedIn
    );
}

#[test]
fn this_mac_and_a_machine_without_claude_show_no_worker_login() {
    let model = model_of(&status());
    assert_eq!(model.row(ME).expect("row").worker_login_text(), None);
    assert_eq!(model.row(WORKER).expect("row").worker_login_text(), None);
}

#[test]
fn the_codex_and_antigravity_words_do_not_move() {
    let mut reported = status();
    report(
        &mut reported,
        STANDBY,
        "own_logins",
        json!({"claude": true}),
    );
    let logins = model_of(&reported)
        .row(STANDBY)
        .expect("row")
        .logins
        .clone();
    assert_eq!(
        logins,
        [
            ("Claude Code".to_owned(), Login::SignedIn),
            ("Codex".to_owned(), Login::SignedIn),
            ("Antigravity".to_owned(), Login::SignedIn),
        ]
    );
}
