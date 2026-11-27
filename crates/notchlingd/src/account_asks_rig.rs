use super::follow::AccountFollow;
use super::*;
use crate::account_ids::AccountIds;
use crate::acting_ops_tests::FakeRoom;
use crate::consent_testkit::{agent_caller, app_caller, verified};
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::limits_ops_tests::TempDir;
use notchling_engine::accounts::claude::ClaudeAccount;
use notchling_engine::consent::ConsentStore;
use notchling_engine::organizations::{Source, PERSONAL_ID};
use notchling_engine::providers::{ProviderAccount, ProviderId};
use notchling_engine::sessions::{AgentGroup, AgentTarget};
use notchling_events::{MemorySink, Salt};
use notchling_local_api::topics;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Mutex;

pub(super) const ORG: &str = "acme";
pub(super) const OTHER: &str = "globex";
pub(super) const A1: &str = "a1a1:0101";
pub(super) const A2: &str = "a2a2:0202";
pub(super) const A3: &str = "a3a3:0303";
pub(super) const A4: &str = "a4a4:0404";
pub(super) const A5: &str = "a5a5:0505";
pub(super) const A6: &str = "a6a6:0606";
pub(super) const A7: &str = "a7a7:0707";
pub(super) const C1: &str = "c1c1-0001";
pub(super) const C2: &str = "c2c2-0002";
pub(super) const G1: &str = "email:gil@work.com";
pub(super) const MAIN: &str = "/home/u/.claude";
pub(super) const LANE_PATH: &str = "/home/u/.notchling/lanes/cat";
pub(super) const HERE: &str = "/home/u/acme/notchling";
pub(super) const THERE: &str = "/home/u/globex/site";
pub(super) const T0: i64 = 1_900_000_000_000;

pub(super) struct World {
    pub snap: Snapshot,
    pub lanes: Vec<Lane>,
    pub providers: Vec<(String, ProviderState)>,
    pub agents: Vec<Agent>,
    pub listing: Vec<(Kind, Listing)>,
    pub closed: bool,
    pub accounts_on: bool,
    pub acting_on: bool,
    pub can_add: bool,
    pub placed: Vec<(Kind, String, String)>,
    pub added: Vec<(u64, Vec<String>)>,
}

pub(super) type Hook = Box<dyn Fn(&mut World) + Send>;
pub(super) type Listing = Vec<(String, Option<String>)>;

pub(super) struct FakeBook {
    pub world: Mutex<World>,
    pub ids: AccountIds,
    pub calls: AtomicUsize,
    armed: Mutex<Option<(usize, Hook)>>,
}

pub(super) fn claude_account(raw: &str, email: &str) -> ClaudeAccount {
    ClaudeAccount {
        id: raw.to_string(),
        email: email.to_string(),
        plan: "max".to_string(),
        needs_login: false,
        organization: None,
    }
}

pub(super) fn agent(id: &str, job: bool, dir: &str, cwd: &str) -> Agent {
    Agent {
        id: id.to_string(),
        name: "notchling".to_string(),
        project: "notchling".to_string(),
        cwd: cwd.to_string(),
        group: AgentGroup::Working,
        label: "Working".to_string(),
        detail: None,
        since_ms: 0,
        target: if job {
            AgentTarget::Job(format!("job-{id}"))
        } else {
            AgentTarget::Process(55)
        },
        suggested_reply: None,
        transcript_path: Some(format!("{dir}/projects/x/{id}.jsonl")),
        host: "mac".to_string(),
        headless: false,
        config_dir: Some(dir.to_string()),
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
        worktree_path: None,
    }
}

