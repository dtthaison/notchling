use crate::hook_pages::{answer, answer_now, PagesEnv};
use notchling_engine::pages::PageLibrary;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

const LINK: &str = "https://claude.ai/code/artifact/AbCdEfGhIjKlMnOpQrStUv";
const OTHER_LINK: &str = "https://claude.ai/artifact/ZZZZZZZZZZZZZZZZZZZZZZ";

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Dir {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("nl-hp-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work")).unwrap();
    std::fs::write(
        dir.join("work/report.html"),
        "<title>Cost Report</title><p>numbers</p>",
    )
    .unwrap();
    Dir(dir)
}

fn env(dir: &Path) -> PagesEnv {
    PagesEnv {
        pages_dir: dir.join("data/pages"),
        home: dir.to_path_buf(),
        data_dir: dir.join("data"),
        config_dir: None,
        deadline: Duration::from_secs(5),
    }
}

fn publish(dir: &Path) -> Value {
    json!({
        "session_id": "s1",
        "hook_event_name": "PostToolUse",
        "tool_name": "Artifact",
        "cwd": dir.join("work"),
        "tool_input": {"file_path": "report.html", "title": "Cost Report"},
        "tool_response": {"output": format!("Published report.html to {LINK} (version id 1793506664-65e3)")},
    })
}

fn read(event: &str, url: &str, response: Option<&str>) -> Value {
    let mut e = json!({
        "session_id": "s1",
        "hook_event_name": event,
        "tool_name": "Artifact",
        "cwd": "/w/proj",
        "tool_input": {"action": "read", "url": url},
    });
    if let Some(r) = response {
        e["tool_response"] = json!(r);
    }
    e
}

fn pages(env: &PagesEnv) -> Vec<notchling_engine::search::PageEntry> {
    PageLibrary::read_only(&env.pages_dir).entries()
}

#[test]
fn a_publish_event_is_recorded_into_the_library_and_says_nothing() {
    let dir = scratch("record");
    let env = env(&dir.0);
    assert_eq!(answer(publish(&dir.0), env.clone()), None);
    let saved = pages(&env);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].title, "Cost Report");
    assert_eq!(saved[0].url, LINK);
    assert_eq!(saved[0].version, "1793506664-65e3");
    assert_eq!(saved[0].project, "work");
    let copy = env.pages_dir.join(&saved[0].folder).join(&saved[0].file);
    assert!(std::fs::read_to_string(copy).unwrap().contains("numbers"));
    assert!(env.pages_dir.join("index.html").is_file());
    assert_eq!(answer(publish(&dir.0), env.clone()), None);
    assert_eq!(pages(&env).len(), 1);
}

