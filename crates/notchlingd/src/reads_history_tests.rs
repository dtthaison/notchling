use super::*;
use std::time::{Duration, SystemTime};

fn plant(rig: &Rig, folder: &str, id: &str, cwd: &str, text: &str, age_s: u64) {
    let dir = rig.home.join(".claude/projects").join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{id}.jsonl"));
    std::fs::write(
        &path,
        format!(
            "{{\"type\":\"user\",\"cwd\":\"{cwd}\",\"message\":{{\"role\":\"user\",\"content\":\"{text}\"}}}}\n"
        ),
    )
    .unwrap();
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs_f64(NOW - age_s as f64))
        .unwrap();
}

fn history(rig: &Rig, args: Value) -> Value {
    read(rig, ops::SESSIONS_HISTORY, args)
}

fn ids(reply: &Value) -> Vec<String> {
    reply["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect()
}

fn rigged(tag: &str) -> Rig {
    let rig = rig(tag);
    let (work, other) = (
        format!("{}/x", rig.work_dir),
        format!("{}/y", rig.other_dir),
    );
    plant(&rig, "-w-a", "mine-new", &work, "Fix the sidebar", 100);
    plant(
        &rig,
        "-w-a",
        "mine-old",
        &work,
        "Write the notes for c.doe@work-corp.com",
        5_000,
    );
    plant(&rig, "-w-a", "mine-stale", &work, "Too old", 20 * 86_400);
    plant(&rig, "-w-b", "theirs", &other, "FAR-SECRET-TITLE", 200);
    plant(&rig, "-w-a", "live-one", &work, "Already a live row", 300);
    *rig.ops.sessions.lock().unwrap() = json!([
        {"id": "s1", "cwd": work, "label": "Idle", "transcriptPath": "/p/live-one.jsonl"},
        {"id": "gone-1", "cwd": work, "label": "Exited", "name": "Ended job", "sinceMs": NOW as i64 * 1000 - 60_000},
        {"id": "gone-far", "cwd": other, "label": "Exited", "name": "FAR-ENDED", "sinceMs": NOW as i64 * 1000 - 70_000},
    ]);
    rig
}

#[cfg(unix)]
#[test]
fn history_without_a_verified_session_is_empty_with_the_reason() {
    let rig = rigged("hist-none");
    for identity in [None, Some(who(false, "s1", Some(&rig.work)))] {
        let _bound = bind_identity(identity);
        let reply = history(&rig, json!({}));
        assert_eq!(reply["reason"], json!("no_session"));
        assert_eq!(reply["sessions"], json!([]));
        assert!(!reply.to_string().contains("FAR-"));
    }
}

#[cfg(unix)]
#[test]
fn history_lists_the_ended_rows_then_this_organizations_last_14_days_masked() {
    let rig = rigged("hist-mine");
    let _bound = bind_identity(Some(who(true, "s1", Some(&rig.work))));
    let reply = history(&rig, json!({}));
    assert_eq!(ids(&reply), ["gone-1", "mine-new", "mine-old"]);
    assert_eq!(reply["sessions"][0]["kind"], json!("ended"));
    assert_eq!(reply["sessions"][0]["title"], json!("Ended job"));
    assert_eq!(reply["sessions"][1]["kind"], json!("past"));
    assert_eq!(reply["sessions"][1]["title"], json!("Fix the sidebar"));
    assert_eq!(
        reply["sessions"][2]["title"],
        json!("Write the notes for c***@w***.com")
    );
    assert_eq!(reply["masked"], json!(true));
    assert_eq!(reply["window_days"], json!(14));
    assert_eq!(reply["organization"], json!(rig.work));
    assert_eq!(reply["next"], Value::Null);
    let text = reply.to_string();
    for planted in ["FAR-", "work-corp.com", "mine-stale", "Too old"] {
        assert!(!text.contains(planted), "leaked {planted}");
    }
}

#[cfg(unix)]
#[test]
fn history_pages_by_cursor_and_refuses_a_bad_one() {
    let rig = rigged("hist-page");
    let _bound = bind_identity(Some(who(true, "s1", Some(&rig.work))));
    let first = history(&rig, json!({"limit": 2}));
    assert_eq!(ids(&first), ["gone-1", "mine-new"]);
    let cursor = first["next"].as_str().expect("a next cursor");
    let second = history(&rig, json!({"limit": 2, "before": cursor}));
    assert_eq!(ids(&second), ["mine-old"]);
    assert_eq!(second["next"], Value::Null);
    for args in [
        json!({"limit": 0}),
        json!({"limit": 101}),
        json!({"limit": "x"}),
        json!({"before": "nonsense"}),
        json!({"before": 5}),
    ] {
        assert!(rig
            .ops
            .call(&caller(), ops::SESSIONS_HISTORY, &args)
            .is_err());
    }
}
