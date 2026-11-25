use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::consent_testkit::{agent_caller, app_caller, shell, verified};
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::versions_check_tests::{native_claude, out, rig as versions_rig, Rig as Versions};
use notchling_engine::consent::ConsentStore;
use notchling_engine::store;
use notchling_events::{MemorySink, Salt};
use notchling_local_api::topics;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

struct Rig {
    v: Versions,
    consent: Arc<ConsentOps>,
    asks: Arc<UpdateAsks>,
    on: Arc<AtomicBool>,
    who: Arc<Mutex<AgentIdentity>>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.v.ops.wait_updates();
        self.consent.close();
    }
}

fn rig(tag: &str) -> Rig {
    let v = versions_rig(&format!("update-asks-{tag}"));
    let room = Arc::new(FakeRoom::default());
    let inbox = InboxOps::new(v.tmp.path("inbox"), room.clone());
    let consent = ConsentOps::with_clock(
        Arc::new(ConsentStore::in_memory_for_tests()),
        room,
        Some(inbox),
        Some(Events::fixed(Arc::new(MemorySink::new()), Salt([3; 32]))),
        Arc::new(crate::acting_ops::now_ms),
    );
    let who = Arc::new(Mutex::new(verified("parent")));
    let seen = who.clone();
    consent.bind_identity(Arc::new(move |_| seen.lock().unwrap().clone()));
    consent.start();
    v.ops
        .set_install_switch(crate::switches::SwitchFlag::in_memory(
            crate::switches::INSTALL,
            crate::switches::INSTALL_OFF,
            true,
        ));
    let on = Arc::new(AtomicBool::new(true));
    let flag = on.clone();
    let asks = UpdateAsks::new(
        consent.clone(),
        v.ops.clone(),
        Arc::new(move || flag.load(Ordering::SeqCst)),
    );
    Rig {
        v,
        consent,
        asks,
        on,
        who,
    }
}

fn reason(e: &ApiError) -> String {
    e.data
        .as_ref()
        .and_then(|d| d["reason"].as_str())
        .unwrap_or_default()
        .to_string()
}

fn seen(r: &Rig, entries: &[&str]) {
    store::put_doc(
        &r.v.tmp.path("data"),
        "versions/seen",
        &serde_json::to_string(entries).unwrap(),
    )
    .unwrap();
}

#[cfg(unix)]
fn installed(r: &Rig, version: &str) {
    native_claude(&r.v, version, &format!("claude {version}"));
    r.v.ops.rescan();
}

#[cfg(unix)]
fn with_latest(r: &Rig, latest: &str) {
    r.v.launcher.push_output(out(latest));
}

impl Rig {
    fn ask(&self, args: Value) -> Result<Value, ApiError> {
        self.asks
            .call(&agent_caller(), ops::VERSIONS_REQUEST, &args)
    }

    fn card(&self, args: Value) -> String {
        let reply = self.ask(args).unwrap();
        assert_eq!(reply["state"], "waiting", "{reply}");
        reply["request"]["id"].as_str().unwrap().to_string()
    }

    fn row(&self, id: &str) -> Consent {
        self.consent.get_row(id).unwrap()
    }

    fn open_count(&self) -> usize {
        self.consent.store().open_rows().unwrap().len()
    }

    fn apply(&self, id: &str) -> Result<Value, ApiError> {
        let row = self.row(id);
        let op = row.change["apply"]["op"].as_str().unwrap().to_string();
        let mut args = row.change["apply"]["args"].clone();
        args["consentId"] = json!(id);
        let versions = self.v.ops.clone();
        Cited::apply(&*self.asks, &app_caller(), &op, &args, &move |a| {
            versions.call(&app_caller(), ops::VERSIONS_UPDATE_LOCAL, a)
        })
    }
}

