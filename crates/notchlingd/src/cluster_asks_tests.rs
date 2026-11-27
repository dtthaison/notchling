use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::consent_testkit::{agent_caller, app_caller, shell, verified};
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::limits_ops_tests::TempDir;
use notchling_engine::consent::ConsentStore;
use notchling_events::{MemorySink, Salt};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

const HOME: &str = "00000000000000aa";
const BOX: &str = "0123456789abcdef";
const FAR: &str = "fedcba9876543210";
const NEW: &str = "1111111111111111";
const SPARE: &str = "2222222222222222";
const WORKBOX: &str = "3333333333333333";
const TOKEN: &str = "nlj1.SECRET-TOKEN-VALUE";

struct Gate {
    entered: Mutex<Sender<()>>,
    release: Mutex<Receiver<()>>,
}

struct FakeAdmin {
    status: Mutex<Value>,
    orgs: Vec<(&'static str, &'static str)>,
    secrets: Vec<(&'static str, &'static str)>,
    nodes: Vec<(&'static str, &'static str)>,
    host: Mutex<Option<String>>,
    probes: AtomicUsize,
    probe_down: AtomicBool,
    gate: Mutex<Option<Arc<Gate>>>,
    on_host: Mutex<Option<Box<dyn Fn() + Send>>>,
    moved: Mutex<Vec<(String, String)>>,
    forgot: Mutex<Vec<String>>,
    twice: Mutex<Vec<String>>,
    closed: AtomicBool,
}

impl Admin for FakeAdmin {
    fn status(&self) -> Value {
        self.status.lock().unwrap().clone()
    }
    fn own_organization(&self) -> Option<String> {
        (!self.closed.load(Ordering::SeqCst)).then(|| PERSONAL_ID.to_string())
    }
    fn organization_of(&self, machine: &str) -> Option<String> {
        if let Some((_, org)) = self
            .moved
            .lock()
            .unwrap()
            .iter()
            .find(|(n, _)| n == machine)
        {
            return Some(org.clone());
        }
        self.orgs
            .iter()
            .find(|(n, _)| *n == machine)
            .map(|(_, o)| o.to_string())
    }
    fn host_of(&self, machine: &str) -> Option<String> {
        if let Some(hook) = self.on_host.lock().unwrap().as_ref() {
            hook();
        }
        if let Some(host) = self.host.lock().unwrap().clone() {
            return Some(host);
        }
        Some(format!("{machine}.tail.ts.net"))
    }
    fn matches(&self, machine: &str) -> usize {
        if self.forgot.lock().unwrap().iter().any(|n| n == machine) {
            0
        } else if self.twice.lock().unwrap().iter().any(|n| n == machine) {
            2
        } else {
            usize::from(self.nodes.iter().any(|(n, _)| *n == machine))
        }
    }
    fn node_of(&self, machine: &str) -> Option<String> {
        self.probes.fetch_add(1, Ordering::SeqCst);
        let gate = self.gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            let _ = gate.entered.lock().unwrap().send(());
            let _ = gate
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(20));
        }
        if self.probe_down.load(Ordering::SeqCst) {
            return None;
        }
        self.nodes
            .iter()
            .find(|(n, _)| *n == machine)
            .map(|(_, id)| id.to_string())
    }
    fn secret_organization(&self, name: &str) -> Option<String> {
        self.secrets
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, o)| o.to_string())
    }
}

fn status() -> Value {
    json!({
        "cluster": {"id": "c1", "name": "Home", "home": HOME},
        "joining": null,
        "me": HOME,
        "members": [
            {"id": HOME, "name": "mac", "role": "home", "canBeMaster": true, "state": "active",
             "acceptsDelegation": false, "secrets": {"names": [], "any": false}, "addrs": ["100.64.0.1"], "port": 4001},
            {"id": BOX, "name": "build", "role": "worker", "canBeMaster": false, "state": "active",
             "acceptsDelegation": false, "secrets": {"names": ["old-token"], "any": false}, "addrs": ["100.64.0.2"], "port": 4001},
            {"id": FAR, "name": "abroad", "role": "worker", "canBeMaster": true, "state": "active",
             "acceptsDelegation": true, "secrets": {"names": [], "any": false}, "addrs": ["100.64.0.3"], "port": 4001},
            {"id": SPARE, "name": "spare", "role": "standby", "canBeMaster": true, "state": "active",
             "acceptsDelegation": false, "secrets": {"names": [], "any": false}, "addrs": ["100.64.0.4"], "port": 4001},
        ],
        "main": ["sess-1"],
        "failover": {"value": "off", "armed": false, "target": null},
    })
}

