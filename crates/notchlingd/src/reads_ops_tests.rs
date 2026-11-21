use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::inbox_ops_tests::item;
use crate::limits_ops_tests::{rig as limits_rig, TempDir, NOW};
use crate::switches::{SwitchFlag, ACCOUNTS, ACCOUNTS_OFF};
use notchling_engine::providers::ProviderRegistry;
use notchling_local_api::{bind_identity, ClientKind};

fn caller() -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid: Some(1),
    }
}

fn who(verified: bool, session: &str, org: Option<&str>) -> AgentIdentity {
    let mut who = AgentIdentity::shell();
    who.verified = verified;
    who.session = verified.then(|| session.to_string());
    who.organization = org.map(str::to_string);
    who
}

struct Rig {
    _tmp: TempDir,
    _limits_tmp: TempDir,
    work: String,
    ops: ReadsOps,
}

fn rig(tag: &str) -> Rig {
    let (limits_tmp, _flag, room, limits) = limits_rig();
    let tmp = TempDir::new(tag);
    let home = tmp.path("home");
    let work_dir = home.join("work");
    let other_dir = home.join("other");
    std::fs::create_dir_all(work_dir.join("x")).unwrap();
    std::fs::create_dir_all(other_dir.join("y")).unwrap();
    let data = tmp.path("data");
    std::fs::create_dir_all(&data).unwrap();
    let orgs = Organizations::new(&data).with_home(&home);
    let work = orgs.create("Work", None, None).unwrap();
    orgs.set_folders(&work, &[work_dir.display().to_string()])
        .unwrap();
    let sessions = Arc::new(Mutex::new(json!([
        {"id": "s1", "cwd": work_dir.join("x").display().to_string()},
        {"id": "s3", "cwd": work_dir.join("x").display().to_string()},
        {"id": "s2", "cwd": other_dir.join("y").display().to_string()},
    ])));
    let inbox = InboxOps::new(tmp.path("inbox"), Arc::new(FakeRoom::default()));
    for (id, session, title, detail) in [
        ("own", "s1", "OWN-TITLE", "OWN-DETAIL"),
        ("mate", "s3", "MATE-TITLE", "MATE-DETAIL-SECRET"),
        ("far", "s2", "FAR-TITLE-SECRET", "FAR-DETAIL-SECRET"),
    ] {
        let mut it = item(id);
        it.title = title.into();
        it.detail = detail.into();
        it.source.session = Some(session.into());
        inbox.raise(it, None);
    }
    let providers = Arc::new(ProvidersOps::new(
        SwitchFlag::off(ACCOUNTS, ACCOUNTS_OFF),
        ProviderRegistry::new(),
        room,
    ));
    let ops = ReadsOps::new(
        orgs,
        &home,
        sessions,
        inbox,
        providers,
        Arc::new(limits),
        tmp.path("quota"),
        Arc::new(|| NOW),
    );
    Rig {
        _tmp: tmp,
        _limits_tmp: limits_tmp,
        work,
        ops,
    }
}

fn read(rig: &Rig, op: &str, args: Value) -> Value {
    rig.ops.call(&caller(), op, &args).unwrap()
}

#[test]
fn an_email_is_masked_to_its_first_letters() {
    assert_eq!(mask_email("alice@example.com"), "a***@e***.com");
    assert_eq!(mask_email("not an email"), "");
}

#[test]
fn search_keeps_only_the_callers_organization_and_drops_unplaced_rows() {
    let mut reply = json!({
        "pages": [{"title": "mine", "folder": "/a/x"}, {"title": "theirs", "folder": "/b/y"}, {"title": "none"}],
        "sessions": [{"title": "mine", "folder": "/a/x", "modified": 10.0}, {"title": "theirs", "folder": "/b/y", "modified": 10.0}],
    });
    let org_of = |f: &str| if f.starts_with("/a") { "work" } else { "other" }.to_string();
    scope_search(&mut reply, Some("work"), &json!({}), &org_of);
    assert_eq!(reply["pages"].as_array().unwrap().len(), 1);
    assert_eq!(reply["sessions"].as_array().unwrap().len(), 1);
    assert!(!reply.to_string().contains("theirs"));
}