pub(super) fn world() -> World {
    let acme = || Some(ORG.to_string());
    World {
        snap: Snapshot {
            accounts: vec![
                claude_account(A1, "ann@work.com"),
                claude_account(A2, "bob@work.com"),
                claude_account(A3, "cat@work.com"),
                claude_account(A4, "zed@else.com"),
                claude_account(A5, "unlisted@work.com"),
                claude_account(A6, "ghost@work.com"),
                claude_account(A7, "solo@home.com"),
            ],
            active: Some(A1.to_string()),
        },
        lanes: vec![Lane {
            name: "cat".to_string(),
            path: LANE_PATH.to_string(),
            account_id: A3.to_string(),
            email: "cat@work.com".to_string(),
            folder: None,
        }],
        providers: vec![
            (
                ProviderId::CODEX.to_string(),
                ProviderState {
                    accounts: vec![
                        ProviderAccount::new(C1, "dan@work.com", None),
                        ProviderAccount::new(C2, "eve@work.com", None),
                    ],
                    active_id: Some(C1.to_string()),
                    ..ProviderState::default()
                },
            ),
            (
                ProviderId::ANTIGRAVITY.to_string(),
                ProviderState {
                    accounts: vec![ProviderAccount::new(G1, "gil@work.com", None)],
                    active_id: Some(G1.to_string()),
                    ..ProviderState::default()
                },
            ),
        ],
        agents: vec![
            agent("agent-1", true, MAIN, HERE),
            agent("agent-7", true, LANE_PATH, HERE),
        ],
        listing: vec![
            (
                Kind::Claude,
                vec![
                    (A1.to_string(), acme()),
                    (A2.to_string(), acme()),
                    (A3.to_string(), acme()),
                    (A4.to_string(), Some(OTHER.to_string())),
                    (A6.to_string(), Some("ghost".to_string())),
                    (A7.to_string(), None),
                ],
            ),
            (
                Kind::Codex,
                vec![(C1.to_string(), acme()), (C2.to_string(), acme())],
            ),
            (Kind::Antigravity, vec![(G1.to_string(), acme())]),
        ],
        closed: false,
        accounts_on: true,
        acting_on: true,
        can_add: true,
        placed: Vec::new(),
        added: Vec::new(),
    }
}

impl FakeBook {
    fn tick(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }

    pub(super) fn with<T>(&self, f: impl FnOnce(&mut World) -> T) -> T {
        f(&mut self.world.lock().unwrap())
    }

    pub(super) fn arm(&self, reads: usize, hook: impl Fn(&mut World) + Send + 'static) {
        *self.armed.lock().unwrap() = Some((reads, Box::new(hook)));
    }
}

impl Book for FakeBook {
    fn claude(&self) -> Snapshot {
        self.tick();
        let mut armed = self.armed.lock().unwrap();
        if let Some((left, hook)) = armed.as_mut() {
            *left -= 1;
            if *left == 0 {
                hook(&mut self.world.lock().unwrap());
                *armed = None;
            }
        }
        drop(armed);
        self.with(|w| w.snap.clone())
    }
    fn lanes(&self) -> Vec<Lane> {
        self.tick();
        self.with(|w| w.lanes.clone())
    }
    fn providers(&self) -> Vec<(String, ProviderState)> {
        self.tick();
        self.with(|w| w.providers.clone())
    }
    fn agents(&self) -> Vec<Agent> {
        self.tick();
        self.with(|w| w.agents.clone())
    }
    fn closed(&self) -> bool {
        self.tick();
        self.with(|w| w.closed)
    }
    fn listing(&self, kind: Kind) -> Option<Vec<(String, Option<String>)>> {
        self.tick();
        self.with(|w| {
            w.listing
                .iter()
                .find(|(k, _)| *k == kind)
                .map(|(_, l)| l.clone())
        })
    }
    fn resolve(&self, _kind: Kind, _key: &str, field: Option<&str>) -> Resolved {
        self.tick();
        match field {
            Some(org) => Resolved {
                id: org.to_string(),
                source: Source::Field,
                known: [ORG, OTHER, PERSONAL_ID].contains(&org),
            },
            None => Resolved {
                id: PERSONAL_ID.to_string(),
                source: Source::Default,
                known: true,
            },
        }
    }
    fn folder_org(&self, folder: &str) -> String {
        self.tick();
        if folder.starts_with("/home/u/acme") {
            ORG.to_string()
        } else if folder.starts_with("/home/u/globex") {
            OTHER.to_string()
        } else {
            PERSONAL_ID.to_string()
        }
    }
    fn main_dir(&self) -> String {
        self.tick();
        MAIN.to_string()
    }
    fn home(&self) -> String {
        self.tick();
        "/home/u".to_string()
    }
    fn accounts_on(&self) -> bool {
        self.tick();
        self.with(|w| w.accounts_on)
    }
    fn acting_on(&self) -> bool {
        self.tick();
        self.with(|w| w.acting_on)
    }
    fn can_add(&self, _provider: &str) -> bool {
        self.tick();
        self.with(|w| w.can_add)
    }
    fn add_limit_ms(&self) -> i64 {
        self.tick();
        900_000
    }
    fn added(&self, attempt: u64) -> Option<Vec<String>> {
        self.with(|w| {
            w.added
                .iter()
                .find(|(kept, _)| *kept == attempt)
                .map(|(_, ids)| ids.clone())
        })
    }
    fn id(&self, provider: &str, raw: &str) -> Option<String> {
        self.ids.id(provider, raw)
    }
    fn lane_id(&self, name: &str) -> Option<String> {
        self.ids.lane(name)
    }
    fn place(&self, kind: Kind, raw: &str, organization: &str) -> bool {
        self.tick();
        self.with(|w| {
            w.placed
                .push((kind, raw.to_string(), organization.to_string()));
            if let Some((_, list)) = w.listing.iter_mut().find(|(k, _)| *k == kind) {
                list.retain(|(key, _)| key != raw);
                list.push((raw.to_string(), Some(organization.to_string())));
            }
        });
        true
    }
}