struct Rig {
    _tmp: TempDir,
    sink: Arc<MemorySink>,
    inbox: Arc<InboxOps>,
    consent: Arc<ConsentOps>,
    asks: Arc<ClusterAsks>,
    admin: Arc<FakeAdmin>,
    on: Arc<AtomicBool>,
    who: Arc<Mutex<AgentIdentity>>,
    ran: Arc<Mutex<Vec<(String, Value)>>>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.consent.close();
    }
}

fn rig(tag: &str) -> Rig {
    let mut me = verified("parent");
    me.organization = Some(PERSONAL_ID.to_string());
    rig_as(tag, me)
}

fn rig_as(tag: &str, me: AgentIdentity) -> Rig {
    let tmp = TempDir::new(&format!("cluster-asks-{tag}"));
    let room = Arc::new(FakeRoom::default());
    let inbox = InboxOps::new(tmp.path("inbox"), room.clone());
    let sink = Arc::new(MemorySink::new());
    let consent = ConsentOps::with_clock(
        Arc::new(ConsentStore::in_memory_for_tests()),
        room,
        Some(inbox.clone()),
        Some(Events::fixed(sink.clone(), Salt([3; 32]))),
        Arc::new(now_ms),
    );
    let who = Arc::new(Mutex::new(me));
    let seen = who.clone();
    consent.bind_identity(Arc::new(move |_| seen.lock().unwrap().clone()));
    consent.start();
    let admin = Arc::new(FakeAdmin {
        status: Mutex::new(status()),
        orgs: vec![
            ("mac", PERSONAL_ID),
            ("build", PERSONAL_ID),
            ("abroad", "Work"),
            ("spare", PERSONAL_ID),
            ("newbox", PERSONAL_ID),
            ("workbox", "Work"),
        ],
        secrets: vec![("deploy-token", PERSONAL_ID), ("work-token", "Work")],
        nodes: vec![
            ("mac", HOME),
            ("build", BOX),
            ("abroad", FAR),
            ("spare", SPARE),
            ("newbox", NEW),
            ("workbox", WORKBOX),
        ],
        host: Mutex::new(None),
        probes: AtomicUsize::new(0),
        probe_down: AtomicBool::new(false),
        gate: Mutex::new(None),
        on_host: Mutex::new(None),
        moved: Mutex::new(Vec::new()),
        forgot: Mutex::new(Vec::new()),
        twice: Mutex::new(Vec::new()),
        closed: AtomicBool::new(false),
    });
    let on = Arc::new(AtomicBool::new(true));
    let flag = on.clone();
    let asks = ClusterAsks::new(
        consent.clone(),
        admin.clone(),
        Arc::new(move || flag.load(Ordering::SeqCst)),
    );
    Rig {
        _tmp: tmp,
        sink,
        inbox,
        consent,
        asks,
        admin,
        on,
        who,
        ran: Arc::new(Mutex::new(Vec::new())),
    }
}

fn reason(e: &ApiError) -> String {
    e.data
        .as_ref()
        .and_then(|d| d["reason"].as_str())
        .unwrap_or_default()
        .to_string()
}

impl Rig {
    fn ask(&self, action: &str, args: Value) -> Result<Value, ApiError> {
        self.asks.call(
            &agent_caller(),
            ops::CLUSTER_ADMIN_REQUEST,
            &json!({"action": action, "args": args}),
        )
    }

    fn card(&self, action: &str, args: Value) -> String {
        let reply = self.ask(action, args).unwrap();
        assert_eq!(reply["state"], "waiting", "{reply}");
        reply["request"]["id"].as_str().unwrap().to_string()
    }

    fn row(&self, id: &str) -> Consent {
        self.consent.get_row(id).unwrap()
    }

    fn apply(&self, id: &str, reply: Value) -> Result<Value, ApiError> {
        let row = self.row(id);
        let op = row.change["apply"]["op"].as_str().unwrap().to_string();
        let mut args = row.change["apply"]["args"].clone();
        args["consentId"] = json!(id);
        let ran = self.ran.clone();
        self.asks
            .apply(&app_caller(), &op.clone(), &args, &move |a| {
                ran.lock().unwrap().push((op.clone(), a.clone()));
                Ok(reply.clone())
            })
    }