#[test]
fn the_request_is_an_agent_act_and_the_apply_op_is_claimed_only_with_a_card() {
    let r = rig("tier");
    assert_eq!(r.asks.spec(ops::VERSIONS_REQUEST), Some(OpSpec::AGENT_ACTS));
    assert_eq!(r.asks.spec(ops::VERSIONS_UPDATE_LOCAL), None);
    let cited: &dyn Cited = &*r.asks;
    assert!(cited.claims(
        ops::VERSIONS_UPDATE_LOCAL,
        &json!({"tool": "claude", "consentId": "c1"})
    ));
    assert!(!cited.claims(ops::VERSIONS_UPDATE_LOCAL, &json!({"tool": "claude"})));
    assert!(!cited.claims(ops::VERSIONS_GET, &json!({"consentId": "c1"})));
}

#[cfg(unix)]
#[test]
fn only_an_agent_inside_a_session_may_ask() {
    let r = rig("no-session");
    installed(&r, "2.1.324");
    *r.who.lock().unwrap() = shell("Terminal");
    let refused = r.ask(json!({"tool": "claude"})).unwrap_err();
    assert_eq!(reason(&refused), "no_session");
    assert_eq!(r.open_count(), 0);
}

#[test]
fn a_tool_that_is_not_installed_is_refused_at_open() {
    let r = rig("absent");
    r.v.ops.rescan();
    let refused = r.ask(json!({"tool": "codex"})).unwrap_err();
    assert_eq!(refused.code, ErrorCode::BadRequest);
    assert_eq!(reason(&refused), "not_installed");
    let refused = r.ask(json!({"tool": "nope"})).unwrap_err();
    assert_eq!(refused.code, ErrorCode::BadRequest);
    assert_eq!(r.open_count(), 0);
}

#[test]
fn notchling_itself_is_never_a_request() {
    let r = rig("self");
    r.v.ops.rescan();
    let refused = r.ask(json!({"tool": "notchling"})).unwrap_err();
    assert_eq!(reason(&refused), "cant_update");
    assert_eq!(r.open_count(), 0);
}

#[cfg(unix)]
#[test]
fn a_to_that_is_not_the_go_back_on_offer_is_refused_at_open() {
    let r = rig("not-back");
    installed(&r, "2.1.324");
    seen(&r, &["claude:2.1.322", "claude:2.1.323", "claude:2.1.324"]);
    for to in ["2.1.322", "2.1.325", "latest", "2.1.324"] {
        let refused = r.ask(json!({"tool": "claude", "to": to})).unwrap_err();
        assert_eq!(refused.code, ErrorCode::BadRequest, "{to}");
    }
    assert_eq!(
        reason(
            &r.ask(json!({"tool": "claude", "to": "2.1.322"}))
                .unwrap_err()
        ),
        "not_go_back"
    );
    assert_eq!(r.open_count(), 0);
    let id = r.card(json!({"tool": "claude", "to": "2.1.323"}));
    assert_eq!(r.row(&id).change["action"], "back");
}

#[cfg(unix)]
#[test]
fn a_brew_install_has_no_go_back_request() {
    let r = rig("brew");
    r.v.tmp.write(
        "home/opt/homebrew/Caskroom/claude-code/2.1.324/claude",
        "claude",
    );
    seen(&r, &["claude:2.1.323", "claude:2.1.324"]);
    r.v.ops.rescan();
    let refused = r
        .ask(json!({"tool": "claude", "to": "2.1.323"}))
        .unwrap_err();
    assert_eq!(reason(&refused), "not_go_back");
    assert_eq!(r.open_count(), 0);
}

#[cfg(unix)]
#[test]
fn install_off_refuses_at_open_and_names_the_switch_ask() {
    let r = rig("off");
    installed(&r, "2.1.324");
    with_latest(&r, "2.1.330");
    r.on.store(false, Ordering::SeqCst);
    let refused = r.ask(json!({"tool": "claude"})).unwrap_err();
    assert_eq!(refused.code, ErrorCode::Forbidden);
    assert_eq!(reason(&refused), "switch_off");
    let data = refused.data.unwrap();
    assert_eq!(data["switch"], "install");
    assert_eq!(data["ask"]["command"], "notchling switch on install");
    assert_eq!(r.open_count(), 0);
    assert!(r.v.launcher.calls().is_empty());
}

