#![cfg(unix)]

mod common;

use common::{home, notchling, scratch, Daemon};
use hmac::{Hmac, Mac};
use notchling_engine::organizations::{Kind, Organizations};
use notchling_local_api::{Client, ClientKind};
use serde_json::{json, Value};
use sha2::Sha256;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const SESSION: &str = "52afae32-0000-4000-8000-000000000001";
const JOB_SESSION: &str = "be50d36a-0000-4000-8000-000000000002";
const HELPER: &str = "accounts_helper_runs_commands_as_the_session_process";
const ANN: &str = "a1a1:0101";
const ZED: &str = "a2a2:0202";

#[test]
fn accounts_helper_runs_commands_as_the_session_process() {
    let Ok(bin) = std::env::var("NL_ACCOUNTS_E2E_BIN") else {
        return;
    };
    let mut go = String::new();
    std::io::stdin().lock().read_line(&mut go).unwrap();
    let steps: Vec<Vec<String>> =
        serde_json::from_str(&std::env::var("NL_ACCOUNTS_E2E_STEPS").unwrap()).unwrap();
    for step in steps {
        let out = Command::new(&bin)
            .args(&step)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        println!(
            "STEP {}",
            json!({"code": out.status.code(), "out": String::from_utf8_lossy(&out.stdout),
                   "err": String::from_utf8_lossy(&out.stderr)})
        );
    }
}

fn as_session(root: &Path, home: &Path, steps: &[&[&str]]) -> Vec<Value> {
    let claude_dir = root.join(".local/share/claude/versions/1");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let claude = claude_dir.join("claude");
    if !claude.exists() {
        std::fs::copy(std::env::current_exe().unwrap(), &claude).unwrap();
    }
    let mut parent = common::spawn_retrying(
        Command::new(&claude)
            .args(["--exact", HELPER, "--nocapture", "--test-threads=1"])
            .env("NL_ACCOUNTS_E2E_BIN", env!("CARGO_BIN_EXE_notchling"))
            .env(
                "NL_ACCOUNTS_E2E_STEPS",
                serde_json::to_string(steps).unwrap(),
            )
            .env("NOTCHLING_DATA_ROOT", root)
            .env("HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
    );
    let sessions = root.join("claude/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let file = json!({
        "sessionId": SESSION,
        "pid": parent.id(),
        "cwd": root.join("launch").to_string_lossy(),
        "jobId": "j1",
    });
    std::fs::write(
        sessions.join(format!("{}.json", parent.id())),
        file.to_string(),
    )
    .unwrap();
    writeln!(parent.stdin.take().unwrap(), "go").unwrap();
    let reader = BufReader::new(parent.stdout.take().unwrap());
    let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
    let _ = parent.wait();
    lines
        .iter()
        .filter_map(|line| line.split_once("STEP ").map(|(_, text)| text.to_owned()))
        .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
        .collect()
}

fn parsed(step: &Value) -> Value {
    serde_json::from_str(step["out"].as_str().unwrap_or("")).unwrap_or(Value::Null)
}

fn code(step: &Value) -> i64 {
    step["code"].as_i64().unwrap_or(-1)
}

struct World {
    _dir: common::Dir,
    root: PathBuf,
    home: PathBuf,
}

fn world(tag: &str) -> World {
    let dir = scratch(tag);
    let root = dir.0.join("root");
    let home = home(&dir.0);
    let work = dir.0.join("work");
    std::fs::create_dir_all(work.join("app")).unwrap();
    let data = root.join("notchling");
    std::fs::create_dir_all(data.join("daemon")).unwrap();
    let book = Organizations::new(&data).with_home(&root);
    let acme = book.create("Acme", None, None).unwrap();
    let globex = book.create("Globex", None, None).unwrap();
    book.set_folders(&acme, &[work.to_string_lossy().into_owned()])
        .unwrap();
    notchling_engine::store::put_doc(
        &data,
        Kind::Claude.doc(),
        &json!([
            {"id": ANN, "email": "ann@work.com", "plan": "max"},
            {"id": ZED, "email": "zed@else.com", "plan": "max"},
        ])
        .to_string(),
    )
    .unwrap();
    book.set_organization(Kind::Claude, ANN, &acme).unwrap();
    book.set_organization(Kind::Claude, ZED, &globex).unwrap();
    let job = root.join("claude/jobs/j1");
    std::fs::create_dir_all(&job).unwrap();
    std::fs::write(
        job.join("state.json"),
        json!({
            "sessionId": JOB_SESSION,
            "resumeSessionId": SESSION,
            "cwd": work.join("app").to_string_lossy(),
            "name": "work",
            "state": "running",
        })
        .to_string(),
    )
    .unwrap();
    World {
        _dir: dir,
        root,
        home,
    }
}

fn app(root: &Path) -> Client {
    let mut client = common::connect(root, ClientKind::App);
    client.set_timeout(Duration::from_secs(30));
    client
}

fn open_cards(app: &mut Client) -> Vec<Value> {
    app.call("inbox.list", json!({})).unwrap()["items"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|item| item["state"] == "open")
        .collect()
}

fn first_account(summary: &Value) -> String {
    summary["tools"]["claude"]
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row["id"].as_str())
        .unwrap_or_else(|| panic!("no claude account row: {summary}"))
        .to_string()
}

