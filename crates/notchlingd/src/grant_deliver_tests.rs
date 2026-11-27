use super::*;
use crate::cluster_redact::{mask_tail, JobMasks};
use crate::grant_wiring::GrantParts;

fn deliver(rig: &Rig, args: &Value) -> Result<Value, ApiError> {
    rig.uses.deliver(&from_vault(), args)
}

#[cfg(unix)]
#[test]
fn deliver_for_a_job_the_state_lists_elsewhere_or_finished_is_refused() {
    let rig = rig();
    let other_node = job("j-aaaaaaaaaaaa", "other", JobState::Running);
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[other_node]));
    let args = deliver_args("j-aaaaaaaaaaaa", "api-key", "API_TOKEN", "NL-MARKER-1");
    let err = deliver(&rig, &args).expect_err("a job on another node");
    assert!(err.message.contains("isn't running on this machine"));
    assert!(!err.message.contains("NL-MARKER"), "no value in the error");
    let done = job(JOB, "worker", JobState::Done);
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[done]));
    let args = deliver_args(JOB, "api-key", "API_TOKEN", "NL-MARKER-1");
    let err = deliver(&rig, &args).expect_err("a finished job");
    assert!(err.message.contains("isn't running"));
    for gone in ["j-aaaaaaaaaaaa", JOB] {
        assert!(rig
            .store
            .get(gone, "api-key", real_now())
            .unwrap()
            .is_none());
        assert_eq!(rig.masks.state(gone), JobMasks::Never);
    }
    assert!(rig.marks.list().is_empty(), "no mark for a refused grant");
    assert!(
        !rig.roots.is_pending(JOB, real_now()),
        "nothing held pending"
    );
}

#[cfg(unix)]
#[test]
fn deliver_stores_masks_and_marks_the_job() {
    let rig = rig();
    let args = deliver_args(JOB, "API-Key", "API_TOKEN", "NL-MARKER-1");
    assert_eq!(deliver(&rig, &args).expect("stored")["stored"], "api-key");
    let got = rig
        .store
        .get(JOB, "api-key", real_now())
        .unwrap()
        .expect("a grant file");
    let env = decode_env(&got.value).expect("env");
    assert!(
        env["API_TOKEN"].expose() == "NL-MARKER-1",
        "the stored value"
    );
    assert_eq!(rig.masks.state(JOB), JobMasks::Masked);
    assert_eq!(rig.marks.list(), [JOB.to_string()]);
    let (out, _) = mask_tail(&rig.masks, JOB, 0, &[], b"x NL-MARKER-1 y", true).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(!text.contains("NL-MARKER"), "the value is masked");
    assert!(text.contains("[secret api-key]"));
}

#[cfg(unix)]
#[test]
fn deliver_clamps_the_expiry_to_twelve_hours() {
    let rig = rig();
    let mut args = deliver_args(JOB, "api-key", "API_TOKEN", "NL-MARKER-1");
    args["expiresMs"] = json!(real_now() + 30 * 24 * 3_600_000);
    deliver(&rig, &args).expect("stored");
    let near = real_now() + GRANT_TTL_MS - 60_000;
    let far = real_now() + GRANT_TTL_MS + 60_000;
    assert!(rig.store.get(JOB, "api-key", near).unwrap().is_some());
    assert!(rig.store.get(JOB, "api-key", far).unwrap().is_none());
    let mut gone = deliver_args(JOB, "api-key", "API_TOKEN", "NL-MARKER-1");
    gone["expiresMs"] = json!(real_now() - 1);
    assert!(deliver(&rig, &gone).is_err(), "already expired");
}

#[cfg(unix)]
#[test]
fn deliver_is_taken_only_from_the_vault_node_while_it_is_home() {
    let rig = rig();
    let args = deliver_args(JOB, "api-key", "API_TOKEN", "NL-MARKER-1");
    let from = |node: &str, tier| Origin {
        node: id(node),
        tier,
    };
    for origin in [
        from("other", OriginTier::Node),
        from("worker", OriginTier::Node),
        from("vault", OriginTier::Agent),
        from("vault", OriginTier::App),
    ] {
        let err = rig.uses.deliver(&origin, &args).expect_err("refused");
        assert_eq!(err.code, ErrorCode::Forbidden);
    }
    *rig.fake.state.lock().unwrap() =
        Some(cluster("other", &[job(JOB, "worker", JobState::Running)]));
    assert!(deliver(&rig, &args).is_err(), "the vault node isn't home");
    assert!(rig.store.get(JOB, "api-key", real_now()).unwrap().is_none());
    assert!(rig.marks.list().is_empty());
}