#[cfg(unix)]
#[test]
fn the_cards_facts_come_from_versions_state() {
    let r = rig("facts");
    installed(&r, "2.1.324");
    with_latest(&r, "2.1.330");
    let reply = r
        .ask(json!({"tool": "claude", "reason": "newer fixes"}))
        .unwrap();
    assert_eq!(reply["state"], "waiting");
    let id = reply["request"]["id"].as_str().unwrap();
    let row = r.row(id);
    assert_eq!(row.kind, UPDATE);
    assert_eq!(row.target, "claude:mac");
    assert_eq!(row.change["from"], "2.1.324");
    assert_eq!(row.change["to"], "2.1.330");
    assert_eq!(row.change["method"], "native");
    assert_eq!(row.change["action"], "update");
    assert_eq!(
        row.change["apply"],
        json!({"op": "versions.update_local", "args": {"tool": "claude"}})
    );
    assert_eq!(
        row.facts,
        json!([
            {"line": "consent-update-tool", "args": {
                "tool": "Claude Code", "method": "native installer",
                "from": "2.1.324", "to": "2.1.330"}},
            {"line": "consent-update-running", "args": {}},
        ])
    );
}

#[cfg(unix)]
#[test]
fn a_go_back_card_names_the_version_it_goes_to() {
    let r = rig("facts-back");
    installed(&r, "2.1.324");
    seen(&r, &["claude:2.1.322", "claude:2.1.323", "claude:2.1.324"]);
    let id = r.card(json!({"tool": "claude", "to": "2.1.323"}));
    let row = r.row(&id);
    assert_eq!(row.change["to"], "2.1.323");
    assert_eq!(
        row.change["apply"]["args"],
        json!({"tool": "claude", "to": "2.1.323"})
    );
    assert_eq!(row.facts[0]["line"], "consent-update-back");
    assert_eq!(row.facts[0]["args"]["from"], "2.1.324");
    assert_eq!(row.facts[0]["args"]["to"], "2.1.323");
}

#[cfg(unix)]
#[test]
fn an_unknown_newest_version_says_so_and_a_current_tool_is_refused() {
    let r = rig("latest");
    installed(&r, "2.1.324");
    let id = r.card(json!({"tool": "claude"}));
    assert_eq!(r.row(&id).facts[0]["line"], "consent-update-tool-latest");
    assert_eq!(r.row(&id).change["to"], Value::Null);
    let other = rig("current");
    installed(&other, "2.1.324");
    with_latest(&other, "2.1.324");
    let refused = other.ask(json!({"tool": "claude"})).unwrap_err();
    assert_eq!(reason(&refused), "up_to_date");
    assert_eq!(other.open_count(), 0);
}

#[cfg(unix)]
#[test]
fn a_worker_machine_goes_through_machines_request() {
    let r = rig("machine");
    installed(&r, "2.1.324");
    with_latest(&r, "2.1.330");
    let refused = r
        .ask(json!({"tool": "claude", "machine": "build"}))
        .unwrap_err();
    assert_eq!(reason(&refused), "use_machines");
    assert_eq!(r.open_count(), 0);
}

#[cfg(unix)]
#[test]
fn a_dry_run_and_a_repeated_key_open_one_card_at_most() {
    let r = rig("dry");
    installed(&r, "2.1.324");
    with_latest(&r, "2.1.330");
    let dry = r.ask(json!({"tool": "claude", "dryRun": true})).unwrap();
    assert_eq!(dry["dryRun"], true);
    assert_eq!(dry["would"], "ask");
    assert_eq!(r.open_count(), 0);
    let first = r.card(json!({"tool": "claude", "consent": {"key": "k1"}}));
    let again = r.card(json!({"tool": "claude", "consent": {"key": "k1"}}));
    assert_eq!(first, again);
    assert_eq!(r.open_count(), 1);
    seen(&r, &["claude:2.1.323", "claude:2.1.324"]);
    let reused = r
        .ask(json!({"tool": "claude", "to": "2.1.323", "consent": {"key": "k1"}}))
        .unwrap_err();
    assert_eq!(reason(&reused), "key_reused");
}

