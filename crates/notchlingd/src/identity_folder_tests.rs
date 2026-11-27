use super::job_tests::{identify, identify_in, rig, JOB_SESSION, SESSION};
use crate::identity::folder_in;
use serde_json::{json, Value};

fn job(id: &str, short: &str, resume: &str, cwd: &str, group: &str, label: &str) -> Value {
    json!({
        "id": id, "cwd": cwd, "resumeSessionId": resume,
        "target": {"job": short}, "group": group, "label": label,
    })
}

#[cfg(unix)]
#[test]
fn a_finished_job_that_once_resumed_the_session_never_lends_its_folder() {
    let r = rig("stale", json!({"sessionId": SESSION}));
    let in_acme = r.work.join("app").to_string_lossy().into_owned();
    let in_beta = r.other.join("app").to_string_lossy().into_owned();
    std::fs::write(
        r.home.0.join(".claude/sessions/800.json"),
        json!({"sessionId": SESSION, "cwd": in_beta}).to_string(),
    )
    .unwrap();
    let rows = json!([job("old-job", "j0", SESSION, &in_acme, "completed", "Done")]);
    let who = identify(&r, rows);
    assert_eq!(who.folder.as_deref(), Some(in_beta.as_str()));
    assert_eq!(who.organization.as_deref(), Some(r.beta.as_str()));
}

#[cfg(unix)]
#[test]
fn two_jobs_that_resumed_the_same_session_each_get_their_own_folder_by_job_id() {
    let r = rig("twins", json!({"sessionId": SESSION}));
    let in_acme = r.work.join("app").to_string_lossy().into_owned();
    let in_beta = r.other.join("app").to_string_lossy().into_owned();
    std::fs::write(
        r.home.0.join(".claude/sessions/800.json"),
        json!({"sessionId": SESSION, "cwd": "/launch", "jobId": "j2"}).to_string(),
    )
    .unwrap();
    let rows = json!([
        job("job-1", "j1", SESSION, &in_acme, "working", "Working"),
        job("job-2", "j2", SESSION, &in_beta, "working", "Working"),
    ]);
    let who = identify(&r, rows);
    assert_eq!(who.folder.as_deref(), Some(in_beta.as_str()));
    assert_eq!(who.organization.as_deref(), Some(r.beta.as_str()));
}

#[cfg(unix)]
#[test]
fn several_running_jobs_on_one_resumed_session_are_no_match_without_a_job_id() {
    let r = rig("many", json!({"sessionId": SESSION}));
    let in_acme = r.work.join("app").to_string_lossy().into_owned();
    let in_beta = r.other.join("app").to_string_lossy().into_owned();
    let rows = json!([
        job("job-1", "j1", SESSION, &in_acme, "working", "Working"),
        job("job-2", "j2", SESSION, &in_beta, "completed", "Idle"),
    ]);
    let who = identify(&r, rows);
    assert!(who.folder.is_none());
    assert!(who.organization.is_none());
}

#[cfg(unix)]
#[test]
fn a_row_with_an_empty_folder_does_not_hide_the_running_job_that_has_one() {
    let r = rig("stub", json!({"sessionId": SESSION, "cwd": "/launch"}));
    let in_acme = r.work.join("app").to_string_lossy().into_owned();
    let rows = json!([
        {"id": SESSION, "cwd": ""},
        job(JOB_SESSION, "j1", SESSION, &in_acme, "working", "Working"),
    ]);
    let who = identify(&r, rows);
    assert_eq!(who.folder.as_deref(), Some(in_acme.as_str()));
    assert_eq!(who.organization.as_deref(), Some(r.acme.as_str()));
}

#[cfg(unix)]
#[test]
fn with_no_row_and_no_file_folder_the_process_working_folder_decides() {
    let r = rig("live", json!({"sessionId": SESSION}));
    let in_beta = r.other.join("app");
    let who = identify_in(&r, json!([]), Some(in_beta.clone()));
    assert_eq!(
        who.folder.as_deref(),
        Some(in_beta.to_string_lossy().as_ref())
    );
    assert_eq!(who.organization.as_deref(), Some(r.beta.as_str()));
}

