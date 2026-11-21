use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::inbox_ops_tests::item;
use crate::limits_ops_tests::{rig as limits_rig, Claude, NoJobs, TempDir, NOW};
use crate::switches::{SwitchFlag, ACCOUNTS, ACCOUNTS_OFF};
use notchling_engine::limits::mode::{LimitMode, CLAUDE};
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
    rig_seen(tag, registry, None)
}

fn rig_seen(tag: &str, registry: ProviderRegistry, claude_at: Option<f64>) -> Rig {
    let (limits_tmp, flag, room, limits) = limits_rig();
    if let Some(percent) = claude_at {
        let claude = Arc::new(Claude::default());
        *claude.me.lock().unwrap() = percent;
        limits
            .config()
            .set_mode(&CLAUDE, LimitMode::Notify)
            .unwrap();
        limits.set_claude(claude, Arc::new(NoJobs));
        flag.set(true);
        assert!(limits.tick_at(NOW));
    }
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
fn a_session_a_running_job_resumed_in_another_organization_is_not_shown_to_the_first() {
    let rig = rig("inbox-resumed");
    let job = json!({
        "id": "job-3", "resumeSessionId": "s3", "cwd": format!("{}/y", rig.other_dir),
        "target": {"job": "j3"}, "group": "working", "label": "Working",
    });
    if let Some(rows) = rig.ops.sessions.lock().unwrap().as_array_mut() {
        rows.push(job);
    }
    let _bound = bind_identity(Some(who(true, "s1", Some(&rig.work))));
    let reply = read(&rig, ops::INBOX_MINE, json!({}));
    assert_eq!(reply["organization"], json!([]));
    assert_eq!(reply["otherOrganizations"], json!(2));
    assert!(!reply.to_string().contains("MATE-"));
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

fn claude_registry(active: &str, needs_login: bool) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    let mut me = ProviderAccount::new("me", "me@mine.test", None);
    me.needs_login = needs_login;
    let state = ProviderState {
        accounts: vec![me, ProviderAccount::new("other", "other@mine.test", None)],
        active_id: Some(active.into()),
        ..ProviderState::default()
    };
    registry.register(Arc::new(Planted {
        id: ProviderId::CLAUDE_CODE,
        state,
    }));
    registry
}

fn claude_limits(rig: &Rig) -> Value {
    let _bound = bind_identity(Some(who(true, "s2", Some("personal"))));
    read(rig, ops::LIMITS_SUMMARY, json!({"tool": "claude"}))["tool"].clone()
}

#[test]
fn claude_with_no_reading_yet_is_usable_for_its_own_organization() {
    let rig = rig_seen("claude-none", claude_registry("me", false), None);
    let tool = claude_limits(&rig);
    assert_eq!(tool["usable"], json!(true), "{tool}");
    assert_eq!(tool["reason"], Value::Null, "{tool}");
}

#[test]
fn claude_uses_the_reading_of_the_active_account_only() {
    let seen = rig_seen("claude-same", claude_registry("me", false), Some(99.0));
    let tool = claude_limits(&seen);
    assert_eq!(tool["usable"], json!(false), "{tool}");
    assert_eq!(tool["reason"], json!("pool_full"), "{tool}");

    let switched = rig_seen(
        "claude-switched",
        claude_registry("other", false),
        Some(99.0),
    );
    let tool = claude_limits(&switched);
    assert_eq!(
        tool["usable"],
        json!(true),
        "the old account's windows: {tool}"
    );
    assert_eq!(tool["pools"], json!([]), "{tool}");
}

#[test]
fn a_signed_out_claude_account_is_not_usable_with_or_without_a_reading() {
    for (tag, seen) in [("claude-out-seen", Some(10.0)), ("claude-out-none", None)] {
        let rig = rig_seen(tag, claude_registry("me", true), seen);
        let tool = claude_limits(&rig);
        assert_eq!(tool["usable"], json!(false), "{tag}: {tool}");
        assert_eq!(tool["reason"], json!("signed_out"), "{tag}: {tool}");
    }
}

#[test]
fn a_pool_matches_by_exact_name_first_then_by_one_substring_and_never_by_two() {
    let pools = json!({"usable": true, "reason": null, "pools": [
        {"name": "Gemini Flash", "usable": true, "reason": null},
        {"name": "Gemini", "usable": false, "reason": "pool_full"},
        {"name": "Claude and GPT", "usable": true, "reason": null},
    ]});
    assert_eq!(pick_pool(&pools, "gemini")["reason"], json!("pool_full"));
    assert_eq!(pick_pool(&pools, "GEMINI")["name"], json!("Gemini"));
    assert_eq!(pick_pool(&pools, "claude")["name"], json!("Claude and GPT"));
    let two = json!({"pools": [{"name": "Gemini Flash"}, {"name": "Gemini Pro"}]});
    assert_eq!(pick_pool(&two, "gemini")["reason"], json!("ambiguous_pool"));
    assert_eq!(pick_pool(&two, "gemini")["usable"], json!(false));
    assert_eq!(pick_pool(&pools, "nope")["reason"], json!("unknown_pool"));
}

#[test]
fn a_pool_of_a_tool_with_no_pools_gives_the_tools_own_verdict() {
    let none_yet = json!({"usable": true, "reason": null, "pools": []});
    let verdict = pick_pool(&none_yet, "claude");
    assert_eq!(verdict["usable"], json!(true));
    assert_eq!(verdict["reason"], Value::Null);
    let theirs = json!({"usable": null, "reason": "other_organization", "pools": []});
    let verdict = pick_pool(&theirs, "claude");
    assert_eq!(verdict["usable"], json!(false));
    assert_eq!(verdict["reason"], json!("other_organization"));
}

#[test]
fn limits_with_a_pool_passes_the_reason_through_for_a_shell_or_another_organization() {
    let rig = rig_with("pool-through", planted_registry());
    assign_work(&rig);
    let args = json!({"tool": "agy", "pool": "claude"});
    let bound = bind_identity(Some(who(true, "s1", Some(&rig.work))));
    let reply = read(&rig, ops::LIMITS_SUMMARY, args.clone());
    assert_eq!(
        reply["pool"]["reason"],
        json!("other_organization"),
        "{reply}"
    );
    drop(bound);
    let _bound = bind_identity(None);
    let reply = read(&rig, ops::LIMITS_SUMMARY, args);
    assert_eq!(reply["pool"]["reason"], json!("no_session"), "{reply}");
    assert_eq!(reply["tool"]["reason"], json!("no_session"), "{reply}");
}

#[test]
fn a_row_with_an_empty_folder_is_dropped_and_never_placed_in_personal() {
    let mut reply = json!({
        "pages": [{"title": "unplaced", "folder": ""}, {"title": "placed", "folder": "/a"}],
        "sessions": [{"title": "unplaced", "folder": "", "modified": 1.0}],
    });
    scope_search(&mut reply, Some("personal"), &json!({}), &|_| {
        "personal".into()
    });
    assert_eq!(reply["pages"].as_array().unwrap().len(), 1);
    assert_eq!(reply["pages"][0]["title"], json!("placed"));
    assert_eq!(reply["sessions"], json!([]));
}
