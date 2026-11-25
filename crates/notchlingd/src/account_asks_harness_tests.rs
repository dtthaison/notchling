use super::*;
use crate::account_ids::AccountIds;
use crate::accounts_ops_tests::{profile, slot, world, World, ID1, ID2};
use crate::acting_ops_tests::{app, FakeRoom};
use crate::consent_testkit::{agent_caller, app_caller, verified};
use crate::event_assert::{of, text};
use crate::op_group::OpGroup;
use notchling_engine::accounts::claude::{ClaudeAccounts, LIVE_SERVICE, SAVED_SERVICE};
use notchling_engine::accounts::fake::{MemoryKeychain, ScriptedHttp};
use notchling_engine::accounts::http::{PROFILE_URL, TOKEN_URL};
use notchling_engine::consent::ConsentStore;
use notchling_engine::organizations::{Kind, Organizations};
use notchling_engine::providers::ProviderId;

struct StoreBook {
    store: Arc<ClaudeAccounts>,
    orgs: Organizations,
    ids: AccountIds,
    home: std::path::PathBuf,
}

impl Book for StoreBook {
    fn claude(&self) -> Snapshot {
        self.store.snapshot()
    }
    fn lanes(&self) -> Vec<Lane> {
        self.store.env().lanes.all()
    }
    fn providers(&self) -> Vec<(String, ProviderState)> {
        Vec::new()
    }
    fn agents(&self) -> Vec<Agent> {
        Vec::new()
    }
    fn closed(&self) -> bool {
        self.orgs.view().is_closed()
    }
    fn listing(&self, kind: Kind) -> Option<Vec<(String, Option<String>)>> {
        self.orgs.listing(kind)
    }
    fn resolve(&self, kind: Kind, key: &str, field: Option<&str>) -> Resolved {
        self.orgs.view().resolve(kind, key, field)
    }
    fn folder_org(&self, folder: &str) -> String {
        self.orgs
            .view()
            .folder_here(std::path::Path::new(folder), &self.home)
            .organization
    }
    fn main_dir(&self) -> String {
        self.home.join(".claude").display().to_string()
    }
    fn home(&self) -> String {
        self.home.display().to_string()
    }
    fn accounts_on(&self) -> bool {
        true
    }
    fn acting_on(&self) -> bool {
        true
    }
    fn can_add(&self, _provider: &str) -> bool {
        false
    }
    fn add_limit_ms(&self) -> i64 {
        0
    }
    fn added(&self, _attempt: u64) -> Option<Vec<String>> {
        None
    }
    fn id(&self, provider: &str, raw: &str) -> Option<String> {
        self.ids.id(provider, raw)
    }
    fn lane_id(&self, name: &str) -> Option<String> {
        self.ids.lane(name)
    }
    fn place(&self, kind: Kind, raw: &str, organization: &str) -> bool {
        self.orgs.set_organization(kind, raw, organization).is_ok()
    }
}

fn signed_in_as_one(w: &World, chain: &MemoryKeychain, http: &ScriptedHttp) {
    std::fs::write(
        w.d.0.join("home/.claude.json"),
        "{\"oauthAccount\": {\"accountUuid\": \"a1a1\", \"organizationUuid\": \"0101\", \"emailAddress\": \"one@example.com\"}}",
    )
    .unwrap();
    chain.put(
        LIVE_SERVICE,
        "me",
        r#"{"claudeAiOauth":{"accessToken":"test-access-1"}}"#,
    );
    chain.put(SAVED_SERVICE, ID2, &slot(ID2, "two@example.com", "2"));
    http.answer(PROFILE_URL, 200, &profile(ID1, "one@example.com"));
}

struct Rig {
    w: World,
    http: Arc<ScriptedHttp>,
    consent: Arc<ConsentOps>,
    asks: Arc<AccountAsks>,
    ids: AccountIds,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.consent.close();
    }
}