    fn as_session(&self, session: &str, organization: Option<&str>) {
        let mut me = verified(session);
        me.organization = organization.map(str::to_string);
        *self.who.lock().unwrap() = me;
    }

    fn open_count(&self) -> usize {
        self.consent.store().open_rows().unwrap().len()
    }

    fn everywhere(&self) -> String {
        self.inbox.flush();
        let rows = self.consent.store().open_rows().unwrap();
        let mine: Vec<Value> = (0..20)
            .filter_map(|_| None::<Value>)
            .chain(rows.iter().map(|r| json!(format!("{r:?}"))))
            .collect();
        format!(
            "{:?}|{:?}|{:?}",
            self.sink.events(),
            self.inbox.items(),
            mine
        )
    }
}

fn table() -> Vec<(&'static str, Value, &'static str, Value)> {
    vec![
        (
            "create",
            json!({"name": "Fleet"}),
            ops::CLUSTER_CREATE,
            json!({"name": "Fleet"}),
        ),
        ("token", json!({}), ops::CLUSTER_JOIN_TOKEN, json!({})),
        (
            "invite",
            json!({"node": NEW, "name": "newbox", "canBeMaster": true}),
            ops::CLUSTER_INVITE,
            json!({"node": NEW, "name": "newbox", "canBeMaster": true, "acting": false}),
        ),
        (
            "member",
            json!({"node": "build", "role": "master"}),
            ops::CLUSTER_MEMBER_SET,
            json!({"node": BOX, "canBeMaster": true}),
        ),
        (
            "failover",
            json!({"value": "on"}),
            ops::CLUSTER_FAILOVER_SET,
            json!({"value": "30m"}),
        ),
        (
            "main",
            json!({"session": "sess-2"}),
            ops::CLUSTER_MAIN_SET,
            json!({"id": "sess-2", "main": true}),
        ),
        (
            "allow",
            json!({"node": "build", "names": ["deploy-token"]}),
            ops::CLUSTER_ALLOW_SET,
            json!({"node": BOX, "names": ["deploy-token"], "any": false}),
        ),
        (
            "remove",
            json!({"node": "build"}),
            ops::CLUSTER_REMOVE,
            json!({"node": BOX}),
        ),
        (
            "move_home",
            json!({"node": "spare"}),
            ops::CLUSTER_MOVE_HOME,
            json!({"to": SPARE}),
        ),
        ("move_now", json!({}), ops::CLUSTER_MOVE_NOW, json!({})),
    ]
}

fn fresh_cluster_less(r: &Rig) {
    *r.admin.status.lock().unwrap() = json!({"cluster": null, "joining": null, "members": []});
}

#[test]
fn every_action_asks_first_and_runs_only_when_the_app_allows() {
    for (action, args, op, applied) in table() {
        let r = rig(&format!("each-{action}"));
        if action == "create" {
            fresh_cluster_less(&r);
        }
        let id = r.card(action, args);
        let row = r.row(&id);
        assert_eq!(row.kind, "cluster", "{action}");
        assert_eq!(row.change["action"], action);
        assert!(r.ran.lock().unwrap().is_empty(), "asking runs nothing");
        let result = json!({"done": true});
        r.apply(&id, result).unwrap();
        let ran = r.ran.lock().unwrap().clone();
        assert_eq!(ran, vec![(op.to_string(), applied.clone())], "{action}");
        assert_eq!(r.row(&id).state.as_str(), "applied", "{action}");
    }
}

#[test]
fn the_card_names_the_cluster_the_machine_and_the_change() {
    let r = rig("facts");
    let id = r.card(
        "member",
        json!({"node": "build", "role": "master", "delegation": true}),
    );
    let facts = r.row(&id).facts;
    let text = facts.to_string();
    assert!(text.contains("Home"), "{text}");
    assert!(text.contains("build"));
    assert!(text.contains("build.tail.ts.net"));
    assert!(text.contains(PERSONAL_ID));
    assert!(text.contains("worker") && text.contains("master"));
    assert!(text.contains("delegation"));
    let allow = r.card("allow", json!({"node": "build", "names": ["deploy-token"]}));
    let text = r.row(&allow).facts.to_string();
    assert!(text.contains("consent-cluster-allow-secret") && text.contains("deploy-token"));
}

