mod common;

use common::verified::{run_helper, Session, Step};
use common::{home, scratch, Daemon};
use notchling_engine::install::skills::rendered;
use notchling_engine::pages::{PageLibrary, RecordContext};
use notchling_local_api::{ops, Client, ClientKind};
use notchling_notchlingd::skills_scan::{check_against_rows, installed_specs, known, sh_examples};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const HELPER: &str = "skills_helper_runs_commands_as_the_session_process";
const PAGE: &str = "AbCdEfGhIjKlMnOpQrStUv";
const OTHER: &str = "be50d36a-0000-4000-8000-000000000002";
const WORDS: &str = "Cost Report";
const LIMIT: Duration = Duration::from_secs(20);

#[path = "skills_e2e_rows/mod.rs"]
mod rows;
use rows::*;

#[test]
fn skills_helper_runs_commands_as_the_session_process() {
    run_helper();
}

fn plant_other_session(root: &Path) {
    let job = root.join("claude/jobs/j1");
    std::fs::create_dir_all(&job).unwrap();
    std::fs::write(
        job.join("state.json"),
        json!({
            "sessionId": OTHER,
            "cwd": "/tmp",
            "name": "work",
            "state": "running",
        })
        .to_string(),
    )
    .unwrap();
}

fn fixture_pages(root: &Path) {
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(
        work.join("a.html"),
        "<title>Cost Report</title><p>numbers</p>",
    )
    .unwrap();
    let lib = PageLibrary::writable(root.join("notchling/pages"));
    let event = json!({
        "tool_name": "Artifact",
        "cwd": work,
        "tool_input": {"file_path": "a.html"},
        "tool_response": format!("Published https://claude.ai/code/artifact/{PAGE}"),
    });
    lib.record(
        &event,
        &RecordContext {
            account: "me@example.com",
            now: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0),
            home: root,
        },
    )
    .unwrap()
    .unwrap();
}

fn examples_of_installed_skills(binary: &str, codex_home: &Path) -> BTreeSet<(String, String)> {
    let mut found = BTreeSet::new();
    for spec in installed_specs(Path::new(binary), codex_home, None) {
        for tool in spec.tools.clone() {
            let text = rendered(&spec, tool);
            for line in sh_examples(binary, &text) {
                let rest = line
                    .strip_prefix(&format!("'{binary}' "))
                    .unwrap_or(&line)
                    .to_string();
                found.insert((spec.name.clone(), rest));
            }
        }
    }
    found
}

fn the_rows_and_the_examples_agree(
    examples: &BTreeSet<(String, String)>,
    rows: &[Example],
) -> Vec<String> {
    let mut problems = Vec::new();
    let extras: BTreeSet<(String, String)> = rows
        .iter()
        .filter(|r| r.extra)
        .map(|r| (r.skill.to_string(), r.line.to_string()))
        .collect();
    let table: BTreeSet<(String, String)> = rows
        .iter()
        .map(|r| (r.skill.to_string(), r.line.to_string()))
        .collect();
    if table.len() != rows.len() {
        problems.push("two rows name the same example".to_string());
    }
    for (skill, line) in examples {
        if AWAITING.contains(&skill.as_str()) {
            continue;
        }
        if !table.contains(&(skill.clone(), line.clone())) {
            problems.push(format!("{skill}: example has no row: {line}"));
        }
    }
    for (skill, line) in &table {
        if extras.contains(&(skill.clone(), line.clone())) {
            continue;
        }
        if !examples.contains(&(skill.clone(), line.clone())) {
            problems.push(format!("{skill}: row has no example: {line}"));
        }
    }
    for skill in AWAITING {
        if table.iter().any(|(s, _)| s == skill) {
            problems.push(format!("{skill} has rows, so it is not awaiting"));
        }
    }
    problems
}

fn decide_card(app: &mut Client, card: &Value, allow: bool) {
    let id = &card["id"];
    if allow && card["kind"] == "switch" && card["appliedBy"] == "app" {
        let _ = app.call(ops::CONSENT_OPENED, json!({"consentId": id}));
        let target = card["target"].as_str().unwrap_or("");
        if target == "acting" {
            let _ = app.call("settings.set_acting", json!({"on": true, "consentId": id}));
        } else {
            let _ = app.call(
                "settings.set_switch",
                json!({"switch": target, "on": true, "consentId": id}),
            );
        }
        return;
    }
    let _ = app.call(
        ops::CONSENT_ANSWER,
        json!({"consentId": id, "decision": if allow { "allow" } else { "deny" }}),
    );
}

