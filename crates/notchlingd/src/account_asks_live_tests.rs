use super::*;
use crate::account_asks::follow::{AccountFollow, AccountTap};
use crate::account_asks::AccountAsks;
use crate::accounts_ops_tests::{world, World, ID1, ID2};
use crate::acting_ops_tests::{sessions, setup, FakeRoom, PROMPT};
use crate::consent_ops::ConsentOps;
use crate::consent_testkit::{agent_caller, app_caller, shell, verified};
use crate::op_group::OpGroup;
use crate::providers_ops_tests::codex_login;
use notchling_engine::accounts::codex::{CodexDeps, CodexPaths, CodexStore};
use notchling_engine::accounts::fake::{PanicHttp, PanicKeychain};
use notchling_engine::accounts::generic_password::FakeKeychain;
use notchling_engine::accounts::procs::{FakeApps, FakeProcs, ProcCall};
use notchling_engine::consent::ConsentStore;
use notchling_engine::providers::{
    AccountError, AccountProvider, AddProgress, AddStarted, Capabilities, FakeHttp,
    ProviderAccount, ProviderRegistry, ProviderState,
};
use notchling_local_api::{ops, AgentIdentity};
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Tool(&'static str, Capabilities);

impl AccountProvider for Tool {
    fn id(&self) -> ProviderId {
        ProviderId::new(self.0)
    }
    fn title(&self) -> &'static str {
        "Tool"
    }
    fn capabilities(&self) -> Capabilities {
        self.1
    }
    fn state(&self) -> ProviderState {
        ProviderState::default()
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

fn tools() -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(Tool(ProviderId::CODEX, Capabilities::ADD_ACCOUNT)));
    registry.register(Arc::new(Tool(ProviderId::ANTIGRAVITY, Capabilities::USAGE)));
    registry
}

struct Rig {
    w: World,
    book: LiveBook,
    orgs: Organizations,
    consent: Arc<ConsentOps>,
    asks: Arc<AccountAsks>,
    who: Arc<Mutex<AgentIdentity>>,
    acme: String,
    other: String,
    acme_dir: PathBuf,
    far_dir: PathBuf,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.consent.close();
    }
}

fn rig(tag: &str) -> Rig {
    live(tag, |_| tools())
}

fn live(tag: &str, registry: impl FnOnce(&Path) -> ProviderRegistry) -> Rig {
    let w = world(tag, true, Arc::new(PanicKeychain), Arc::new(PanicHttp));
    let data = w.d.0.join("data");
    let home = w.d.0.join("home");
    let acme_dir = home.join("acme");
    let far_dir = home.join("far");
    std::fs::create_dir_all(acme_dir.join("x")).unwrap();
    std::fs::create_dir_all(far_dir.join("y")).unwrap();
    notchling_engine::store::put_doc(
        &data,
        Kind::Claude.doc(),
        &json!([
            {"id": ID1, "email": "one@example.com", "plan": "Pro", "needsLogin": false},
            {"id": ID2, "email": "two@example.com", "plan": "Pro", "needsLogin": false},
            {"id": "a3a3:0303", "email": "three@example.com", "plan": "Pro", "organization": "zz-gone"}
        ])
        .to_string(),
    )
    .unwrap();
    let orgs = Organizations::new(&data).with_home(&home);
    let acme = orgs.create("Acme", None, None).unwrap();
    let other = orgs.create("Other", None, None).unwrap();
    orgs.set_folders(&acme, &[acme_dir.display().to_string()])
        .unwrap();
    orgs.set_organization(Kind::Claude, ID1, &acme).unwrap();
    orgs.set_organization(Kind::Claude, ID2, &other).unwrap();
    let lanes = [
        ("alpha-secret-lane", ID1, "one@example.com"),
        ("beta-secret-lane", ID2, "two@example.com"),
        ("gamma-secret-lane", "a3a3:0303", "three@example.com"),
    ]
    .map(|(name, id, email)| notchling_engine::sessions::Lane {
        name: name.into(),
        path: home.join("lanes").join(name).display().to_string(),
        account_id: id.into(),
        email: email.into(),
        folder: None,
    });
    w.ops.store().env().lanes.save(&lanes).unwrap();
    let room = Arc::new(FakeRoom::default());
    let follow = AccountFollow::new(Arc::new(crate::acting_ops::now_ms));
    let tap = AccountTap::new(room.clone(), follow.clone());
    let providers = Arc::new(ProvidersOps::new(w.flag.clone(), registry(&data), tap));
    providers.set_add_timing(Duration::from_millis(5), Duration::from_secs(30));
    let acting = Arc::new(setup(true, PROMPT, sessions()).ops);
    let book = LiveBook::new(
        w.ops.store(),
        providers.clone(),
        acting.clone(),
        orgs.clone(),
        &data.join("daemon"),
        w.flag.clone(),
        &home,
    );
    let live = LiveBook::new(
        w.ops.store(),
        providers,
        acting,
        orgs.clone(),
        &data.join("daemon"),
        w.flag.clone(),
        &home,
    );
    let consent = ConsentOps::with_clock(
        Arc::new(ConsentStore::in_memory_for_tests()),
        room,
        None,
        None,
        Arc::new(crate::acting_ops::now_ms),
    );
    let mut me = verified("s-live");
    me.organization = Some(acme.clone());
    let who = Arc::new(Mutex::new(me));
    let seen = who.clone();
    consent.bind_identity(Arc::new(move |_| seen.lock().unwrap().clone()));
    consent.start();
    let asks = AccountAsks::new(consent.clone(), Arc::new(live), follow);
    Rig {
        w,
        book,
        orgs,
        consent,
        asks,
        who,
        acme,
        other,
        acme_dir,
        far_dir,
    }
}