#[test]
fn the_join_token_goes_once_to_the_asking_session_and_is_kept_out_of_every_record() {
    let r = rig("token");
    let id = r.card("invite", json!({"node": NEW, "name": "newbox"}));
    let shown = r
        .apply(&id, json!({"token": TOKEN, "expiresMs": 99, "node": NEW}))
        .unwrap();
    assert!(!shown.to_string().contains(TOKEN), "the app never sees it");
    assert_eq!(r.row(&id).state.as_str(), "applied");

    r.as_session("intruder", Some(PERSONAL_ID));
    let other = r
        .consent
        .call(&agent_caller(), ops::CONSENT_GET, &json!({"id": id}));
    let leaked = other.map(|v| v.to_string()).unwrap_or_default();
    assert!(!leaked.contains(TOKEN), "{leaked}");

    r.as_session("parent", Some(PERSONAL_ID));
    let got = r
        .consent
        .call(&agent_caller(), ops::CONSENT_GET, &json!({"id": id}))
        .unwrap();
    assert_eq!(got["handover"]["token"], TOKEN);
    let again = r
        .consent
        .call(&agent_caller(), ops::CONSENT_GET, &json!({"id": id}))
        .unwrap();
    assert!(again.get("handover").is_none(), "only once");

    let everything = format!("{:?}|{}", r.row(&id), r.everywhere());
    assert!(!everything.contains(TOKEN), "token in a record");
}

#[test]
fn the_join_token_of_the_token_action_is_handed_over_the_same_way() {
    let r = rig("join-token");
    let id = r.card("token", json!({}));
    r.apply(&id, json!({"token": TOKEN})).unwrap();
    let got = r
        .consent
        .call(&agent_caller(), ops::CONSENT_GET, &json!({"id": id}))
        .unwrap();
    assert_eq!(got["handover"]["token"], TOKEN);
}

#[test]
fn a_token_is_lost_after_its_time_and_never_reaches_an_unverified_asker() {
    let r = rig("lost");
    let id = r.card("token", json!({}));
    r.apply(&id, json!({"token": TOKEN})).unwrap();
    let row = r.row(&id);
    {
        let mut held = lock(&r.asks.held);
        for h in held.values_mut() {
            h.at_ms -= HANDOVER_MS + 1;
        }
    }
    let owner = r.who.lock().unwrap().clone();
    assert!(r.asks.take_held(&row, &owner).is_none());
}

#[test]
fn an_unverified_shell_is_refused_for_every_action() {
    let r = rig("shell");
    *r.who.lock().unwrap() = shell("Terminal");
    for (action, args, _, _) in table() {
        let refused = r.ask(action, args).unwrap_err();
        assert_eq!(refused.code, ErrorCode::Forbidden, "{action}");
        assert_eq!(reason(&refused), "no_session", "{action}");
    }
    assert_eq!(r.open_count(), 0);
}

#[test]
fn promote_take_back_and_continue_here_are_refused_with_never() {
    let r = rig("never");
    for action in USER_ONLY {
        let refused = r.ask(action, json!({"node": "build"})).unwrap_err();
        assert_eq!(refused.code, ErrorCode::Forbidden, "{action}");
        assert_eq!(reason(&refused), "never", "{action}");
    }
    assert_eq!(r.open_count(), 0);
}

#[test]
fn a_phone_allow_is_not_taken_and_a_phone_may_deny() {
    let r = rig("phone");
    let id = r.card("remove", json!({"node": "build"}));
    let _ = r.consent.decide(&id, true, None, "phone", None);
    assert_eq!(r.row(&id).state.as_str(), "waiting");
    assert!(r.ran.lock().unwrap().is_empty());
    let denied = r.consent.decide(&id, false, None, "phone", None).unwrap();
    assert_eq!(denied["answered"], json!(true));
    assert_eq!(r.row(&id).state.as_str(), "denied");
}

#[test]
fn dry_run_opens_nothing_and_the_key_gives_one_card_for_the_asking_session() {
    let r = rig("dry-key");
    let dry = r
        .asks
        .call(
            &agent_caller(),
            ops::CLUSTER_ADMIN_REQUEST,
            &json!({"action": "remove", "args": {"node": "build"}, "dryRun": true}),
        )
        .unwrap();
    assert_eq!(dry["dryRun"], true);
    assert_eq!(dry["would"], "ask");
    assert_eq!(r.open_count(), 0);
    assert!(r.ran.lock().unwrap().is_empty());

    let ask = |key: &str| {
        r.asks
            .call(
                &agent_caller(),
                ops::CLUSTER_ADMIN_REQUEST,
                &json!({"action": "remove", "args": {"node": "build"}, "consent": {"key": key}}),
            )
            .unwrap()
    };
    let first = ask("k1");
    let again = ask("k1");
    assert_eq!(first["request"]["id"], again["request"]["id"]);
    assert_eq!(r.open_count(), 1);
    r.as_session("other", Some(PERSONAL_ID));
    let theirs = ask("k1");
    assert_ne!(first["request"]["id"], theirs["request"]["id"]);
    assert_eq!(r.open_count(), 2);
}

