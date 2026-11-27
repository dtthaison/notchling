use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::consent_testkit::{agent_caller, verified};
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::limits_ops_tests::TempDir;
use notchling_engine::consent::ConsentStore;
use notchling_events::{MemorySink, Salt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

pub(crate) struct FakeFleet {
    pub(crate) members: Mutex<Vec<Member>>,
    pub(crate) jobs: Mutex<Vec<JobRow>>,
    secrets: Vec<(&'static str, &'static str)>,
}

impl Fleet for FakeFleet {
    fn member(&self, name_or_id: &str) -> Option<Member> {
        self.members
            .lock()
            .unwrap()
            .iter()
            .find(|m| m.id == name_or_id || m.name == name_or_id)
            .cloned()
    }
    fn jobs(&self) -> Vec<JobRow> {
        self.jobs.lock().unwrap().clone()
    }
    fn secret_organization(&self, name: &str) -> Option<String> {
        self.secrets
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, o)| o.to_string())
    }
}

#[derive(Default)]
pub(crate) struct FakeHands {
    pub(crate) calls: Mutex<Vec<(String, Value)>>,
}

impl Hands for FakeHands {
    fn can_halt(&self, job: &JobRow) -> bool {
        job.tool == "shell"
    }
    fn dispatch(&self, args: &Value) -> Result<Value, ApiError> {
        self.calls
            .lock()
            .unwrap()
            .push(("dispatch".into(), args.clone()));
        Ok(json!({"job": job_id(args["key"].as_str().unwrap_or_default()), "state": "queued"}))
    }
    fn tail(&self, job: &JobRow, from: u64) -> Result<Value, ApiError> {
        self.calls
            .lock()
            .unwrap()
            .push(("tail".into(), json!({"job": job.id, "from": from})));
        Ok(json!({"text": "output", "next": 6}))
    }
    fn give_back(&self, job: &JobRow) -> Result<Value, ApiError> {
        self.calls
            .lock()
            .unwrap()
            .push(("return".into(), json!({"job": job.id})));
        Ok(json!({"returned": true}))
    }
    fn halt(&self, job: &JobRow) -> Result<Value, ApiError> {
        self.calls
            .lock()
            .unwrap()
            .push(("stop".into(), json!({"job": job.id})));
        Ok(json!({"stopped": true}))
    }
}

pub(crate) struct Rig {
    _tmp: TempDir,
    pub(crate) consent: Arc<ConsentOps>,
    pub(crate) asks: Arc<JobAsks>,
    pub(crate) hands: Arc<FakeHands>,
    pub(crate) on: Arc<AtomicBool>,
    pub(crate) fleet: Arc<FakeFleet>,
    pub(crate) who: Arc<Mutex<AgentIdentity>>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.consent.close();
    }
}

pub(crate) fn member(id: &str, name: &str, organization: &str) -> Member {
    Member {
        id: id.to_string(),
        name: name.to_string(),
        organization: organization.to_string(),
        online: true,
        me: false,
    }
}

pub(crate) fn job(id: &str, organization: &str) -> JobRow {
    JobRow {
        id: id.to_string(),
        node: "0123456789abcdef".to_string(),
        machine: "build".to_string(),
        organization: organization.to_string(),
        tool: "shell".to_string(),
        folder: "/work/a".to_string(),
        state: "running".to_string(),
        created_ms: 5,
        session: None,
        detail: Some("secret detail".to_string()),
        secrets: vec!["deploy-token".to_string()],
        local: false,
    }
}

pub(crate) fn rig(tag: &str) -> Rig {
    let mut me = verified("parent");
    me.organization = Some("Personal".to_string());
    let members = vec![
        member("0123456789abcdef", "build", "Personal"),
        member("fedcba9876543210", "abroad", "Work"),
    ];
    rig_as(tag, me, members)
}

