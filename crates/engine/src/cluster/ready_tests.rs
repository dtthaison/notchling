use super::ready::{annotate, assess, checks_in, ready, CheckKind, HostFacts, MIN_FREE_MB};
use super::testkit::{id, member};
use super::Member;
use crate::versions::{Tool, Version};
use serde_json::{json, Value};

fn facts(encrypted: Option<bool>) -> HostFacts {
    HostFacts {
        encrypted,
        home: "/Users/son".to_string(),
        user: "son".to_string(),
        free_mb: 200_000,
    }
}

fn master(name: &str, host: Option<HostFacts>) -> Member {
    let mut m = member(name, "macos", true);
    m.can_be_master = true;
    m.versions.as_mut().expect("versions").host = host;
    m
}

fn primary() -> Member {
    master("m5", Some(facts(Some(true))))
}

#[test]
fn encrypted_matching_standby_is_ready() {
    let standby = master("m1", Some(facts(Some(true))));
    assert_eq!(ready(&standby, &primary(), 0), Ok(()));
}

#[test]
fn unencrypted_is_refused_with_its_name() {
    let standby = master("m1", Some(facts(Some(false))));
    assert_eq!(
        ready(&standby, &primary(), 0),
        Err("m1 has no disk encryption".to_string())
    );
}

#[test]
fn unknown_encryption_is_not_ready() {
    let standby = master("m1", Some(facts(None)));
    let why = ready(&standby, &primary(), 0).expect_err("not ready");
    assert!(why.contains("m1") && why.contains("encryption"), "{why}");
}

#[test]
fn a_member_that_reported_nothing_is_not_ready() {
    let standby = master("m1", None);
    assert!(ready(&standby, &primary(), 0).is_err());
}

#[test]
fn different_home_path_fails() {
    let mut other = facts(Some(true));
    other.home = "/Users/other".to_string();
    let standby = master("m1", Some(other));
    let why = ready(&standby, &primary(), 0).expect_err("not ready");
    assert!(
        why.contains("/Users/other") && why.contains("/Users/son"),
        "{why}"
    );
}

#[test]
fn different_user_fails() {
    let mut other = facts(Some(true));
    other.user = "other".to_string();
    let standby = master("m1", Some(other));
    assert!(ready(&standby, &primary(), 0).is_err());
}

#[test]
fn low_space_fails() {
    let mut low = facts(Some(true));
    low.free_mb = MIN_FREE_MB - 1;
    let standby = master("m1", Some(low));
    let why = ready(&standby, &primary(), 0).expect_err("not ready");
    assert!(why.contains("m1") && why.contains("free"), "{why}");
}

#[test]
fn space_must_be_twice_the_last_set() {
    let mut tight = facts(Some(true));
    tight.free_mb = 10_000;
    let standby = master("m1", Some(tight));
    assert_eq!(ready(&standby, &primary(), 4_999), Ok(()));
    assert!(ready(&standby, &primary(), 5_001).is_err());
}

#[test]
fn tool_versions_must_equal_the_primarys() {
    let mut standby = master("m1", Some(facts(Some(true))));
    standby
        .versions
        .as_mut()
        .expect("versions")
        .tools
        .insert(Tool::Claude, Version::parse("2.1.1").expect("version"));
    let why = ready(&standby, &primary(), 0).expect_err("not ready");
    assert!(
        why.contains("Claude Code") && why.contains("2.1.1"),
        "{why}"
    );
}

#[test]
fn a_missing_tool_is_not_ready_with_a_reason() {
    let mut standby = master("m1", Some(facts(Some(true))));
    standby
        .versions
        .as_mut()
        .expect("versions")
        .tools
        .remove(&Tool::Codex);
    let why = ready(&standby, &primary(), 0).expect_err("not ready");
    assert!(why.contains("m1") && why.contains("Codex"), "{why}");
}

#[test]
fn can_be_master_is_inert_until_ready() {
    let mut standby = master("m1", Some(facts(Some(false))));
    assert!(ready(&standby, &primary(), 0).is_err());
    standby.can_be_master = false;
    let why = ready(&standby, &primary(), 0).expect_err("not ready");
    assert!(why.contains("Can Be Master"), "{why}");
    standby.can_be_master = true;
    standby.versions.as_mut().expect("versions").host = Some(facts(Some(true)));
    assert_eq!(ready(&standby, &primary(), 0), Ok(()));
}

#[test]
fn first_failing_check_is_the_reason() {
    let mut bad = facts(Some(false));
    bad.free_mb = 1;
    let standby = master("m1", Some(bad));
    assert_eq!(
        ready(&standby, &primary(), 0),
        Err("m1 has no disk encryption".to_string())
    );
}

fn status_of(members: &[&Member], home: &Member) -> Value {
    json!({
        "cluster": {"home": home.id},
        "me": home.id,
        "members": members.iter().map(|m| json!({
            "id": m.id,
            "name": m.name,
            "canBeMaster": m.can_be_master,
            "state": "active",
            "versions": m.versions,
        })).collect::<Vec<_>>(),
    })
}

#[test]
fn annotate_adds_readiness_to_can_be_master_members_only() {
    let home = primary();
    let ok = master("m1", Some(facts(Some(true))));
    let bad = master("m2", Some(facts(Some(false))));
    let worker = member("w1", "linux", false);
    let mut status = status_of(&[&home, &ok, &bad, &worker], &home);
    annotate(&mut status, 0);
    let members = status["members"].as_array().expect("members");
    let by = |name: &str| {
        members
            .iter()
            .find(|m| m["name"] == name)
            .expect("member")
            .clone()
    };
    assert_eq!(by("m1")["ready"]["ok"], true);
    assert_eq!(by("m1")["ready"]["reason"], Value::Null);
    assert_eq!(by("m2")["ready"]["ok"], false);
    assert_eq!(by("m2")["ready"]["reason"], "m2 has no disk encryption");
    assert_eq!(by("m2")["ready"]["checks"][0]["id"], "encrypted");
    assert_eq!(by("m2")["ready"]["checks"][0]["ok"], false);
    assert!(by("w1").get("ready").is_none());
}

#[test]
fn assess_without_a_primary_is_not_ready() {
    let ok = master("m1", Some(facts(Some(true))));
    let mut status = status_of(&[&ok], &ok);
    status["cluster"]["home"] = json!(id("nobody"));
    let found = assess(&status, 0);
    let a = found.get(&ok.id).expect("assessed");
    assert!(a.ready.is_err());
}

#[test]
fn check_kinds_are_in_a_fixed_order() {
    assert_eq!(
        CheckKind::ALL.map(CheckKind::key),
        ["encrypted", "home", "space", "tools"]
    );
}

#[test]
fn checks_read_back_from_the_status_a_machine_card_gets() {
    let home = primary();
    let bad = master("m2", Some(facts(Some(false))));
    let off = member("m3", "macos", false);
    let mut status = status_of(&[&home, &bad, &off], &home);
    annotate(&mut status, 0);
    let got = checks_in(&status, bad.id.as_str()).expect("checks");
    assert_eq!(got.len(), 4);
    assert_eq!(
        got[0],
        (
            CheckKind::Encrypted,
            Err("m2 has no disk encryption".to_string())
        )
    );
    assert!(got[1].1.is_ok() && got[2].1.is_ok() && got[3].1.is_ok());
    assert_eq!(checks_in(&status, off.id.as_str()), None);
    assert_eq!(checks_in(&status, "nobody"), None);
}