fn answer_cards(root: &Path, allow: bool, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    let socket = common::socket(root);
    std::thread::spawn(move || {
        let Ok(mut app) = Client::connect(&socket, ClientKind::App, "0.1.0") else {
            return;
        };
        app.set_timeout(Duration::from_secs(10));
        while !stop.load(Ordering::SeqCst) {
            if let Ok(list) = app.call(ops::CONSENT_LIST, json!({})) {
                for item in list["requests"].as_array().cloned().unwrap_or_default() {
                    if item["state"] == "waiting" {
                        decide_card(&mut app, &item, allow);
                    }
                }
            }
            if let Ok(list) = app.call(ops::ASK_LIST, json!({})) {
                for row in list.as_array().cloned().unwrap_or_default() {
                    if row["state"] == "open" {
                        let _ = app.call(ops::ASK_ANSWER, json!({"id": row["id"], "choice": 1}));
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    })
}

fn set_switch(root: &Path, name: &str, on: bool) {
    let mut app = Client::connect(&common::socket(root), ClientKind::App, "0.1.0").unwrap();
    app.set_timeout(Duration::from_secs(10));
    if name == "acting" {
        app.call("settings.set_acting", json!({"on": on})).unwrap();
        return;
    }
    app.call("settings.set_switch", json!({"switch": name, "on": on}))
        .unwrap();
}

fn a_question_of_the_session(
    root: &Path,
    home: &Path,
    binary: &str,
) -> (String, std::thread::JoinHandle<()>) {
    set_switch(root, "acting", true);
    let (root_owned, home_owned, binary_owned) =
        (root.to_path_buf(), home.to_path_buf(), binary.to_string());
    let asker = std::thread::spawn(move || {
        Session {
            root: &root_owned,
            home: &home_owned,
            helper: HELPER,
            bin: &binary_owned,
            folder: None,
            job: false,
            step_timeout: Duration::from_secs(300),
        }
        .run(&[Step::args(&[
            "ask", "--choice", "yes", "--choice", "no", "--", "Go on?",
        ])]);
    });
    let mut app = Client::connect(&common::socket(root), ClientKind::App, "0.1.0").unwrap();
    app.set_timeout(Duration::from_secs(10));
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let id = loop {
        let rows = app.call(ops::ASK_LIST, json!({})).unwrap_or(Value::Null);
        let open = rows
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["state"] == "open"))
            .and_then(|row| row["id"].as_str().map(str::to_string));
        if let Some(id) = open {
            break id;
        }
        assert!(std::time::Instant::now() < deadline, "no question opened");
        std::thread::sleep(Duration::from_millis(100));
    };
    set_switch(root, "acting", false);
    (id, asker)
}

#[test]
fn every_skill_example_runs_as_an_agent_would() {
    let binary = env!("CARGO_BIN_EXE_notchling");
    let dir = scratch("skills-e2e");
    let root = common::fixture_root(&dir.0);
    let home = home(&dir.0);
    let codex = home.join(".codex");
    std::fs::create_dir_all(&codex).unwrap();
    fixture_pages(&root);
    plant_other_session(&root);

    let rows = table();
    let examples = examples_of_installed_skills(binary, &codex);
    let mut problems = the_rows_and_the_examples_agree(&examples, &rows);

    let known = known();
    common::verified::plant_claude(&root);
    let daemon = Daemon::start_for_app(&root, &home).warmed(&root);
    let (ask, asker) = a_question_of_the_session(&root, &home, binary);
    for row in &rows {
        let command = row.command(binary, "self", "ask");
        let line = command
            .strip_prefix(&format!("'{binary}' "))
            .unwrap_or(&command);
        let parse_only = row.setup.iter().find_map(|s| match s {
            Setup::ParseOnly(why) => Some(*why),
            _ => None,
        });
        if let Some(why) = parse_only {
            let words = row.line.split(" -- ").next().unwrap_or(row.line);
            if let Err(why_not) = check_against_rows(&known, words) {
                problems.push(format!("{}: {why_not} (parse only: {why})", row.skill));
            }
            continue;
        }
        let switches: Vec<&str> = row
            .setup
            .iter()
            .filter_map(|s| match s {
                Setup::SwitchOn(name) => Some(*name),
                _ => None,
            })
            .collect();
        for name in &switches {
            set_switch(&root, name, true);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let answer = row.setup.iter().find_map(|s| match s {
            Setup::Allow => Some(true),
            Setup::Deny => Some(false),
            _ => None,
        });
        let responder = answer.map(|allow| answer_cards(&root, allow, stop.clone()));
        let session = Session {
            root: &root,
            home: &home,
            helper: HELPER,
            bin: binary,
            folder: None,
            job: false,
            step_timeout: LIMIT,
        };
        let me = common::verified::SESSION;
        let command = row.command(binary, me, &ask);
        let started = std::time::Instant::now();
        let steps = session.run(&[Step::Shell(command.clone())]);
        stop.store(true, Ordering::SeqCst);
        if let Some(handle) = responder {
            let _ = handle.join();
        }
        for name in &switches {
            set_switch(&root, name, false);
        }
        let Some(step) = steps.first() else {
            problems.push(format!("{}: `{line}` gave no result", row.skill));
            continue;
        };
        let code = step["code"].as_i64().map(|c| c as i32);
        let out = step["out"].as_str().unwrap_or("");
        let err = step["err"].as_str().unwrap_or("");
        let head = format!("{}: `{}`", row.skill, row.line);
        if step["timedOut"] == true || started.elapsed() > LIMIT + Duration::from_secs(5) {
            problems.push(format!("{head} did not finish in {LIMIT:?}"));
        }
        if code == Some(2) && !(row.extra && row.exit == 2) {
            problems.push(format!("{head} is a bad command line: {err}{out}"));
        } else if code != Some(row.exit) {
            problems.push(format!(
                "{head} exited {code:?}, the row says {}: {out}{err}",
                row.exit
            ));
        }
        if command.contains(" --json") {
            if serde_json::from_str::<Value>(out).is_err() {
                problems.push(format!("{head} printed no single JSON value: {out}"));
            }
            if !err.is_empty() {
                problems.push(format!("{head} wrote to stderr under --json: {err}"));
            }
        }
    }
    let mut app = Client::connect(&common::socket(&root), ClientKind::App, "0.1.0").unwrap();
    app.set_timeout(Duration::from_secs(10));
    let _ = app.call(ops::ASK_ANSWER, json!({"id": ask, "choice": 1}));
    asker.join().unwrap();
    drop(daemon);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn a_made_up_example_has_no_row_and_a_made_up_row_has_no_example() {
    let binary = "/Apps/N";
    let mut examples = examples_of_installed_skills(binary, Path::new("/nowhere"));
    let rows = table();
    assert!(
        the_rows_and_the_examples_agree(&examples, &rows).is_empty(),
        "{:?}",
        the_rows_and_the_examples_agree(&examples, &rows)
    );
    examples.insert((GENERAL.to_string(), "teleport <id>".to_string()));
    let problems = the_rows_and_the_examples_agree(&examples, &rows);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("example has no row: teleport <id>"));

    let mut rows = table();
    rows.push(ex(GENERAL, "frobnicate now"));
    let problems = the_rows_and_the_examples_agree(
        &examples_of_installed_skills(binary, Path::new("/nowhere")),
        &rows,
    );
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("row has no example: frobnicate now"));
}

#[test]
fn a_denied_request_ends_the_example_with_exit_5() {
    let binary = env!("CARGO_BIN_EXE_notchling");
    let dir = scratch("skills-e2e-deny");
    let root = common::fixture_root(&dir.0);
    let home = home(&dir.0);
    common::verified::plant_claude(&root);
    let _daemon = Daemon::start_for_app(&root, &home).warmed(&root);
    let stop = Arc::new(AtomicBool::new(false));
    let responder = answer_cards(&root, false, stop.clone());
    let steps = Session {
        root: &root,
        home: &home,
        helper: HELPER,
        bin: binary,
        folder: None,
        job: false,
        step_timeout: LIMIT,
    }
    .run(&[Step::args(&[
        "switch",
        "on",
        "vault",
        "--reason",
        "the task needs it",
        "--timeout",
        "10",
        "--json",
    ])]);
    stop.store(true, Ordering::SeqCst);
    responder.join().unwrap();
    assert_eq!(steps[0]["code"], 5, "{steps:?}");
}