#[test]
fn the_cluster_switch_must_be_on() {
    let r = rig("off");
    r.on.store(false, Ordering::SeqCst);
    let refused = r.ask("remove", json!({"node": "build"})).unwrap_err();
    assert_eq!(reason(&refused), "switch_off");
    assert_eq!(r.open_count(), 0);
}

#[test]
fn a_machine_or_secret_of_another_organization_is_refused() {
    let r = rig("orgs");
    let far = r.ask("remove", json!({"node": "abroad"})).unwrap_err();
    assert_eq!(reason(&far), "other_organization");
    let secret = r
        .ask("allow", json!({"node": "build", "names": ["work-token"]}))
        .unwrap_err();
    assert_eq!(reason(&secret), "other_organization");
    let invite = r
        .ask("invite", json!({"node": WORKBOX, "name": "workbox"}))
        .unwrap_err();
    assert_eq!(reason(&invite), "other_organization");
    r.as_session("parent", None);
    assert!(
        r.ask("remove", json!({"node": "build"})).is_ok(),
        "no organization is Personal"
    );
    assert_eq!(
        reason(&r.ask("remove", json!({"node": "abroad"})).unwrap_err()),
        "other_organization",
        "never a wildcard"
    );
    r.as_session("worker", Some("Work"));
    assert_eq!(
        reason(&r.ask("remove", json!({"node": "build"})).unwrap_err()),
        "other_organization"
    );
}

#[test]
fn a_change_that_does_nothing_or_names_nothing_is_refused() {
    let r = rig("noop");
    assert_eq!(
        reason(
            &r.ask("member", json!({"node": "build", "role": "worker"}))
                .unwrap_err()
        ),
        "already_set"
    );
    assert_eq!(
        reason(&r.ask("failover", json!({"value": "off"})).unwrap_err()),
        "already_set"
    );
    assert_eq!(
        reason(&r.ask("remove", json!({"node": "ghost"})).unwrap_err()),
        "no_machine"
    );
    assert_eq!(
        reason(&r.ask("create", json!({"name": "Again"})).unwrap_err()),
        "already_in"
    );
    assert_eq!(
        reason(&r.ask("move_home", json!({"node": "build"})).unwrap_err()),
        "bad_value",
        "build can't be the primary"
    );
    assert!(r.ask("nothing", json!({})).is_err());
    assert!(r
        .ask("remove", json!({"node": "build", "extra": 1}))
        .is_err());
    fresh_cluster_less(&r);
    assert_eq!(
        reason(&r.ask("remove", json!({"node": "build"})).unwrap_err()),
        "no_cluster"
    );
}

#[test]
fn the_app_re_checks_the_cluster_when_the_user_allows() {
    let r = rig("stale");
    let id = r.card("member", json!({"node": "build", "role": "master"}));
    r.admin.status.lock().unwrap()["members"][1]["canBeMaster"] = json!(true);
    let refused = r.apply(&id, json!({})).unwrap_err();
    assert_eq!(reason(&refused), "stale");
    assert!(r.ran.lock().unwrap().is_empty());
    assert_ne!(r.row(&id).state.as_str(), "applied");

    let gone = r.card("remove", json!({"node": "build"}));
    r.admin.status.lock().unwrap()["members"][1]["state"] = json!("removed");
    assert_eq!(reason(&r.apply(&gone, json!({})).unwrap_err()), "stale");
}

#[test]
fn switching_the_cluster_off_after_asking_stops_the_apply() {
    let r = rig("off-later");
    let id = r.card("remove", json!({"node": "build"}));
    r.on.store(false, Ordering::SeqCst);
    assert_eq!(reason(&r.apply(&id, json!({})).unwrap_err()), "stale");
    assert!(r.ran.lock().unwrap().is_empty());
}