#[test]
fn one_single_function_picks_the_folder_for_any_session() {
    let rows = json!([
        {"id": "a", "cwd": ""},
        job("job-a", "ja", "a", "/in/a", "completed", "Idle"),
        job("job-b", "jb", "b", "/in/b", "completed", "Done"),
        job("job-c", "jc", "c", "/in/c1", "working", "Working"),
        job("job-d", "jd", "c", "/in/c2", "needsInput", "Needs input"),
    ]);
    assert_eq!(folder_in(&rows, "a", None).as_deref(), Some("/in/a"));
    assert_eq!(folder_in(&rows, "b", None), None);
    assert_eq!(folder_in(&rows, "c", None), None);
    assert_eq!(folder_in(&rows, "c", Some("jd")).as_deref(), Some("/in/c2"));
    assert_eq!(folder_in(&rows, "c", Some("jz")), None);
    assert_eq!(folder_in(&rows, "a", Some("jb")), None);
    assert_eq!(folder_in(&rows, "a", Some("ja")).as_deref(), Some("/in/a"));
}

fn unlisted_job(tag: &str) -> (super::job_tests::Rig, String, Value) {
    let r = rig(
        tag,
        json!({"sessionId": SESSION, "cwd": "/launch", "jobId": "j2"}),
    );
    let in_acme = r.work.join("app").to_string_lossy().into_owned();
    let rows = json!([
        {"id": SESSION, "cwd": in_acme},
        job("job-1", "j1", SESSION, &in_acme, "working", "Working"),
    ]);
    (r, in_acme, rows)
}

#[cfg(unix)]
#[test]
fn a_job_the_list_lacks_borrows_no_other_row_and_not_its_launch_folder() {
    let (r, _, rows) = unlisted_job("unlisted");
    let who = identify(&r, rows);
    assert!(who.verified);
    assert!(who.folder.is_none());
    assert!(who.organization.is_none());
    let alone = identify(&r, json!([]));
    assert!(alone.folder.is_none());
    assert!(alone.organization.is_none());
}

#[cfg(unix)]
#[test]
fn a_job_the_list_lacks_takes_its_live_process_folder() {
    let (r, _, rows) = unlisted_job("unlisted-live");
    let in_beta = r.other.join("app");
    let who = identify_in(&r, rows, Some(in_beta.clone()));
    assert_eq!(
        who.folder.as_deref(),
        Some(in_beta.to_string_lossy().as_ref())
    );
    assert_eq!(who.organization.as_deref(), Some(r.beta.as_str()));
}

#[cfg(unix)]
#[test]
fn a_running_resume_row_with_no_folder_yet_leaves_the_one_with_a_folder_unique() {
    let r = rig("blank", json!({"sessionId": SESSION, "cwd": "/launch"}));
    let in_acme = r.work.join("app").to_string_lossy().into_owned();
    let rows = json!([
        job("job-0", "j0", SESSION, "", "working", "Working"),
        job(JOB_SESSION, "j1", SESSION, &in_acme, "working", "Working"),
    ]);
    let who = identify(&r, rows);
    assert_eq!(who.folder.as_deref(), Some(in_acme.as_str()));
    assert_eq!(who.organization.as_deref(), Some(r.acme.as_str()));
}

#[cfg(unix)]
#[test]
fn a_job_row_of_another_session_never_lends_its_folder_to_the_jobs_caller() {
    let r = rig("foreign", json!({"sessionId": SESSION, "jobId": "j1"}));
    let in_acme = r.work.join("app").to_string_lossy().into_owned();
    let in_beta = r.other.join("app").to_string_lossy().into_owned();
    let rows = json!([
        job("job-x", "j1", "another-session", &in_acme, "working", "Working"),
        {"id": SESSION, "cwd": in_beta},
    ]);
    let who = identify(&r, rows);
    assert!(who.verified);
    assert!(who.folder.is_none());
    assert!(who.organization.is_none());
}