#[cfg(unix)]
#[test]
fn the_owner_of_an_account_comes_from_the_organizations_listing_only() {
    let rig = rig("live-owner");
    let book = &rig.book;
    let listing = book.listing(Kind::Claude).unwrap();
    assert!(listing.iter().any(|(k, _)| k == ID1));
    let field = |id: &str| listing.iter().find(|(k, _)| k == id).unwrap().1.clone();
    let mine = book.resolve(Kind::Claude, ID1, field(ID1).as_deref());
    assert!(mine.known);
    assert_eq!(mine.id, rig.acme);
    let theirs = book.resolve(Kind::Claude, ID2, field(ID2).as_deref());
    assert!(theirs.known);
    assert_eq!(theirs.id, rig.other);
    let lost = book.resolve(Kind::Claude, "a3a3:0303", field("a3a3:0303").as_deref());
    assert!(!lost.known, "an id the document does not know is not known");
    assert!(book.listing(Kind::Codex).is_none());
    assert!(!book.closed());
    assert_eq!(rig.orgs.view().organizations().len(), 3);
}

#[cfg(unix)]
#[test]
fn the_folder_org_places_a_folder_by_the_organizations_folders() {
    let rig = rig("live-folder");
    let inside = rig.acme_dir.join("x").display().to_string();
    let outside = rig.far_dir.join("y").display().to_string();
    assert_eq!(rig.book.folder_org(&inside), rig.acme);
    assert_eq!(
        rig.book.folder_org(&outside),
        notchling_engine::organizations::PERSONAL_ID
    );
    assert_eq!(
        rig.book.home(),
        rig.w.d.0.join("home").display().to_string()
    );
    assert!(rig.book.accounts_on());
    assert!(rig.book.can_add("codex") && !rig.book.can_add("claudeCode"));
    assert!(!rig.book.can_add("antigravity") && !rig.book.can_add("gemini"));
    assert!(rig.book.id("claudeCode", ID1).unwrap().starts_with("acct-"));
    assert!(rig
        .book
        .lane_id("alpha-secret-lane")
        .unwrap()
        .starts_with("lane-"));
}

#[cfg(unix)]
#[test]
fn a_closed_view_has_no_owner() {
    let rig = rig("live-closed");
    notchling_engine::store::put_doc(&rig.w.d.0.join("data"), "organizations", "{").unwrap();
    assert!(rig.book.closed());
}