fn opaque(root: &Path, raw: &str) -> String {
    let key = std::fs::read(root.join("notchling/daemon/account-id.key")).unwrap();
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&key).unwrap();
    mac.update(b"claudeCode");
    mac.update(&[0u8]);
    mac.update(raw.as_bytes());
    let tag = mac.finalize().into_bytes();
    let hex: String = tag[..8].iter().map(|b| format!("{b:02x}")).collect();
    format!("acct-{hex}")
}

#[test]
fn a_shell_is_refused_with_no_session_and_exit_4() {
    let world = world("accounts-e2e-shell");
    let _daemon = Daemon::start(&world.root, &world.home);
    for args in [
        &["accounts", "switch", "acct-1111111111111111", "--json"][..],
        &["accounts", "add", "claude", "--json"][..],
        &["accounts", "move-all", "main", "--json"][..],
        &["lanes", "create", "acct-1111111111111111", "--json"][..],
    ] {
        let (code, out, err) = notchling(&world.root, &world.home, args);
        assert_eq!(code, 4, "{args:?} {out} {err}");
        let value: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["error"]["reason"], "no_session", "{args:?} {out}");
        assert!(
            value["error"]["message"]
                .as_str()
                .unwrap()
                .contains("session in the organization's folder"),
            "{out}"
        );
    }
    let (code, out, _) = notchling(&world.root, &world.home, &["lanes", "list"]);
    assert_eq!(code, 0, "{out}");
    let value: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["reason"], "no_session", "{out}");
}

#[test]
fn dry_run_prints_the_change_and_exits_0() {
    let world = world("accounts-e2e-dry");
    let _daemon = Daemon::start_for_app(&world.root, &world.home);
    let mut app = app(&world.root);
    app.call(
        "settings.set_switch",
        json!({"switch": "accounts", "on": true}),
    )
    .unwrap();

    let read = as_session(&world.root, &world.home, &[&["accounts", "--json"]]);
    let ann = first_account(&parsed(&read[0]));
    assert!(ann.starts_with("acct-"), "{ann}");

    let steps = as_session(
        &world.root,
        &world.home,
        &[&["accounts", "switch", &ann, "--dry-run", "--json"]],
    );
    assert_eq!(code(&steps[0]), 0, "{steps:?}");
    let out = parsed(&steps[0]);
    assert_eq!(out["dryRun"], true, "{out}");
    assert_eq!(out["change"]["action"], "switch", "{out}");
    let text = steps[0]["out"].as_str().unwrap();
    for secret in [ANN, "ann@work.com", "email:"] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
    assert!(open_cards(&mut app).is_empty(), "a dry run opens no card");
}

#[test]
fn a_switch_goes_from_the_words_through_the_daemon_to_an_open_card() {
    let world = world("accounts-e2e-card");
    let _daemon = Daemon::start_for_app(&world.root, &world.home);
    let mut app = app(&world.root);
    app.call(
        "settings.set_switch",
        json!({"switch": "accounts", "on": true}),
    )
    .unwrap();

    let read = as_session(&world.root, &world.home, &[&["accounts", "--json"]]);
    let summary = parsed(&read[0]);
    let ann = first_account(&summary);

    let steps = as_session(
        &world.root,
        &world.home,
        &[&[
            "accounts",
            "switch",
            &ann,
            "--reason",
            "the work login",
            "--no-wait",
            "--json",
        ]],
    );
    assert_eq!(code(&steps[0]), 6, "{steps:?}");
    let out = parsed(&steps[0]);
    let request = &out["error"]["request"];
    assert_eq!(request["kind"], "account", "{out}");
    assert_eq!(request["state"], "waiting", "{out}");

    let cards = open_cards(&mut app);
    assert_eq!(cards.len(), 1, "{cards:?}");
    assert_eq!(cards[0]["kind"], "consent", "{cards:?}");
    let card = serde_json::to_string(&cards[0]).unwrap();
    for secret in [ANN, "ann@work.com", "email:", "https://"] {
        assert!(!card.contains(secret), "{secret} in {card}");
    }
}

#[test]
fn another_organizations_account_exits_4_never() {
    let world = world("accounts-e2e-never");
    let _daemon = Daemon::start_for_app(&world.root, &world.home);
    let mut app = app(&world.root);
    app.call(
        "settings.set_switch",
        json!({"switch": "accounts", "on": true}),
    )
    .unwrap();

    let read = as_session(&world.root, &world.home, &[&["accounts", "--json"]]);
    let summary = parsed(&read[0]);
    assert_eq!(
        summary["tools"]["claude"].as_array().unwrap().len(),
        1,
        "{summary}"
    );
    let zed = opaque(&world.root, ZED);
    assert_ne!(zed, first_account(&summary));

    let steps = as_session(
        &world.root,
        &world.home,
        &[&["accounts", "switch", &zed, "--json"]],
    );
    assert_eq!(code(&steps[0]), 4, "{steps:?} {summary}");
    let out = parsed(&steps[0]);
    assert_eq!(out["error"]["reason"], "never", "{out}");
    assert_eq!(out["error"]["never"], true, "{out}");
    assert!(
        out["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Only you can do this, in Notchling."),
        "{out}"
    );
    assert!(open_cards(&mut app).is_empty(), "no card for a refusal");
}