pub(super) type Ran = Arc<Mutex<Vec<(String, Value)>>>;

pub(super) struct Rig {
    _tmp: TempDir,
    pub sink: Arc<MemorySink>,
    pub room: Arc<FakeRoom>,
    pub inbox: Arc<InboxOps>,
    pub consent: Arc<ConsentOps>,
    pub asks: Arc<AccountAsks>,
    pub book: Arc<FakeBook>,
    pub who: Arc<Mutex<AgentIdentity>>,
    pub ran: Ran,
    pub clock: Arc<AtomicI64>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.consent.close();
    }
}

pub(super) fn rig(tag: &str) -> Rig {
    let tmp = TempDir::new(&format!("account-asks-{tag}"));
    let room = Arc::new(FakeRoom::default());
    let inbox = InboxOps::new(tmp.path("inbox"), room.clone());
    let sink = Arc::new(MemorySink::new());
    let clock = Arc::new(AtomicI64::new(T0));
    let read = clock.clone();
    let now: crate::consent_ops::Clock = Arc::new(move || read.load(Ordering::SeqCst));
    let consent = ConsentOps::with_clock(
        Arc::new(ConsentStore::in_memory_for_tests()),
        room.clone(),
        Some(inbox.clone()),
        Some(Events::fixed(sink.clone(), Salt([3; 32]))),
        now.clone(),
    );
    let mut me = verified("parent");
    me.organization = Some(ORG.to_string());
    let who = Arc::new(Mutex::new(me));
    let seen = who.clone();
    consent.bind_identity(Arc::new(move |_| seen.lock().unwrap().clone()));
    consent.start();
    let book = Arc::new(FakeBook {
        world: Mutex::new(world()),
        ids: AccountIds::in_dir(&tmp.path("ids")),
        calls: AtomicUsize::new(0),
        armed: Mutex::new(None),
    });
    let asks = AccountAsks::new(consent.clone(), book.clone(), AccountFollow::new(now));
    Rig {
        _tmp: tmp,
        sink,
        room,
        inbox,
        consent,
        asks,
        book,
        who,
        ran: Arc::new(Mutex::new(Vec::new())),
        clock,
    }
}

pub(super) fn reason(e: &ApiError) -> String {
    e.data
        .as_ref()
        .and_then(|d| d["reason"].as_str())
        .unwrap_or_default()
        .to_string()
}