#[test]
fn a_publish_is_saved_under_the_account_in_use() {
    let dir = scratch("account");
    std::fs::write(
        dir.0.join(".claude.json"),
        r#"{"oauthAccount":{"emailAddress":"main@example.com"}}"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.0.join("data")).unwrap();
    std::fs::write(
        dir.0.join("data/lanes.json"),
        r#"[{"name":"w","path":"/lanes/w","accountID":"a","email":"lane@example.com"}]"#,
    )
    .unwrap();
    let main = env(&dir.0);
    answer(publish(&dir.0), main.clone());
    assert_eq!(pages(&main)[0].account, "main@example.com");

    let lane_dir = scratch("account-lane");
    std::fs::create_dir_all(lane_dir.0.join("data")).unwrap();
    std::fs::copy(
        dir.0.join("data/lanes.json"),
        lane_dir.0.join("data/lanes.json"),
    )
    .unwrap();
    let mut lane = env(&lane_dir.0);
    lane.config_dir = Some("/lanes/w".into());
    answer(publish(&lane_dir.0), lane.clone());
    assert_eq!(pages(&lane)[0].account, "lane@example.com");
}

#[test]
fn a_read_of_a_saved_link_is_denied_with_the_local_copys_path() {
    let dir = scratch("deny");
    let env = env(&dir.0);
    answer(publish(&dir.0), env.clone());
    let out = answer(read("PreToolUse", LINK, None), env.clone()).unwrap();
    let h = &out["hookSpecificOutput"];
    assert_eq!(h["hookEventName"], "PreToolUse");
    assert_eq!(h["permissionDecision"], "deny");
    let reason = h["permissionDecisionReason"].as_str().unwrap();
    let saved = &pages(&env)[0];
    let copy = env.pages_dir.join(&saved.folder).join(&saved.file);
    assert!(
        reason.contains(&format!("Local copy: {}", copy.display())),
        "{reason}"
    );
    assert!(reason.contains("this is not an error"));
    let same = LINK.replace("/code/artifact/", "/artifact/");
    assert!(answer(read("PreToolUse", &same, None), env.clone()).is_some());
    let mut one_file = read("PreToolUse", LINK, None);
    one_file["tool_input"]["path"] = json!("index.html");
    assert_eq!(answer(one_file, env), None);
}

#[test]
fn a_link_we_do_not_keep_passes() {
    let dir = scratch("unsaved");
    let env = env(&dir.0);
    answer(publish(&dir.0), env.clone());
    assert_eq!(
        answer(read("PreToolUse", OTHER_LINK, None), env.clone()),
        None
    );
    let empty = scratch("unsaved-empty");
    let none = self::env(&empty.0);
    assert_eq!(answer(read("PreToolUse", LINK, None), none.clone()), None);
    assert!(!none.pages_dir.exists());
}

#[test]
fn a_failed_read_gets_the_local_copy_as_context() {
    let dir = scratch("fallback");
    let env = env(&dir.0);
    answer(publish(&dir.0), env.clone());
    for event in [
        read("PostToolUseFailure", LINK, None),
        read("PostToolUse", LINK, Some("HTTP 404: Not found")),
    ] {
        let name = event["hook_event_name"].clone();
        let out = answer(event, env.clone()).unwrap();
        let h = &out["hookSpecificOutput"];
        assert_eq!(h["hookEventName"], name);
        let context = h["additionalContext"].as_str().unwrap();
        assert!(
            context.starts_with("Notchling keeps a local copy"),
            "{context}"
        );
        assert!(context.contains("Local copy: "));
        assert!(h.get("permissionDecision").is_none());
    }
    assert_eq!(
        answer(
            read("PostToolUse", LINK, Some("<html>fine</html>")),
            env.clone()
        ),
        None
    );
    assert_eq!(
        answer(read("PostToolUseFailure", OTHER_LINK, None), env),
        None
    );
}

#[test]
fn events_that_are_not_ours_pass_and_touch_nothing() {
    let dir = scratch("other");
    let env = env(&dir.0);
    for event in [
        json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "ls"}}),
        json!({"hook_event_name": "PostToolUse", "tool_name": "Read", "tool_response": "404 not found"}),
        json!({"hook_event_name": "PermissionRequest", "tool_name": "Artifact"}),
        json!({"hook_event_name": "PreToolUse", "tool_name": "AskUserQuestion"}),
    ] {
        assert_eq!(answer_now(&event, &env), None, "{event}");
    }
    assert!(!env.pages_dir.exists());
}

#[test]
fn a_library_that_cannot_be_written_is_a_silent_pass() {
    let dir = scratch("readonly");
    let mut env = env(&dir.0);
    std::fs::create_dir_all(dir.0.join("data")).unwrap();
    std::fs::write(dir.0.join("data/pages"), "not a folder").unwrap();
    assert_eq!(answer(publish(&dir.0), env.clone()), None);
    env.pages_dir = dir.0.join("data/library");
    let mut gone = publish(&dir.0);
    gone["tool_input"]["file_path"] = json!("missing.html");
    assert_eq!(answer(gone, env.clone()), None);
    assert!(pages(&env).is_empty());
}