fn rig(tag: &str) -> Rig {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let w = world(tag, true, chain.clone(), http.clone());
    signed_in_as_one(&w, &chain, &http);
    w.ops
        .call(&app(), ops::ACCOUNTS_LIST, &json!({"sync": true}))
        .unwrap();
    let data = w.d.0.join("data");
    let home = w.d.0.join("home");
    let orgs = Organizations::new(&data).with_home(&home);
    let acme = orgs.create("Acme", None, None).unwrap();
    notchling_engine::store::put_doc(
        &data,
        Kind::Claude.doc(),
        &json!([
            {"id": ID1, "email": "one@example.com", "plan": "Pro", "needsLogin": false},
            {"id": ID2, "email": "two@example.com", "plan": "Pro", "needsLogin": false}
        ])
        .to_string(),
    )
    .unwrap();
    for id in [ID1, ID2] {
        orgs.set_organization(Kind::Claude, id, &acme).unwrap();
    }
    let ids = AccountIds::in_dir(&data.join("daemon"));
    let book = StoreBook {
        store: w.ops.store(),
        orgs,
        ids: AccountIds::in_dir(&data.join("daemon")),
        home,
    };
    let consent = ConsentOps::with_clock(
        Arc::new(ConsentStore::in_memory_for_tests()),
        Arc::new(FakeRoom::default()),
        None,
        None,
        Arc::new(crate::acting_ops::now_ms),
    );
    let mut me = verified("s-harness");
    me.organization = Some(acme);
    consent.bind_identity(Arc::new(move |_| me.clone()));
    consent.start();
    let asks = AccountAsks::new(
        consent.clone(),
        Arc::new(book),
        follow::AccountFollow::new(Arc::new(crate::acting_ops::now_ms)),
    );
    Rig {
        w,
        http,
        consent,
        asks,
        ids,
    }
}

impl Rig {
    fn switch_through_the_harness(&self) -> String {
        let target = self.ids.id(ProviderId::CLAUDE_CODE, ID2).unwrap();
        let reply = self
            .asks
            .call(
                &agent_caller(),
                ops::ACCOUNTS_REQUEST,
                &json!({"action": "switch", "args": {"account": target}}),
            )
            .unwrap();
        assert_eq!(reply["state"], "waiting", "{reply}");
        let id = reply["request"]["id"].as_str().unwrap().to_string();
        let row = self.consent.get_row(&id).unwrap();
        let op = row.change["apply"]["op"].as_str().unwrap().to_string();
        let mut args = row.change["apply"]["args"].clone();
        args["consentId"] = json!(id);
        let w = &self.w;
        self.asks
            .apply(&app_caller(), &op, &args, &|raw| {
                w.ops.call(&app(), &op, raw)
            })
            .unwrap();
        id
    }
}

#[test]
fn a_switch_goes_through_the_harness_only() {
    let rig = rig("asks-harness-switch");
    assert_eq!(
        of(&rig.w.sink, "account.switched")
            .iter()
            .filter_map(|e| text(e, "to"))
            .collect::<Vec<_>>(),
        [ID1]
    );
    let id = rig.switch_through_the_harness();
    let moved: Vec<String> = of(&rig.w.sink, "account.switched")
        .iter()
        .filter_map(|e| text(e, "to"))
        .collect();
    assert_eq!(moved, [ID1, ID2]);
    let last = of(&rig.w.sink, "account.switched").pop().unwrap();
    assert_eq!(text(&last, "to").as_deref(), Some(ID2));
    assert_eq!(text(&last, "cause").as_deref(), Some("user"));
    assert_eq!(rig.consent.get_row(&id).unwrap().state.as_str(), "applied");
    assert_eq!(rig.w.ops.store().snapshot().active.as_deref(), Some(ID2));
}

#[test]
fn a_consented_switch_never_renews_the_login_in_use() {
    let rig = rig("asks-harness-renew");
    let before = rig.http.seen().len();
    rig.switch_through_the_harness();
    let seen = rig.http.seen();
    assert!(
        seen.iter().all(|(url, _)| url != TOKEN_URL),
        "a login was renewed: {seen:?}"
    );
    assert!(seen.len() >= before);
}