impl Rig {
    pub(super) fn acct(&self, raw: &str) -> String {
        self.book.ids.id(ProviderId::CLAUDE_CODE, raw).unwrap()
    }

    pub(super) fn codex(&self, raw: &str) -> String {
        self.book.ids.id(ProviderId::CODEX, raw).unwrap()
    }

    pub(super) fn lane(&self, name: &str) -> String {
        self.book.ids.lane(name).unwrap()
    }

    pub(super) fn ask(&self, action: &str, args: Value) -> Result<Value, ApiError> {
        self.asks.call(
            &agent_caller(),
            ops::ACCOUNTS_REQUEST,
            &json!({"action": action, "args": args}),
        )
    }

    pub(super) fn card(&self, action: &str, args: Value) -> String {
        let reply = self.ask(action, args).unwrap();
        assert_eq!(reply["state"], "waiting", "{reply}");
        reply["request"]["id"].as_str().unwrap().to_string()
    }

    pub(super) fn row(&self, id: &str) -> Consent {
        self.consent.get_row(id).unwrap()
    }

    pub(super) fn state(&self, id: &str) -> String {
        self.row(id).state.as_str().to_string()
    }

    pub(super) fn apply_with(
        &self,
        id: &str,
        run: impl Fn(&Value) -> Result<Value, ApiError>,
    ) -> Result<Value, ApiError> {
        let row = self.row(id);
        let op = row.change["apply"]["op"].as_str().unwrap().to_string();
        let mut args = row.change["apply"]["args"].clone();
        args["consentId"] = json!(id);
        let ran = self.ran.clone();
        let name = op.clone();
        self.asks.apply(&app_caller(), &op, &args, &move |a| {
            ran.lock().unwrap().push((name.clone(), a.clone()));
            run(a)
        })
    }

    pub(super) fn apply(&self, id: &str, reply: Value) -> Result<Value, ApiError> {
        self.apply_with(id, move |_| Ok(reply.clone()))
    }

    pub(super) fn ran(&self) -> Vec<(String, Value)> {
        self.ran.lock().unwrap().clone()
    }

    pub(super) fn open_count(&self) -> usize {
        self.consent.store().open_rows().unwrap().len()
    }

    pub(super) fn list_new(&self, kind: Kind, raw: &str) {
        self.book.with(|w| {
            if let Some((_, list)) = w.listing.iter_mut().find(|(k, _)| *k == kind) {
                if !list.iter().any(|(key, _)| key == raw) {
                    list.push((raw.to_string(), None));
                }
            }
        });
    }

    pub(super) fn add_ends(&self, attempt: u64, new: &[&str]) {
        self.book.with(|w| {
            let codex = &mut w.providers[0].1;
            for raw in new {
                codex
                    .accounts
                    .push(ProviderAccount::new(raw, "new@work.com", None));
                codex.active_id = Some(raw.to_string());
            }
            w.added
                .push((attempt, new.iter().map(|raw| raw.to_string()).collect()));
        });
        for raw in new {
            self.list_new(Kind::Codex, raw);
        }
    }

    pub(super) fn add_changed(&self, provider: &str, attempt: u64, state: &str) {
        self.asks.follow().saw(
            topics::PROVIDERS,
            topics::PROVIDERS_CHANGED,
            &json!({"provider": provider, "reason": "add", "state": state, "attempt": attempt}),
        );
    }

    pub(super) fn advance(&self, ms: i64) {
        self.clock.fetch_add(ms, Ordering::SeqCst);
    }

    pub(super) fn everywhere(&self, ids: &[String]) -> String {
        self.inbox.flush();
        let rows: Vec<String> = ids
            .iter()
            .filter_map(|id| self.consent.get_row(id).ok())
            .map(|r| format!("{r:?}"))
            .collect();
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}",
            self.sink.events(),
            self.room.events.lock().unwrap(),
            self.room.scoped.lock().unwrap(),
            self.inbox.items(),
            rows
        )
    }
}
