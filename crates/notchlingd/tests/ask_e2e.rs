#![cfg(unix)]

mod common;

use common::{connect, scratch, Daemon};
use notchling_local_api::{ops, Client, ClientKind};
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Kid(Child);

impl Drop for Kid {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn eventually<T>(what: &str, mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if let Some(found) = probe() {
            return found;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

fn app(root: &Path) -> Client {
    let mut client = connect(root, ClientKind::App);
    client.set_timeout(Duration::from_secs(30));
    client
        .call("settings.set_acting", json!({"on": true}))
        .unwrap();
    client
}

fn open_row(app: &mut Client) -> Option<Value> {
    let rows = app.call(ops::ASK_LIST, json!({})).ok()?;
    rows.as_array()?
        .iter()
        .find(|row| row["state"] == "open")
        .cloned()
}

#[test]
fn restart_keeps_the_same_question() {
    let dir = scratch("ask-e2e");
    let root = dir.0.join("root");
    let home = common::home(&dir.0);
    let sessions = root.join("claude/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(
        sessions.join(format!("{}.json", std::process::id())),
        br#"{"sessionId":"sess-ask-e2e"}"#,
    )
    .unwrap();

    let mut daemon = Daemon::start_for_app(&root, &home);
    let mut first = app(&root);

    let out_file = dir.0.join("ask.out");
    let err_file = dir.0.join("ask.err");
    let mut ask = Kid(Command::new(env!("CARGO_BIN_EXE_notchling"))
        .args(["ask", "--choice", "Keep it", "--choice", "Drop it", "--"])
        .arg("Keep the cache?")
        .env("NOTCHLING_DATA_ROOT", &root)
        .env("HOME", &home)
        .env_remove(notchling_local_api::APP_TIER_PATHS_ENV)
        .env_remove("NOTCHLING_DELEGATE_DEADLINE_MS")
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&out_file).unwrap())
        .stderr(std::fs::File::create(&err_file).unwrap())
        .spawn()
        .unwrap());

    let row = eventually("the question is open", || open_row(&mut first));
    let id = row["id"].as_str().unwrap().to_string();
    assert_eq!(row["question"], "Keep the cache?");
    eventually("the command says it asked", || {
        std::fs::read_to_string(&err_file)
            .ok()
            .filter(|text| text.contains(&format!("Asked ({id}).")))
    });

    drop(first);
    daemon.stop();
    assert!(
        matches!(ask.0.try_wait(), Ok(None)),
        "the command keeps waiting while the daemon is down"
    );

    let mut daemon = Daemon::start_for_app(&root, &home);
    let mut second = app(&root);
    let again = eventually("the same question is back", || open_row(&mut second));
    assert_eq!(again["id"], json!(id), "nothing is asked twice");

    let reply = second
        .call(ops::ASK_ANSWER, json!({"id": id, "choice": 1}))
        .unwrap();
    assert_eq!(reply, json!({"answered": true}));

    let status = eventually("the command ends", || ask.0.try_wait().ok().flatten());
    assert_eq!(status.code(), Some(0));
    assert_eq!(
        std::fs::read_to_string(&out_file).unwrap(),
        "Answer: Keep it\nFrom: you (Mac)\n"
    );
    let err = std::fs::read_to_string(&err_file).unwrap();
    assert_eq!(err.matches("Asked (").count(), 1, "{err}");
    assert!(err.contains(&format!("Asked ({id}).")), "{err}");
    let rows = second.call(ops::ASK_LIST, json!({})).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1, "{rows}");
    drop(second);
    daemon.stop();
}
