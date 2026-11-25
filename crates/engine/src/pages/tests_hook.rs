use super::hook::*;
use super::owner::Outcome;
use super::tests::{ctx, URL_A};
use super::*;
use crate::sessions::testutil::TempDir;
use serde_json::{json, Value};

const PRE_READ_SAVED: &str = include_str!("fixtures/pre-read-saved.json");
const PRE_READ_ONE_FILE: &str = include_str!("fixtures/pre-read-one-file.json");
const PRE_READ_UNSAVED: &str = include_str!("fixtures/pre-read-unsaved.json");
const POST_PUBLISH: &str = include_str!("fixtures/post-publish.json");
const POST_FAILURE_READ: &str = include_str!("fixtures/post-failure-read.json");
const POST_READ_404: &str = include_str!("fixtures/post-read-404.json");
const POST_READ_OK: &str = include_str!("fixtures/post-read-ok.json");

fn event(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn library_with_page(dir: &TempDir) -> PageLibrary {
    dir.write(
        "work/report.html",
        "<title>Cost Report</title><p>numbers</p>",
    );
    let lib = PageLibrary::writable(dir.path("pages"));
    let work = dir.path("work");
    let text = POST_PUBLISH.replace("WORK", &work.to_string_lossy());
    let saved = lib
        .record(&event(&text), &ctx("owner@example.com", dir.root()))
        .unwrap();
    assert_eq!(
        saved.map(|e| e.url),
        Some(URL_A.replace("/artifact/", "/code/artifact/"))
    );
    lib
}

#[test]
fn only_the_artifact_tool_events_are_handled() {
    assert!(handles(&event(PRE_READ_SAVED)));
    assert!(handles(&event(POST_PUBLISH)));
    assert!(handles(&event(POST_FAILURE_READ)));
    assert!(!handles(
        &json!({"tool_name": "Bash", "hook_event_name": "PreToolUse"})
    ));
    assert!(!handles(
        &json!({"tool_name": "Artifact", "hook_event_name": "PermissionRequest"})
    ));
    assert!(!handles(&json!({})));
}

#[test]
fn a_publish_event_is_recorded_and_says_nothing() {
    let dir = TempDir::new("hook-publish");
    let lib = library_with_page(&dir);
    let page = lib.entries().remove(0);
    assert_eq!(page.version, "1793506664-65e3");
    assert_eq!(page.account, "owner@example.com");
    let again = event(&POST_PUBLISH.replace("WORK", &dir.path("work").to_string_lossy()));
    assert_eq!(post_tool_use(&lib, &again, &ctx("x", dir.root())), None);
}

#[test]
fn a_read_of_a_saved_page_is_answered_from_the_copy() {
    let dir = TempDir::new("hook-read");
    let lib = library_with_page(&dir);
    let out = pre_tool_use(&lib, &event(PRE_READ_SAVED), None).unwrap();
    let h = &out["hookSpecificOutput"];
    assert_eq!(h["hookEventName"], "PreToolUse");
    assert_eq!(h["permissionDecision"], "deny");
    let reason = h["permissionDecisionReason"].as_str().unwrap();
    assert!(reason.contains("this is not an error"));
    assert!(reason.contains("Local copy: "));
    assert!(reason.contains("Published by: owner@example.com"));
    assert!(reason.contains("path \"index.html\""));
}

#[test]
fn a_read_that_is_not_for_a_saved_whole_page_passes() {
    let dir = TempDir::new("hook-pass");
    let lib = library_with_page(&dir);
    assert_eq!(pre_tool_use(&lib, &event(PRE_READ_ONE_FILE), None), None);
    assert_eq!(pre_tool_use(&lib, &event(PRE_READ_UNSAVED), None), None);
    let mut publish = event(PRE_READ_SAVED);
    publish["tool_input"] = json!({"file_path": "x.html", "url": URL_A});
    assert_eq!(pre_tool_use(&lib, &publish, None), None);
    let page = lib.entries().remove(0);
    std::fs::remove_file(lib.local_path(&page).unwrap()).unwrap();
    assert_eq!(pre_tool_use(&lib, &event(PRE_READ_SAVED), None), None);
    assert!(local_read(&lib, &event(PRE_READ_SAVED)).is_none());
}

#[test]
fn an_owner_publish_outcome_decides_the_pre_hook() {
    let dir = TempDir::new("hook-owner");
    let lib = library_with_page(&dir);
    let e = event(PRE_READ_SAVED);
    for outcome in [Outcome::Done("done".into()), Outcome::Refused("no".into())] {
        let out = pre_tool_use(&lib, &e, Some(outcome.clone())).unwrap();
        assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "deny");
        let reason = match outcome {
            Outcome::Done(r) | Outcome::Refused(r) => r,
            Outcome::Failed(r) => r,
        };
        assert_eq!(
            out["hookSpecificOutput"]["permissionDecisionReason"],
            json!(reason)
        );
    }
    let out = pre_tool_use(&lib, &e, Some(Outcome::Failed("why".into()))).unwrap();
    assert_eq!(
        out,
        json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "additionalContext": "why"}})
    );
}

