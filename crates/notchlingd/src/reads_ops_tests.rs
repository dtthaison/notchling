use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::inbox_ops_tests::item;
use crate::limits_ops_tests::{rig as limits_rig, TempDir, NOW};
use crate::switches::{SwitchFlag, ACCOUNTS, ACCOUNTS_OFF};
use notchling_engine::providers::{
    AccountError, AccountProvider, AddProgress, AddStarted, Capabilities, ProviderAccount,
    ProviderRegistry, ProviderState,
};
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

struct Planted {
    id: &'static str,
    state: ProviderState,
}

impl AccountProvider for Planted {
    fn id(&self) -> ProviderId {
        ProviderId::new(self.id)
    }
    fn title(&self) -> &'static str {
        "Planted"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::ADD_ACCOUNT
    }
    fn state(&self) -> ProviderState {
        self.state.clone()
    }
    fn sync(&self, _force: bool) {}
    fn switch_to(&self, _id: &str) -> Result<(), AccountError> {
        Ok(())
    }
    fn begin_add(&self) -> Result<AddStarted, AccountError> {
        Ok(AddStarted::default())
    }
    fn poll_add(&self) -> AddProgress {
        AddProgress::Idle
    }
    fn cancel_add(&self) {}
    fn remove(&self, _id: &str) {}
    fn switch_failure_title(&self) -> &'static str {
        "Couldn't switch"
    }
}

fn planted_registry() -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    let claude = ProviderState {
        accounts: vec![
            ProviderAccount::new("acc-work", "alice@work-corp.com", Some("max")),
            ProviderAccount::new("acc-far", "bob@far-secret.com", Some("pro")),
        ],
        active_id: Some("acc-work".into()),
        sign_in_url: Some("https://login.example/TOKEN-SECRET".into()),
        ..ProviderState::default()
    };
    registry.register(Arc::new(Planted {
        id: ProviderId::CLAUDE_CODE,
        state: claude,
    }));
    let codex = ProviderState {
        accounts: vec![ProviderAccount::new(
            "cdx-far",
            "carol@far-secret.com",
            None,
        )],
        active_id: Some("cdx-far".into()),
        ..ProviderState::default()
    };
    registry.register(Arc::new(Planted {
        id: ProviderId::CODEX,
        state: codex,
    }));
    registry
}

struct Rig {
    _tmp: TempDir,
    _limits_tmp: TempDir,
    work: String,
    work_dir: String,
    other_dir: String,
    ops: ReadsOps,
}

fn rig(tag: &str) -> Rig {
    rig_with(tag, ProviderRegistry::new())
}