pub(crate) fn rig_as(tag: &str, me: AgentIdentity, members: Vec<Member>) -> Rig {
    let tmp = TempDir::new(&format!("job-asks-{tag}"));
    let room = Arc::new(FakeRoom::default());
    let inbox = InboxOps::new(tmp.path("inbox"), room.clone());
    let sink = Arc::new(MemorySink::new());
    let consent = ConsentOps::with_clock(
        Arc::new(ConsentStore::in_memory_for_tests()),
        room,
        Some(inbox),
        Some(Events::fixed(sink, Salt([3; 32]))),
        Arc::new(now_ms),
    );
    let who = Arc::new(Mutex::new(me));
    let seen = who.clone();
    consent.bind_identity(Arc::new(move |_| seen.lock().unwrap().clone()));
    consent.start();
    let fleet = Arc::new(FakeFleet {
        members: Mutex::new(members),
        jobs: Mutex::new(vec![job("j-own", "Personal"), job("j-other", "Personal")]),
        secrets: vec![("deploy-token", "Personal"), ("work-token", "Work")],
    });
    let hands = Arc::new(FakeHands::default());
    let on = Arc::new(AtomicBool::new(true));
    let flag = on.clone();
    let asks = JobAsks::new(
        consent.clone(),
        fleet.clone(),
        hands.clone(),
        Arc::new(JobBook::in_memory()),
        Arc::new(move || flag.load(Ordering::SeqCst)),
    );
    asks.book().record(
        "j-own",
        Record {
            session: "parent".to_string(),
            organization: "Personal".to_string(),
            kind: "dispatch".to_string(),
            tool: "shell".to_string(),
            folder: "/work/a".to_string(),
            started_ms: 5,
            ended_ms: None,
            exit: None,
        },
    );
    Rig {
        _tmp: tmp,
        consent,
        asks,
        hands,
        on,
        fleet,
        who,
    }
}

impl Rig {
    pub(crate) fn as_session(&self, session: &str, organization: Option<&str>) {
        let mut me = verified(session);
        me.organization = organization.map(str::to_string);
        *self.who.lock().unwrap() = me;
    }

    pub(crate) fn call(&self, op: &str, args: Value) -> Result<Value, ApiError> {
        self.asks.call(&agent_caller(), op, &args)
    }

    pub(crate) fn ran(&self) -> Vec<String> {
        self.hands
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|(w, _)| w.clone())
            .collect()
    }

    pub(crate) fn card_of(&self, reply: &Value) -> String {
        assert_eq!(reply["state"], "waiting", "{reply}");
        reply["request"]["id"].as_str().unwrap().to_string()
    }

    pub(crate) fn allow(&self, id: &str, by: &str) -> Consent {
        let said = self.consent.decide(id, true, None, by, None).unwrap();
        assert_eq!(said["answered"], json!(true));
        self.consent.wait_for(id, Duration::from_secs(5)).unwrap()
    }
}