#[test]
fn a_failed_read_is_pointed_at_the_local_copy() {
    let dir = TempDir::new("hook-fail");
    let lib = library_with_page(&dir);
    for fixture in [POST_FAILURE_READ, POST_READ_404] {
        let e = event(fixture);
        let name = e["hook_event_name"].as_str().unwrap().to_string();
        let out = post_tool_use(&lib, &e, &ctx("x", dir.root())).unwrap();
        let h = &out["hookSpecificOutput"];
        assert_eq!(h["hookEventName"], json!(name));
        let context = h["additionalContext"].as_str().unwrap();
        assert!(context.starts_with("Notchling keeps a local copy"));
        assert!(context.contains("Local copy: "));
        assert!(context.contains("from another account, publishing makes a new link"));
        assert!(h.get("permissionDecision").is_none());
    }
    assert_eq!(
        post_tool_use(&lib, &event(POST_READ_OK), &ctx("x", dir.root())),
        None
    );
    let mut unsaved = event(POST_FAILURE_READ);
    unsaved["tool_input"]["url"] = json!("https://claude.ai/artifact/ZZZZZZZZZZZZZZZZZZZZZZ");
    assert_eq!(post_tool_use(&lib, &unsaved, &ctx("x", dir.root())), None);
}

#[test]
fn a_read_only_library_stays_silent_and_unchanged() {
    let dir = TempDir::new("hook-ro");
    library_with_page(&dir);
    let ro = PageLibrary::read_only(dir.path("pages"));
    let before = std::fs::read(ro.list_path()).unwrap();
    let publish = event(&POST_PUBLISH.replace("WORK", &dir.path("work").to_string_lossy()));
    assert_eq!(post_tool_use(&ro, &publish, &ctx("x", dir.root())), None);
    assert_eq!(std::fs::read(ro.list_path()).unwrap(), before);
    assert!(pre_tool_use(&ro, &event(PRE_READ_SAVED), None).is_some());
}

#[test]
fn the_outputs_have_the_shape_claude_code_reads() {
    assert_eq!(
        deny_output("r"),
        json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": "r"}})
    );
    assert_eq!(
        context_output("PostToolUseFailure", "c"),
        json!({"hookSpecificOutput": {"hookEventName": "PostToolUseFailure", "additionalContext": "c"}})
    );
}

#[test]
fn answer_runs_the_event_by_its_name_and_leaves_the_rest_alone() {
    let dir = TempDir::new("hook-answer");
    let lib = library_with_page(&dir);
    let c = ctx("x", dir.root());
    let out = answer(&lib, &event(PRE_READ_SAVED), &c).unwrap();
    assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(out["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap()
        .contains("Local copy: "));
    assert_eq!(answer(&lib, &event(PRE_READ_UNSAVED), &c), None);
    for fixture in [POST_FAILURE_READ, POST_READ_404] {
        let out = answer(&lib, &event(fixture), &c).unwrap();
        assert!(out["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .starts_with("Notchling keeps a local copy"));
    }
    let publish = event(&POST_PUBLISH.replace("WORK", &dir.path("work").to_string_lossy()));
    assert_eq!(answer(&lib, &publish, &c), None);
    assert_eq!(answer(&lib, &event(POST_READ_OK), &c), None);
    for other in [
        json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "ls"}}),
        json!({"hook_event_name": "PermissionRequest", "tool_name": "Artifact"}),
        json!({"hook_event_name": "PostToolUse", "tool_name": "Read", "tool_response": "404 not found"}),
        json!({}),
    ] {
        assert_eq!(answer(&lib, &other, &c), None, "{other}");
    }
}

#[test]
fn only_a_post_tool_use_that_may_publish_needs_the_account() {
    assert!(may_publish(&event(POST_PUBLISH)));
    let mut explicit = event(POST_PUBLISH);
    explicit["tool_input"]["action"] = json!("publish");
    assert!(may_publish(&explicit));
    assert!(!may_publish(&event(POST_READ_404)));
    assert!(!may_publish(&event(POST_FAILURE_READ)));
    assert!(!may_publish(&event(PRE_READ_SAVED)));
    assert!(!may_publish(&json!({})));
}