#[test]
fn search_for_a_caller_with_no_session_is_empty_with_the_reason() {
    let mut reply = json!({"pages": [{"folder": "/a/x"}], "sessions": [{"folder": "/a/x"}]});
    scope_search(&mut reply, None, &json!({}), &|_| "personal".to_string());
    assert_eq!(reply["pages"], json!([]));
    assert_eq!(reply["sessions"], json!([]));
    assert_eq!(reply["reason"], json!("no_session"));
}

#[test]
fn search_kind_and_since_narrow_the_answer() {
    let mut reply = json!({
        "pages": [{"folder": "/a"}],
        "sessions": [{"folder": "/a", "modified": 5.0}, {"folder": "/a", "modified": 50.0}],
    });
    scope_search(
        &mut reply,
        Some("o"),
        &json!({"kind": "sessions", "since": 10.0}),
        &|_| "o".into(),
    );
    assert_eq!(reply["pages"], json!([]));
    assert_eq!(reply["sessions"].as_array().unwrap().len(), 1);
}

#[test]
fn a_shell_or_unverified_caller_gets_nothing_and_no_personal_data() {
    let rig = rig("shell");
    for identity in [
        None,
        Some(who(false, "s1", None)),
        Some(who(false, "s1", Some("personal"))),
        Some(who(true, "s1", None)),
    ] {
        let _bound = bind_identity(identity);
        for op in READ_OPS {
            let reply = read(&rig, op, json!({}));
            assert_eq!(reply["reason"], json!("no_session"), "{op}");
            let text = reply.to_string();
            for planted in ["OWN-", "MATE-", "FAR-", "personal"] {
                assert!(!text.contains(planted), "{op} leaked {planted}");
            }
        }
    }
}

#[test]
fn a_verified_session_sees_its_own_items_in_full_and_its_organization_by_title() {
    let rig = rig("inbox");
    let _bound = bind_identity(Some(who(true, "s1", Some(&rig.work))));
    let reply = read(&rig, ops::INBOX_MINE, json!({}));
    assert_eq!(reply["own"].as_array().unwrap().len(), 1);
    assert_eq!(reply["own"][0]["detail"], json!("OWN-DETAIL"));
    assert_eq!(reply["organization"].as_array().unwrap().len(), 1);
    assert_eq!(reply["organization"][0]["title"], json!("MATE-TITLE"));
    assert_eq!(reply["otherOrganizations"], json!(1));
    let text = reply.to_string();
    assert!(!text.contains("MATE-DETAIL"));
    assert!(!text.contains("FAR-"));
}

#[test]
fn another_organizations_session_sees_none_of_the_first_ones_text() {
    let rig = rig("inbox-other");
    let _bound = bind_identity(Some(who(true, "s2", Some("personal"))));
    let reply = read(&rig, ops::INBOX_MINE, json!({}));
    assert_eq!(reply["own"][0]["detail"], json!("FAR-DETAIL-SECRET"));
    let text = reply.to_string();
    for planted in ["OWN-", "MATE-"] {
        assert!(!text.contains(planted), "leaked {planted}");
    }
    assert_eq!(reply["otherOrganizations"], json!(2));
}

#[test]
fn organizations_mine_names_only_the_callers_and_counts_the_rest() {
    let rig = rig("orgs");
    let _bound = bind_identity(Some(who(true, "s1", Some(&rig.work))));
    let reply = read(&rig, ops::ORGANIZATIONS_MINE, json!({}));
    assert_eq!(reply["organization"]["name"], json!("Work"));
    assert_eq!(reply["others"], json!(1));
    assert!(!reply.to_string().contains("Personal"));
}

#[test]
fn accounts_and_limits_answer_for_a_verified_session() {
    let rig = rig("acc");
    let _bound = bind_identity(Some(who(true, "s1", Some(&rig.work))));
    let reply = read(&rig, ops::ACCOUNTS_SUMMARY, json!({}));
    assert_eq!(reply["organization"], json!(rig.work));
    let reply = read(&rig, ops::LIMITS_SUMMARY, json!({"tool": "codex"}));
    assert!(reply["tool"].get("usable").is_some());
    let reply = read(&rig, ops::LIMITS_SUMMARY, json!({"tool": "nope"}));
    assert_eq!(reply["tool"]["reason"], json!("unknown_tool"));
}