pub(crate) fn reason(e: &ApiError) -> String {
    e.data
        .as_ref()
        .and_then(|d| d["reason"].as_str())
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn args_for(tool: &str, key: &str) -> Value {
    let mut args = json!({"machine": "build", "node": "0123456789abcdef", "key": key, "tool": tool, "cwd": "/work/a"});
    match tool {
        "shell" => args["command"] = json!("make test"),
        "claude" => {}
        _ => args["text"] = json!("run the tests"),
    }
    args
}

#[test]
fn a_dispatch_of_every_tool_asks_first_and_runs_only_after_the_mac_allows() {
    let r = rig("tools");
    for tool in ["claude", "codex", "agy", "shell"] {
        let key = format!("k-{tool}");
        let before = r.ran().len();
        let reply = r.call(ops::CLUSTER_DISPATCH, args_for(tool, &key)).unwrap();
        let id = r.card_of(&reply);
        assert_eq!(reply["request"]["kind"], "dispatch", "{tool}");
        assert_eq!(r.ran().len(), before, "asking runs nothing");
        let done = r.allow(&id, "mac");
        assert_eq!(done.state.as_str(), "applied", "{tool}");
        assert_eq!(r.ran().len(), before + 1, "{tool}");
        let args = r.hands.calls.lock().unwrap().last().unwrap().1.clone();
        assert_eq!(args["tool"], tool);
        assert_eq!(args["node"], "0123456789abcdef");
        let started = job_id(&key);
        let owned = r.asks.book().owned_by("parent", &started);
        assert!(owned.is_some(), "{tool} records the dispatching session");
    }
}

#[test]
fn a_phone_may_not_allow_a_dispatch_for_any_tool() {
    for tool in ["claude", "codex", "agy", "shell"] {
        let r = rig(&format!("phone-{tool}"));
        let reply = r
            .call(ops::CLUSTER_DISPATCH, args_for(tool, &format!("p-{tool}")))
            .unwrap();
        let id = r.card_of(&reply);
        let refused = r
            .consent
            .decide(&id, true, None, "phone", None)
            .unwrap_err();
        assert_eq!(refused.code, ErrorCode::Forbidden, "{tool}");
        assert!(r.ran().is_empty());
    }
}

#[test]
fn the_same_key_sends_one_job_and_one_card() {
    let r = rig("key");
    let first = r
        .call(ops::CLUSTER_DISPATCH, args_for("shell", "same"))
        .unwrap();
    let id = r.card_of(&first);
    let again = r
        .call(ops::CLUSTER_DISPATCH, args_for("shell", "same"))
        .unwrap();
    assert_eq!(r.card_of(&again), id);
    assert_eq!(r.consent.store().open_rows().unwrap().len(), 1);
    r.allow(&id, "mac");
    assert_eq!(r.ran(), ["dispatch"]);
    let after = r
        .call(ops::CLUSTER_DISPATCH, args_for("shell", "same"))
        .unwrap();
    assert_eq!(r.card_of_or_state(&after), "applied");
    assert_eq!(r.ran(), ["dispatch"], "the key never sends twice");
}

impl Rig {
    fn card_of_or_state(&self, reply: &Value) -> String {
        reply["state"].as_str().unwrap_or_default().to_string()
    }
}

#[test]
fn dry_run_on_every_path_changes_nothing() {
    let r = rig("dry");
    let mut args = args_for("shell", "dry");
    args["dryRun"] = json!(true);
    let reply = r.call(ops::CLUSTER_DISPATCH, args).unwrap();
    assert_eq!(reply["dryRun"], true);
    assert_eq!(r.consent.store().open_rows().unwrap().len(), 0);
    for op in [ops::JOBS_TAIL, ops::JOBS_RETURN, ops::JOBS_STOP] {
        let own = r.call(op, json!({"id": "j-own", "dryRun": true})).unwrap();
        assert_eq!(own["dryRun"], true, "{op} own");
        assert_eq!(own["would"], "apply");
        let other = r
            .call(op, json!({"id": "j-other", "dryRun": true}))
            .unwrap();
        assert_eq!(other["dryRun"], true, "{op} other");
        assert_eq!(other["would"], "ask");
    }
    assert!(r.ran().is_empty());
    assert_eq!(r.consent.store().open_rows().unwrap().len(), 0);
}

#[test]
fn own_jobs_tail_return_and_stop_free_and_others_need_a_card() {
    let r = rig("own");
    let tail = r
        .call(ops::JOBS_TAIL, json!({"id": "j-own", "from": 3}))
        .unwrap();
    assert_eq!(tail["free"], true);
    assert_eq!(tail["result"]["text"], "output");
    assert_eq!(
        r.call(ops::JOBS_RETURN, json!({"id": "j-own"})).unwrap()["free"],
        true
    );
    assert_eq!(
        r.call(ops::JOBS_STOP, json!({"id": "j-own"})).unwrap()["free"],
        true
    );
    assert_eq!(r.ran(), ["tail", "return", "stop"]);
    for (op, kind) in [
        (ops::JOBS_TAIL, "job"),
        (ops::JOBS_RETURN, "job"),
        (ops::JOBS_STOP, "job_stop"),
    ] {
        let before = r.ran().len();
        let reply = r.call(op, json!({"id": "j-other"})).unwrap();
        let id = r.card_of(&reply);
        assert_eq!(reply["request"]["kind"], kind, "{op}");
        assert_eq!(r.ran().len(), before, "asking runs nothing");
        r.allow(&id, "mac");
    }
    assert_eq!(r.ran().len(), 6);
}

#[test]
fn a_phone_may_allow_a_stop_of_another_job_but_not_a_tail_or_return() {
    let r = rig("phone-stop");
    for op in [ops::JOBS_TAIL, ops::JOBS_RETURN] {
        let reply = r.call(op, json!({"id": "j-other"})).unwrap();
        let id = r.card_of(&reply);
        let refused = r
            .consent
            .decide(&id, true, None, "phone", None)
            .unwrap_err();
        assert_eq!(refused.code, ErrorCode::Forbidden, "{op}");
    }
    let stop = r.call(ops::JOBS_STOP, json!({"id": "j-other"})).unwrap();
    let id = r.card_of(&stop);
    assert_eq!(r.allow(&id, "phone").state.as_str(), "applied");
    assert_eq!(r.ran(), ["stop"]);
}

#[test]
fn a_machine_or_secret_of_another_organization_is_refused() {
    let r = rig("org");
    let mut abroad = args_for("shell", "abroad");
    abroad["machine"] = json!("abroad");
    abroad["node"] = json!("fedcba9876543210");
    let e = r.call(ops::CLUSTER_DISPATCH, abroad).unwrap_err();
    assert_eq!(reason(&e), "other_organization");
    let mut stolen = args_for("shell", "stolen");
    stolen["secrets"] = json!(["work-token"]);
    let e = r.call(ops::CLUSTER_DISPATCH, stolen).unwrap_err();
    assert_eq!(reason(&e), "other_organization");
    let mut missing = args_for("shell", "missing");
    missing["secrets"] = json!(["no-such"]);
    let e = r.call(ops::CLUSTER_DISPATCH, missing).unwrap_err();
    assert_eq!(reason(&e), "no_secret");
    r.fleet.jobs.lock().unwrap().push(job("j-work", "Work"));
    let e = r.call(ops::JOBS_STOP, json!({"id": "j-work"})).unwrap_err();
    assert_eq!(reason(&e), "other_organization");
    assert!(r.consent.store().open_rows().unwrap().is_empty());
}

#[test]
fn a_dispatch_with_secrets_names_them_on_the_card() {
    let r = rig("secrets");
    let mut args = args_for("shell", "with-secrets");
    args["secrets"] = json!(["deploy-token"]);
    let reply = r.call(ops::CLUSTER_DISPATCH, args).unwrap();
    let lines: Vec<&str> = reply["request"]["facts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["line"].as_str())
        .collect();
    assert!(lines.contains(&"consent-job-secrets"));
    let id = r.card_of(&reply);
    r.allow(&id, "mac");
    assert_eq!(
        r.hands.calls.lock().unwrap()[0].1["secrets"],
        json!(["deploy-token"])
    );
}

#[test]
fn the_cluster_switch_off_refuses_every_job_op() {
    let r = rig("off");
    r.on.store(false, Ordering::SeqCst);
    let e = r
        .call(ops::CLUSTER_DISPATCH, args_for("shell", "off"))
        .unwrap_err();
    assert_eq!(reason(&e), "switch_off");
    let e = r.call(ops::JOBS_STOP, json!({"id": "j-own"})).unwrap_err();
    assert_eq!(reason(&e), "switch_off");
}

#[test]
fn jobs_show_own_in_full_and_others_by_state_only() {
    let r = rig("reads");
    let mine = r.call(ops::JOBS_MINE, json!({})).unwrap();
    let rows = mine["jobs"].as_array().unwrap();
    let own = rows.iter().find(|j| j["id"] == "j-own").unwrap();
    assert_eq!(own["own"], true);
    assert_eq!(own["detail"], "secret detail");
    let other = rows.iter().find(|j| j["id"] == "j-other").unwrap();
    assert_eq!(
        other,
        &json!({"id": "j-other", "own": false, "state": "running"})
    );
    let got = r.call(ops::JOBS_GET, json!({"id": "j-other"})).unwrap();
    assert_eq!(
        got,
        json!({"id": "j-other", "own": false, "state": "running"})
    );
    let full = r.call(ops::JOBS_GET, json!({"id": "j-own"})).unwrap();
    assert_eq!(full["folder"], "/work/a");
    assert_eq!(full["secrets"], json!(["deploy-token"]));
}

#[test]
fn a_delegate_is_recorded_listed_and_asks_nothing() {
    let r = rig("delegate");
    let begun = r
        .call(
            ops::DELEGATE_BEGIN,
            json!({"tool": "codex", "folder": "/work/a"}),
        )
        .unwrap();
    let id = begun["id"].as_str().unwrap().to_string();
    assert!(r.consent.store().open_rows().unwrap().is_empty());
    let listed = r.call(ops::JOBS_MINE, json!({})).unwrap();
    let row = listed["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == id.as_str())
        .unwrap();
    assert_eq!(row["kind"], "delegate");
    assert_eq!(row["state"], "running");
    let ended = r
        .call(ops::DELEGATE_END, json!({"id": id, "exit": 0}))
        .unwrap();
    assert_eq!(ended["ended"], true);
    let got = r.call(ops::JOBS_GET, json!({"id": id})).unwrap();
    assert_eq!(got["state"], "done");
    let e = r
        .call(ops::DELEGATE_END, json!({"id": "j-other", "exit": 0}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
}