#[test]
fn an_id_works_once_only_for_its_own_op_and_only_for_the_app() {
    let r = rig("once");
    let id = r.card("remove", json!({"node": "build"}));
    let other = r
        .asks
        .apply(
            &app_caller(),
            ops::CLUSTER_ALLOW_SET,
            &json!({"node": BOX, "names": [], "any": false, "consentId": id}),
            &|_| panic!("a different change never runs"),
        )
        .unwrap_err();
    assert_eq!(reason(&other), "needs_consent");
    let by_agent = r
        .asks
        .apply(
            &agent_caller(),
            ops::CLUSTER_REMOVE,
            &json!({"node": BOX, "consentId": id}),
            &|_| panic!("an agent never reaches the op"),
        )
        .unwrap_err();
    assert_eq!(reason(&by_agent), "app_only");
    assert_eq!(r.row(&id).state.as_str(), "waiting");
    r.apply(&id, json!({})).unwrap();
    assert!(r.apply(&id, json!({})).is_err());
    assert_eq!(r.ran.lock().unwrap().len(), 1);
}

#[test]
fn only_a_local_cluster_row_is_cited() {
    let r = rig("cites");
    let id = r.card("remove", json!({"node": "build"}));
    assert!(r
        .asks
        .cites(ops::CLUSTER_REMOVE, &json!({"node": BOX, "consentId": id})));
    assert!(!r.asks.cites(ops::CLUSTER_REMOVE, &json!({"node": BOX})));
    assert!(!r.asks.cites(
        ops::CLUSTER_REMOVE,
        &json!({"node": BOX, "consentId": "ghost"})
    ));
    assert!(!r.asks.cites(ops::CLUSTER_STATUS, &json!({"consentId": id})));
}

#[test]
fn a_failed_op_marks_the_request_failed() {
    let r = rig("fails");
    let id = r.card("remove", json!({"node": "build"}));
    let row = r.row(&id);
    let mut args = row.change["apply"]["args"].clone();
    args["consentId"] = json!(id);
    let failed = r
        .asks
        .apply(&app_caller(), ops::CLUSTER_REMOVE, &args, &|_| {
            Err(ApiError::new(ErrorCode::Failed, "primary is away"))
        })
        .unwrap_err();
    assert_eq!(failed.message, "primary is away");
    assert_eq!(r.row(&id).state.as_str(), "failed");
}

#[test]
fn the_card_waits_for_the_user_and_the_asker_can_wait_for_it() {
    let r = rig("wait");
    let id = r.card("failover", json!({"value": "1h"}));
    assert!(r
        .consent
        .wait_for(&id, Duration::from_millis(50))
        .is_some_and(|row| !row.state.closed()));
}

fn set_status(r: &Rig, change: impl FnOnce(&mut Value)) {
    change(&mut r.admin.status.lock().unwrap());
}

#[test]
fn an_invite_takes_its_organization_and_address_from_the_node_it_names() {
    let r = rig("invite-node");
    let spoof = r
        .ask("invite", json!({"node": WORKBOX, "name": "newbox"}))
        .unwrap_err();
    assert_eq!(
        reason(&spoof),
        "wrong_node",
        "a Personal name on a Work node"
    );
    let ghost = r
        .ask("invite", json!({"node": NEW, "name": "ghost"}))
        .unwrap_err();
    assert_eq!(reason(&ghost), "no_machine");
    let other = r
        .ask("invite", json!({"node": WORKBOX, "name": "workbox"}))
        .unwrap_err();
    assert_eq!(reason(&other), "other_organization");
    assert_eq!(r.open_count(), 0);

    let id = r.card("invite", json!({"node": NEW, "name": "newbox"}));
    let text = r.row(&id).facts.to_string();
    assert!(text.contains("newbox.tail.ts.net"), "{text}");
    assert_eq!(r.row(&id).change["organization"], PERSONAL_ID);

    r.as_session("worker", Some("Work"));
    let work = r.card("invite", json!({"node": WORKBOX, "name": "workbox"}));
    assert_eq!(r.row(&work).change["organization"], "Work");
}

#[test]
fn an_invite_never_reuses_a_members_name() {
    let r = rig("invite-dup");
    for (node, name) in [(NEW, "build"), (NEW, BOX)] {
        let refused = r
            .ask("invite", json!({"node": node, "name": name}))
            .unwrap_err();
        assert_eq!(reason(&refused), "already_in", "{name}");
    }
    assert_eq!(r.open_count(), 0);
}