#[cfg(unix)]
#[test]
fn lanes_summary_lists_only_the_callers_lanes_without_names() {
    let rig = rig("live-lanes");
    let reply = rig
        .asks
        .call(&agent_caller(), ops::LANES_SUMMARY, &json!({}))
        .unwrap();
    assert_eq!(reply["organization"], json!(rig.acme));
    let lanes = reply["lanes"].as_array().unwrap();
    assert_eq!(lanes.len(), 1, "{reply}");
    let lane = &lanes[0];
    assert_eq!(lane["id"], json!(rig.book.lane_id("alpha-secret-lane")));
    assert_eq!(lane["account"], json!(rig.book.id("claudeCode", ID1)));
    assert_eq!(lane["name"], json!("o***@e***.com"));
    assert_eq!(lane["sessions"], json!(0));
    assert_eq!(lane["needsLogin"], json!(false));
    let text = reply.to_string();
    for planted in [
        "secret-lane",
        "one@",
        "two@",
        "three@",
        "example.com",
        ID1,
        ID2,
        "a1a1",
        "email:",
        "/lanes/",
    ] {
        assert!(!text.contains(planted), "{planted} leaked in {text}");
    }
    *rig.who.lock().unwrap() = shell("Terminal");
    let none = rig
        .asks
        .call(&agent_caller(), ops::LANES_SUMMARY, &json!({}))
        .unwrap();
    assert_eq!(none, json!({"reason": "no_session", "lanes": []}));
    let mut elsewhere = verified("s-other");
    elsewhere.organization = Some(rig.other.clone());
    *rig.who.lock().unwrap() = elsewhere;
    let theirs = rig
        .asks
        .call(&agent_caller(), ops::LANES_SUMMARY, &json!({}))
        .unwrap();
    assert_eq!(theirs["lanes"].as_array().unwrap().len(), 1);
    assert_eq!(theirs["lanes"][0]["name"], json!("t***@e***.com"));
    assert!(!theirs.to_string().contains("o***@e***.com"));
}

#[cfg(unix)]
#[test]
fn a_tool_that_cannot_add_accounts_is_refused_before_a_card() {
    let rig = rig("live-can-add");
    let err = rig
        .asks
        .call(
            &agent_caller(),
            ops::ACCOUNTS_REQUEST,
            &json!({"action": "add", "args": {"tool": "agy"}}),
        )
        .unwrap_err();
    assert_eq!(
        err.data.as_ref().unwrap()["reason"],
        json!("bad_value"),
        "{err:?}"
    );
    assert_eq!(err.message, "Notchling can't add accounts for that tool.");
    assert!(rig.consent.store().open_rows().unwrap().is_empty());
    let codex = rig
        .asks
        .call(
            &agent_caller(),
            ops::ACCOUNTS_REQUEST,
            &json!({"action": "add", "args": {"tool": "codex"}}),
        )
        .unwrap();
    assert_eq!(codex["state"], "waiting", "{codex}");
}

struct Signing {
    data: PathBuf,
    accounts: Mutex<Vec<String>>,
    ends_on: Mutex<Option<&'static str>>,
}

impl Signing {
    fn save(&self, id: &str) {
        let mut accounts = self.accounts.lock().unwrap();
        if accounts.iter().any(|known| known == id) {
            return;
        }
        accounts.push(id.to_string());
        let mut root: Value = notchling_engine::store::doc(&self.data, Kind::Codex.doc())
            .ok()
            .flatten()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| json!({"accounts": []}));
        if let Some(list) = root["accounts"].as_array_mut() {
            list.push(json!({"id": id, "email": "new@example.com"}));
        }
        notchling_engine::store::put_doc(&self.data, Kind::Codex.doc(), &root.to_string()).unwrap();
    }
}