fn rig_with(tag: &str, registry: ProviderRegistry) -> Rig {
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
        registry,
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
    let (work_dir, other_dir) = (
        work_dir.display().to_string(),
        other_dir.display().to_string(),
    );
    Rig {
        _tmp: tmp,
        _limits_tmp: limits_tmp,
        work,
        work_dir,
        other_dir,
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

const PLANTED_OF_WORK: [&str; 6] = [
    "alice@work-corp.com",
    "work-corp",
    "acc-work",
    "OWN-",
    "MATE-",
    "TOKEN-SECRET",
];

const PLANTED_OF_FAR: [&str; 8] = [
    "bob@far-secret.com",
    "carol@far-secret.com",
    "far-secret",
    "acc-far",
    "cdx-far",
    "FAR-",
    "TOKEN-SECRET",
    "bob@",
];

fn assign_work(rig: &Rig) {
    let data = rig._tmp.path("data");
    notchling_engine::store::put_doc(
        &data,
        Kind::Claude.doc(),
        r#"[{"id":"acc-work"},{"id":"acc-far"}]"#,
    )
    .unwrap();
    Organizations::new(&data)
        .with_home(&rig._tmp.path("home"))
        .set_organization(Kind::Claude, "acc-work", &rig.work)
        .unwrap();
}

fn every_read(rig: &Rig) -> Vec<(&'static str, String)> {
    let mut all = Vec::new();
    for op in READ_OPS {
        all.push((op, read(rig, op, json!({})).to_string()));
    }
    for tool in ["claude", "codex", "agy"] {
        all.push((
            "limits.summary",
            read(rig, ops::LIMITS_SUMMARY, json!({"tool": tool, "pool": "x"})).to_string(),
        ));
    }
    all
}

#[test]
fn a_planted_email_token_folder_and_inbox_text_of_another_organization_appear_in_no_reply() {
    let rig = rig_with("planted-far", planted_registry());
    assign_work(&rig);
    let _bound = bind_identity(Some(who(true, "s1", Some(&rig.work))));
    let mut seen_account = false;
    for (op, text) in every_read(&rig) {
        for planted in PLANTED_OF_FAR {
            assert!(!text.contains(planted), "{op} leaked {planted}: {text}");
        }
        assert!(
            !text.contains(&rig.other_dir),
            "{op} leaked a folder: {text}"
        );
        seen_account |= text.contains("a***@w***.com");
    }
    assert!(seen_account, "the caller's own masked account is shown");
}

#[test]
fn a_planted_email_token_folder_and_inbox_text_of_the_first_organization_appear_in_no_other_reply()
{
    let rig = rig_with("planted-work", planted_registry());
    assign_work(&rig);
    let _bound = bind_identity(Some(who(true, "s2", Some("personal"))));
    for (op, text) in every_read(&rig) {
        for planted in PLANTED_OF_WORK {
            assert!(!text.contains(planted), "{op} leaked {planted}: {text}");
        }
        assert!(
            !text.contains(&rig.work_dir),
            "{op} leaked a folder: {text}"
        );
        assert!(
            !text.contains("Work"),
            "{op} leaked the organization name: {text}"
        );
    }
}

#[test]
fn another_sessions_inbox_text_in_the_same_organization_is_a_title_only() {
    let rig = rig("inbox-mate");
    let _bound = bind_identity(Some(who(true, "s3", Some(&rig.work))));
    let text = read(&rig, ops::INBOX_MINE, json!({})).to_string();
    assert!(text.contains("MATE-DETAIL-SECRET"));
    assert!(!text.contains("OWN-DETAIL"), "{text}");
    assert!(text.contains("OWN-TITLE"));
}

#[test]
fn a_planted_search_row_of_another_organization_or_for_a_shell_is_dropped() {
    let rig = rig("search-planted");
    let reply = || {
        json!({
            "pages": [
                {"title": "mine page", "folder": format!("{}/x", rig.work_dir)},
                {"title": "far page bob@far-secret.com TOKEN-SECRET", "folder": format!("{}/y", rig.other_dir)},
            ],
            "sessions": [
                {"title": "mine", "folder": format!("{}/x", rig.work_dir), "modified": 1.0},
                {"title": "far session FAR-DETAIL-SECRET", "folder": format!("{}/y", rig.other_dir), "modified": 1.0},
            ],
        })
    };
    let org_of = |f: &str| rig.ops.folder_org(f);
    let bound = bind_identity(Some(who(true, "s1", Some(&rig.work))));
    let mut mine = reply();
    let org = rig.ops.search_org(&caller());
    assert_eq!(org.as_deref(), Some(rig.work.as_str()));
    scope_search(&mut mine, org.as_deref(), &json!({}), &org_of);
    let text = mine.to_string();
    assert!(text.contains("mine page"));
    for planted in ["far-secret", "TOKEN-SECRET", "FAR-"] {
        assert!(!text.contains(planted), "{planted}");
    }
    drop(bound);
    for identity in [None, Some(who(false, "s1", Some(&rig.work)))] {
        let _bound = bind_identity(identity);
        let mut shell = reply();
        let org = rig.ops.search_org(&caller());
        assert_eq!(org, None);
        scope_search(&mut shell, org.as_deref(), &json!({}), &org_of);
        assert_eq!(shell["reason"], json!("no_session"));
        let text = shell.to_string();
        for planted in ["mine page", "far-secret", "personal"] {
            assert!(!text.contains(planted), "{planted}");
        }
    }
}

#[test]
fn a_shell_with_planted_accounts_still_gets_nothing_from_any_read() {
    let rig = rig_with("planted-shell", planted_registry());
    for identity in [None, Some(who(false, "s1", Some("personal")))] {
        let _bound = bind_identity(identity);
        for (op, text) in every_read(&rig) {
            assert!(text.contains("no_session"), "{op}: {text}");
            for planted in PLANTED_OF_FAR.iter().chain(PLANTED_OF_WORK.iter()) {
                assert!(!text.contains(planted), "{op} leaked {planted}: {text}");
            }
            assert!(!text.contains("personal"), "{op}: {text}");
        }
    }
}