#[cfg(unix)]
#[test]
fn allow_applies_through_the_cited_update_local_call() {
    let r = rig("apply");
    installed(&r, "2.1.324");
    with_latest(&r, "2.1.330");
    let id = r.card(json!({"tool": "claude"}));
    r.v.launcher.push_output(out("updated"));
    let result = r.apply(&id).unwrap();
    assert_eq!(result["started"], true);
    r.v.ops.wait_updates();
    let last = r.v.launcher.commands().pop().unwrap();
    assert_eq!(last.argv[1], "update");
    assert_eq!(r.row(&id).state.as_str(), "applied");
    assert_eq!(r.row(&id).result.unwrap()["tool"], "claude");
    assert_eq!(
        r.v.room.named(topics::VERSIONS_UPDATED)[0]["tool"],
        "claude"
    );
}

#[cfg(unix)]
#[test]
fn a_version_that_moved_between_card_and_apply_gives_stale() {
    let r = rig("stale");
    installed(&r, "2.1.324");
    with_latest(&r, "2.1.330");
    let id = r.card(json!({"tool": "claude"}));
    installed(&r, "2.1.326");
    let before = r.v.launcher.calls().len();
    let refused = r.apply(&id).unwrap_err();
    assert_eq!(reason(&refused), "stale");
    r.v.ops.wait_updates();
    assert_eq!(r.v.launcher.calls().len(), before);
    assert_ne!(r.row(&id).state.as_str(), "applied");
}

#[cfg(unix)]
#[test]
fn a_go_back_target_that_moved_gives_stale() {
    let r = rig("stale-back");
    installed(&r, "2.1.324");
    seen(&r, &["claude:2.1.322", "claude:2.1.323", "claude:2.1.324"]);
    let id = r.card(json!({"tool": "claude", "to": "2.1.323"}));
    seen(&r, &["claude:2.1.324"]);
    let before = r.v.launcher.calls().len();
    let refused = r.apply(&id).unwrap_err();
    assert_eq!(reason(&refused), "stale");
    assert_eq!(r.v.launcher.calls().len(), before);
}

#[cfg(unix)]
#[test]
fn switching_install_off_after_asking_stops_the_apply() {
    let r = rig("off-later");
    installed(&r, "2.1.324");
    with_latest(&r, "2.1.330");
    let id = r.card(json!({"tool": "claude"}));
    r.on.store(false, Ordering::SeqCst);
    let before = r.v.launcher.calls().len();
    assert_eq!(reason(&r.apply(&id).unwrap_err()), "stale");
    assert_eq!(r.v.launcher.calls().len(), before);
}

#[cfg(unix)]
#[test]
fn an_id_works_only_for_its_own_args_and_only_for_the_app() {
    let r = rig("once");
    installed(&r, "2.1.324");
    with_latest(&r, "2.1.330");
    let id = r.card(json!({"tool": "claude"}));
    let other = Cited::apply(
        &*r.asks,
        &app_caller(),
        ops::VERSIONS_UPDATE_LOCAL,
        &json!({"tool": "codex", "consentId": id}),
        &|_| panic!("a different change never runs"),
    )
    .unwrap_err();
    assert_eq!(reason(&other), "needs_consent");
    let by_agent = Cited::apply(
        &*r.asks,
        &agent_caller(),
        ops::VERSIONS_UPDATE_LOCAL,
        &json!({"tool": "claude", "consentId": id}),
        &|_| panic!("an agent never reaches the op"),
    )
    .unwrap_err();
    assert_eq!(reason(&by_agent), "app_only");
    assert_eq!(r.row(&id).state.as_str(), "waiting");
}

#[test]
fn a_consent_id_that_is_not_text_is_claimed_and_refused_before_the_raw_op() {
    let r = rig("non-text");
    let cited: &dyn Cited = &*r.asks;
    for id in [json!(7), json!(null), json!(true), json!(["c1"])] {
        let args = json!({"tool": "claude", "consentId": id});
        assert!(cited.claims(ops::VERSIONS_UPDATE_LOCAL, &args), "{id}");
        let e = Cited::apply(
            &*r.asks,
            &app_caller(),
            ops::VERSIONS_UPDATE_LOCAL,
            &args,
            &|_| Err(ApiError::internal("the raw op ran")),
        )
        .unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{id}");
    }
}