#[test]
fn an_id_beats_a_name_that_looks_like_it() {
    let r = rig("exact-id");
    set_status(&r, |s| {
        s["members"].as_array_mut().unwrap().insert(
            0,
            json!({"id": FAR, "name": BOX, "role": "worker", "canBeMaster": false,
                "state": "active", "acceptsDelegation": false,
                "secrets": {"names": [], "any": false}, "addrs": [], "port": 4001}),
        );
    });
    let found = r.asks.member(&r.admin.status(), BOX).unwrap();
    assert_eq!(found["id"], BOX);
}

#[test]
fn a_card_never_applies_to_another_cluster_or_after_its_facts_changed() {
    for (action, args, _, _) in table() {
        if action == "create" {
            continue;
        }
        let r = rig(&format!("cluster-{action}"));
        let id = r.card(action, args);
        assert!(r.row(&id).change["clusterId"] == "c1", "{action}");
        set_status(&r, |s| s["cluster"]["id"] = json!("c2"));
        let stale = r.apply(&id, json!({})).unwrap_err();
        assert_eq!(reason(&stale), "stale", "{action}");
        assert!(r.ran.lock().unwrap().is_empty(), "{action}");
    }
    let r = rig("facts-changed");
    let id = r.card("remove", json!({"node": "build"}));
    *r.admin.host.lock().unwrap() = Some("elsewhere.example.com".to_string());
    let stale = r.apply(&id, json!({})).unwrap_err();
    assert_eq!(reason(&stale), "stale");
    assert!(r.ran.lock().unwrap().is_empty());
}

#[test]
fn cluster_wide_changes_belong_to_the_organization_of_the_primary() {
    let r = rig("wide");
    r.as_session("worker", Some("Work"));
    for (action, args) in [
        ("token", json!({})),
        ("failover", json!({"value": "on"})),
        ("main", json!({"session": "sess-2"})),
        ("move_now", json!({})),
        ("move_home", json!({"node": "abroad"})),
    ] {
        let refused = r.ask(action, args).unwrap_err();
        assert_eq!(reason(&refused), "other_organization", "{action}");
    }
    assert_eq!(r.open_count(), 0);
    for (action, args) in [
        ("token", json!({})),
        ("failover", json!({"value": "on"})),
        ("main", json!({"session": "sess-2"})),
        ("move_now", json!({})),
    ] {
        let r = rig(&format!("wide-{action}"));
        let id = r.card(action, args);
        assert_eq!(r.row(&id).change["organization"], PERSONAL_ID, "{action}");
    }
}

#[test]
fn moving_the_primary_needs_both_the_old_and_the_new_machine_in_the_organization() {
    let r = rig("move-both");
    set_status(&r, |s| {
        s["cluster"]["home"] = json!(FAR);
        s["members"][0]["role"] = json!("worker");
        s["members"][2]["role"] = json!("home");
    });
    let refused = r.ask("move_home", json!({"node": "spare"})).unwrap_err();
    assert_eq!(
        reason(&refused),
        "other_organization",
        "the primary is in Work"
    );
}

#[test]
fn the_handover_checks_the_caller_not_the_stored_row() {
    let r = rig("who");
    let id = r.card("token", json!({}));
    r.apply(&id, json!({"token": TOKEN})).unwrap();
    let row = r.row(&id);
    let mut other = verified("intruder");
    other.organization = Some(PERSONAL_ID.to_string());
    assert!(r.asks.take_held(&row, &other).is_none());
    assert!(r.asks.take_held(&row, &AgentIdentity::shell()).is_none());
    let mut unverified = verified("parent");
    unverified.verified = false;
    assert!(r.asks.take_held(&row, &unverified).is_none());
    let owner = r.who.lock().unwrap().clone();
    assert_eq!(
        r.asks.take_held(&row, &owner).unwrap()["token"],
        TOKEN,
        "the owner still gets it after the others failed"
    );
    assert!(r.asks.take_held(&row, &owner).is_none(), "once");
}

#[test]
fn main_is_planned_only_where_the_primary_s_list_is_read() {
    let r = rig("main-client");
    set_status(&r, |s| s["me"] = json!(BOX));
    for on in [true, false] {
        let refused = r
            .ask("main", json!({"session": "sess-1", "on": on}))
            .unwrap_err();
        assert_eq!(reason(&refused), "on_home_only");
    }
    assert_eq!(r.open_count(), 0);
    set_status(&r, |s| s["me"] = json!(HOME));
    let id = r.card("main", json!({"session": "sess-1", "on": false}));
    assert_eq!(r.row(&id).change["before"], "on");
}