#[cfg(unix)]
#[test]
fn a_malformed_or_oversize_value_is_refused_whole() {
    let rig = rig();
    let mut args = deliver_args(JOB, "api-key", "API_TOKEN", "NL-MARKER-1");
    args["value"] = json!("not json NL-MARKER-1");
    let err = deliver(&rig, &args).expect_err("malformed");
    assert!(!err.message.contains("NL-MARKER"), "no value in the error");
    let big = deliver_args(JOB, "api-key", "API_TOKEN", &"x".repeat(40_000));
    assert!(deliver(&rig, &big).is_err(), "too big to mask");
    assert!(rig.store.get(JOB, "api-key", real_now()).unwrap().is_none());
    assert!(rig.marks.list().is_empty(), "no marker for a refused value");
    assert_eq!(
        rig.masks.state(JOB),
        JobMasks::Never,
        "the job stays served"
    );
    let empty = deliver_args(JOB, "api-key", "API_TOKEN", "");
    deliver(&rig, &empty).expect("nothing to mask");
    assert!(
        rig.marks.list().is_empty(),
        "no marker with nothing to mask"
    );
    assert_eq!(rig.masks.state(JOB), JobMasks::Never);
    for bad in [
        json!({"job": JOB, "name": "api-key", "value": 3, "expiresMs": 1}),
        json!({"job": "../x", "name": "api-key", "value": "{}", "expiresMs": 1}),
        json!({"job": JOB, "name": "a b", "value": "{}", "expiresMs": 1}),
    ] {
        assert_eq!(
            deliver(&rig, &bad).expect_err("bad").code,
            ErrorCode::BadRequest
        );
    }
}

#[cfg(unix)]
#[test]
fn revoke_deletes_at_once() {
    let rig = rig();
    for (name, value) in [("one", "NL-MARKER-1"), ("two", "NL-MARKER-2")] {
        deliver(&rig, &deliver_args(JOB, name, "V", value)).expect("stored");
    }
    deliver(&rig, &deliver_args(OTHER_JOB, "one", "V", "NL-MARKER-3")).expect("stored");
    let some = json!({"job": JOB, "names": ["one"]});
    rig.uses.revoke(&from_vault(), &some).expect("revoked");
    assert!(rig.store.get(JOB, "one", real_now()).unwrap().is_none());
    assert!(rig.store.get(JOB, "two", real_now()).unwrap().is_some());
    rig.uses
        .revoke(&from_vault(), &json!({"job": JOB}))
        .expect("revoked");
    assert!(rig.store.get(JOB, "two", real_now()).unwrap().is_none());
    assert!(
        rig.store
            .get(OTHER_JOB, "one", real_now())
            .unwrap()
            .is_some(),
        "another job keeps its grant"
    );
    assert_eq!(rig.masks.state(JOB), JobMasks::Masked, "masks only grow");
}

#[cfg(unix)]
#[test]
fn revoke_is_taken_from_the_home_or_the_vault_node_only() {
    let rig = rig();
    deliver(&rig, &deliver_args(JOB, "one", "V", "NL-MARKER-1")).expect("stored");
    let args = json!({"job": JOB});
    for node in ["worker", "other"] {
        let origin = Origin {
            node: id(node),
            tier: OriginTier::Node,
        };
        assert!(rig.uses.revoke(&origin, &args).is_err(), "{node} refused");
    }
    let app = Origin {
        node: id("vault"),
        tier: OriginTier::App,
    };
    assert!(rig.uses.revoke(&app, &args).is_err());
    assert!(rig.store.get(JOB, "one", real_now()).unwrap().is_some());
    let bad = json!({"job": JOB, "names": [1]});
    assert!(rig.uses.revoke(&from_vault(), &bad).is_err());
}

#[cfg(unix)]
#[test]
fn job_end_deletes_grants() {
    let rig = rig();
    deliver(&rig, &deliver_args(JOB, "one", "V", "NL-MARKER-1")).expect("stored");
    let parts = GrantParts {
        store: rig.store.clone(),
        masks: rig.masks.clone(),
        marks: rig.marks.clone(),
        roots: rig.roots.clone(),
    };
    rig.roots.set(JOB, 500);
    let shell = parts.shell();
    (shell.reaped)(JOB);
    assert!(rig.masks.state(JOB) == JobMasks::Masked);
    (shell.ended)(JOB);
    assert!(rig.store.get(JOB, "one", real_now()).unwrap().is_none());
    assert!(rig.roots.is_ended(JOB));
    assert!(rig.masks.is_ended(JOB));
    assert_eq!(
        rig.masks.state(JOB),
        JobMasks::Masked,
        "masks outlive the job"
    );
    let again = deliver(&rig, &deliver_args(JOB, "two", "V", "NL-MARKER-2"));
    assert!(again.is_err(), "no grant after the job ended");
    assert!(rig.store.get(JOB, "two", real_now()).unwrap().is_none());
    assert_eq!(rig.marks.list(), [JOB.to_string()], "the mark stays");
}