impl AccountProvider for Signing {
    fn id(&self) -> ProviderId {
        ProviderId::new(ProviderId::CODEX)
    }
    fn title(&self) -> &'static str {
        "Signing"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::ADD_ACCOUNT
    }
    fn state(&self) -> ProviderState {
        let accounts = self.accounts.lock().unwrap();
        ProviderState {
            accounts: accounts
                .iter()
                .map(|id| ProviderAccount::new(id, "new@example.com", None))
                .collect(),
            ..ProviderState::default()
        }
    }
    fn sync(&self, _force: bool) {}
    fn switch_to(&self, _id: &str) -> Result<(), AccountError> {
        Ok(())
    }
    fn begin_add(&self) -> Result<AddStarted, AccountError> {
        Ok(AddStarted::default())
    }
    fn poll_add(&self) -> AddProgress {
        match self.ends_on.lock().unwrap().take() {
            Some(id) => {
                self.save(id);
                AddProgress::Done
            }
            None => AddProgress::Waiting { sign_in_url: None },
        }
    }
    fn cancel_add(&self) {}
    fn remove(&self, _id: &str) {}
    fn switch_failure_title(&self) -> &'static str {
        "Couldn't switch"
    }
}

fn until(what: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !what() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn asked_add(rig: &Rig) -> String {
    let providers = rig.book.providers.clone();
    let deadline = Instant::now() + Duration::from_secs(5);
    let (request, reply) = loop {
        let asked = rig
            .asks
            .call(
                &agent_caller(),
                ops::ACCOUNTS_REQUEST,
                &json!({"action": "add", "args": {"tool": "codex"}}),
            )
            .unwrap();
        let request = asked["request"]["id"].as_str().unwrap().to_string();
        let row = rig.consent.get_row(&request).unwrap();
        let op = row.change["apply"]["op"].as_str().unwrap().to_string();
        let mut args = row.change["apply"]["args"].clone();
        args["consentId"] = json!(request);
        let run = |raw: &Value| providers.call(&op, raw);
        match rig.asks.apply(&app_caller(), &op, &args, &run) {
            Ok(reply) => break (request, reply),
            Err(e) => {
                assert!(e.message.contains("another account change"), "{e:?}");
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    };
    assert_eq!(reply["state"], "waiting");
    assert!(reply["attempt"].is_u64(), "{reply}");
    assert_eq!(
        rig.consent.get_row(&request).unwrap().state.as_str(),
        "applying"
    );
    request
}

fn ended(rig: &Rig, request: &str) -> Value {
    until(|| rig.consent.get_row(request).unwrap().state.as_str() != "applying");
    let row = rig.consent.get_row(request).unwrap();
    assert_eq!(row.state.as_str(), "applied");
    row.result.unwrap()["account"].clone()
}

fn asked_add_ends_on(rig: &Rig, tool: &Signing, id: &'static str) -> Value {
    let request = asked_add(rig);
    *tool.ends_on.lock().unwrap() = Some(id);
    ended(rig, &request)
}

#[cfg(unix)]
#[test]
fn an_asked_provider_add_places_only_the_account_its_own_attempt_stored() {
    let made: Arc<Mutex<Option<Arc<Signing>>>> = Arc::new(Mutex::new(None));
    let keep = made.clone();
    let rig = live("live-add", move |data| {
        let tool = Arc::new(Signing {
            data: data.to_path_buf(),
            accounts: Mutex::new(Vec::new()),
            ends_on: Mutex::new(None),
        });
        *keep.lock().unwrap() = Some(tool.clone());
        let mut registry = ProviderRegistry::new();
        registry.register(tool);
        registry
    });
    let tool = made.lock().unwrap().clone().unwrap();
    let org_of = |id: &str| {
        let listing = rig.orgs.listing(Kind::Codex).unwrap();
        listing.into_iter().find(|(key, _)| key == id).unwrap().1
    };

    let providers = rig.book.providers.clone();
    let by_hand = providers
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "codex"}))
        .unwrap();
    let attempt = by_hand["attempt"].as_u64().unwrap();
    *tool.ends_on.lock().unwrap() = Some("c-hand");
    until(|| providers.added(attempt).is_some());
    assert_eq!(providers.added(attempt), Some(vec!["c-hand".to_string()]));

    let new = asked_add_ends_on(&rig, &tool, "c-new");
    assert_eq!(new, json!(rig.book.id(ProviderId::CODEX, "c-new")));
    assert_eq!(org_of("c-new"), Some(rig.acme.clone()));
    assert_eq!(org_of("c-hand"), None, "the user's own add stays Personal");

    assert_eq!(asked_add_ends_on(&rig, &tool, "c-hand"), Value::Null);
    assert_eq!(org_of("c-hand"), None);
    assert_eq!(
        asked_add_ends_on(&rig, &tool, "c-new"),
        Value::Null,
        "an add that stored nothing new names no account, the caller's own included"
    );
    assert_eq!(org_of("c-new"), Some(rig.acme.clone()));
}