#[test]
fn the_same_key_after_success_gives_the_first_request_back() {
    let r = rig("key-after");
    let ask = |action: &str, args: Value, key: &str| {
        r.asks.call(
            &agent_caller(),
            ops::CLUSTER_ADMIN_REQUEST,
            &json!({"action": action, "args": args, "consent": {"key": key}}),
        )
    };
    let first = ask("remove", json!({"node": "build"}), "k1").unwrap();
    let id = first["request"]["id"].as_str().unwrap().to_string();
    r.apply(&id, json!({})).unwrap();
    set_status(&r, |s| s["members"][1]["state"] = json!("removed"));
    let again = ask("remove", json!({"node": "build"}), "k1").unwrap();
    assert_eq!(again["request"]["id"], first["request"]["id"]);
    assert_eq!(again["state"], "applied");
    let reused = ask("remove", json!({"node": "spare"}), "k1").unwrap_err();
    assert_eq!(reason(&reused), "key_reused");
    assert_eq!(r.ran.lock().unwrap().len(), 1);

    let member = ask("member", json!({"node": "spare", "role": "worker"}), "k2").unwrap();
    let id = member["request"]["id"].as_str().unwrap().to_string();
    r.apply(&id, json!({})).unwrap();
    set_status(&r, |s| s["members"][3]["canBeMaster"] = json!(false));
    let same = ask("member", json!({"node": "spare", "role": "worker"}), "k2").unwrap();
    assert_eq!(same["request"]["id"], member["request"]["id"]);

    r.as_session("other", Some(PERSONAL_ID));
    let theirs = ask("member", json!({"node": "spare", "role": "worker"}), "k2");
    assert_eq!(
        reason(&theirs.unwrap_err()),
        "already_set",
        "another session's key is its own"
    );
}

#[test]
fn a_cited_id_that_is_not_a_local_cluster_row_is_claimed_and_refused() {
    let r = rig("claims");
    let ghost = json!({"node": BOX, "consentId": "ghost"});
    assert!(r.asks.claims(ops::CLUSTER_REMOVE, &ghost));
    let refused = r
        .asks
        .apply(&app_caller(), ops::CLUSTER_REMOVE, &ghost, &|_| {
            panic!("an unknown id never runs")
        })
        .unwrap_err();
    assert_eq!(reason(&refused), "needs_consent");
    assert!(!r.asks.claims(ops::CLUSTER_REMOVE, &json!({"node": BOX})));
    assert!(!r
        .asks
        .claims(ops::CLUSTER_STATUS, &json!({"consentId": "x"})));
    let worker_switch = json!({"node": BOX, "desired": {"acting": true}, "consentId": "w1"});
    assert!(
        !r.asks.claims(ops::CLUSTER_MEMBER_SET, &worker_switch),
        "a Worker's switch row is answered through desired"
    );
}

#[path = "cluster_asks_probe_tests.rs"]
mod probes;

#[test]
fn cluster_create_refuses_a_generic_name() {
    let r = rig("create-generic");
    fresh_cluster_less(&r);
    for name in ["cluster", "my-cluster", "Team", "new"] {
        let e = r.ask("create", json!({"name": name})).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{name}");
        assert!(
            e.message.contains("doesn't say what the cluster is for"),
            "{name}"
        );
        assert!(e.message.contains("fleet-workers"), "{}", e.message);
    }
    assert_eq!(r.open_count(), 0);
    let id = r.card("create", json!({"name": "acme-builds"}));
    assert_eq!(r.row(&id).change["name"], "acme-builds");
}

#[test]
fn cluster_invite_refuses_a_generic_name() {
    let r = rig("invite-generic");
    for name in ["server", "host", "new-machine"] {
        let e = r
            .ask("invite", json!({"node": NEW, "name": name}))
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{name}");
        assert!(e.message.contains("doesn't say which machine"), "{name}");
    }
    assert_eq!(r.open_count(), 0);
}

#[test]
fn a_dry_run_skips_the_name_check_for_create_and_invite() {
    let r = rig("naming-dry");
    fresh_cluster_less(&r);
    let dry = r
        .asks
        .call(
            &agent_caller(),
            ops::CLUSTER_ADMIN_REQUEST,
            &json!({"action": "create", "args": {"name": "cluster"}, "dryRun": true}),
        )
        .unwrap();
    assert_eq!(dry["would"], "ask");
    assert_eq!(r.open_count(), 0);
}