#[cfg(unix)]
#[test]
fn after_a_restart_a_job_that_had_grants_is_not_served() {
    let rig = rig();
    deliver(&rig, &deliver_args(JOB, "one", "V", "NL-MARKER-1")).expect("stored");
    let restarted = GrantParts::new(rig.dir.path());
    assert_eq!(restarted.masks.state(JOB), JobMasks::Gone);
    let err =
        mask_tail(&restarted.masks, JOB, 0, &[], b"some output", false).expect_err("not served");
    assert!(err.contains(JOB));
    assert_eq!(restarted.masks.state(OTHER_JOB), JobMasks::Never);
    let (out, _) = mask_tail(&restarted.masks, OTHER_JOB, 0, &[], b"plain", false).unwrap();
    assert_eq!(out, b"plain");
}

#[cfg(unix)]
#[test]
fn a_mark_that_cannot_be_written_refuses_the_grant() {
    let rig = rig();
    std::fs::write(
        rig.dir.path().join(crate::grant_roots::MARKS_DIR),
        b"in the way",
    )
    .unwrap();
    let err = deliver(&rig, &deliver_args(JOB, "one", "V", "NL-MARKER-1")).expect_err("refused");
    assert!(!err.message.contains("NL-MARKER"));
    assert!(rig.store.get(JOB, "one", real_now()).unwrap().is_none());
    assert_eq!(rig.masks.state(JOB), JobMasks::Never);
}

#[cfg(unix)]
#[test]
fn no_marker_value_reaches_any_debug_or_error_text() {
    let rig = rig();
    deliver(&rig, &deliver_args(JOB, "one", "V", "NL-MARKER-1")).expect("stored");
    rig.roots.set(JOB, 500);
    let out = answer_text(&rig);
    assert!(!out.contains("NL-MARKER"), "debug output hides values");
    let grants = rig.store.get(JOB, "one", real_now()).unwrap().unwrap();
    assert!(!format!("{grants:?}").contains("NL-MARKER"));
    assert!(!format!("{:?}", rig.masks).contains("NL-MARKER"));
}

#[cfg(unix)]
#[test]
fn a_mark_records_the_keys_that_find_the_jobs_rows() {
    let rig = rig_running_both();
    deliver(&rig, &deliver_args(JOB, "one", "V", "NL-MARKER-1")).expect("stored");
    let shell = rig.marks.marked();
    assert_eq!(
        shell[0].keys,
        [format!("shell:{JOB}")],
        "a shell job's row id"
    );
    let rig = rig_for_claude("c1");
    deliver(&rig, &deliver_args(JOB, "one", "V", "NL-MARKER-1")).expect("stored");
    assert_eq!(rig.marks.marked()[0].keys, ["c1".to_string()]);
    let restarted = GrantParts::new(rig.dir.path());
    assert_eq!(
        restarted.masks.job_for_key("c1"),
        Some(JOB.to_string()),
        "a restart finds the job by its session"
    );
}

#[cfg(unix)]
#[test]
fn a_mark_for_a_claude_job_not_yet_linked_has_no_keys() {
    let rig = rig_for_claude("");
    deliver(&rig, &deliver_args(JOB, "one", "V", "NL-MARKER-1")).expect("stored");
    assert!(rig.marks.marked()[0].keys.is_empty());
}

fn rig_for_claude(session: &str) -> Rig {
    let rig = rig();
    let mut claude = job(JOB, "worker", JobState::Running);
    claude.spec.tool = Tool::Claude;
    claude.session = (!session.is_empty()).then(|| session.to_string());
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[claude]));
    rig
}

fn answer_text(rig: &Rig) -> String {
    let got = rig
        .uses
        .use_secrets(&caller(700), &json!({"names": ["one"]}))
        .expect("path")
        .expect("answered");
    let env = got["env"].as_object().expect("env");
    assert!(env["V"] == "NL-MARKER-1", "the value reaches the caller");
    format!("{:?} {:?}", rig.masks, rig.marks.list())
}