fn codex_live(tag: &str) -> (Rig, Arc<FakeProcs>, PathBuf) {
    let procs = Arc::new(FakeProcs::new().binary("codex", Path::new("/usr/local/bin/codex")));
    let spawner = procs.clone();
    let rig = live(tag, move |data| {
        let root = data.parent().unwrap();
        std::fs::create_dir_all(root.join("codex")).unwrap();
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(CodexStore::new(
            CodexDeps {
                keychain: Arc::new(FakeKeychain::new()),
                http: Arc::new(FakeHttp::new()),
                procs: spawner,
                apps: Arc::new(FakeApps::new()),
                clock: Arc::new(|| 1_000_000.0),
            },
            CodexPaths {
                home: root.join("codex"),
                state_dir: root.join("codex-state"),
                data_dir: data.to_path_buf(),
            },
        )));
        registry
    });
    let auth = rig.w.d.0.join("codex/auth.json");
    (rig, procs, auth)
}

fn asked_codex_add_ends_on(
    rig: &Rig,
    procs: &FakeProcs,
    auth: &Path,
    terminal: &[u8],
    own: &[u8],
) -> Value {
    procs.login_state().lock().unwrap().exit = None;
    let request = asked_add(rig);
    let spawned = procs.calls().into_iter().rev().find_map(|c| match c {
        ProcCall::SpawnLogin(s) => Some(PathBuf::from(&s.env[0].1)),
        _ => None,
    });
    std::fs::write(auth, terminal).unwrap();
    std::fs::write(spawned.unwrap().join("auth.json"), own).unwrap();
    procs.login_state().lock().unwrap().exit = Some(0);
    ended(rig, &request)
}

#[cfg(unix)]
#[test]
fn a_terminal_codex_login_during_an_asked_add_is_never_placed_or_named() {
    let (rig, procs, auth) = codex_live("live-codex-term");
    let org_of = |id: &str| {
        let listing = rig.orgs.listing(Kind::Codex).unwrap();
        listing.into_iter().find(|(key, _)| key == id).unwrap().1
    };
    let known = codex_login("k@example.com", "u1", "w1");
    std::fs::write(&auth, &known).unwrap();
    let providers = rig.book.providers.clone();
    providers.call(ops::PROVIDERS_LIST, &json!({})).unwrap();
    assert_eq!(org_of("u1:w1"), None);

    let terminal = codex_login("t@example.com", "u8", "w8");
    assert_eq!(
        asked_codex_add_ends_on(&rig, &procs, &auth, &terminal, &known),
        Value::Null,
        "it ended on an account known before"
    );
    assert_eq!(org_of("u8:w8"), None, "the terminal login stays Personal");
    assert_eq!(org_of("u1:w1"), None);

    let later = codex_login("v@example.com", "u9", "w9");
    let new = codex_login("n@example.com", "u3", "w3");
    assert_eq!(
        asked_codex_add_ends_on(&rig, &procs, &auth, &later, &new),
        json!(rig.book.id(ProviderId::CODEX, "u3:w3")),
        "the account its own sign-in ended on"
    );
    assert_eq!(org_of("u3:w3"), Some(rig.acme.clone()));
    assert_eq!(org_of("u9:w9"), None, "the terminal login stays Personal");
    assert_eq!(org_of("u8:w8"), None);
}
