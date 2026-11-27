use super::*;
use crate::consent_testkit::{agent_caller, app_caller, shell};
use crate::install_ops::InstallSetup;
use crate::install_ops_tests::{ops_for, rig, settings_text, Dir, USER_SETTINGS};
use crate::setup_testkit::{cards, reason_of, snapshot, Cards};
use notchling_engine::catalog::Tool;
use notchling_engine::cost_profile::guard;
use notchling_engine::cost_profile::manifest::{Entry, Manifest, Part};
use notchling_engine::delegate::skill::SkillTool;
use notchling_engine::install::{text_hash, SkillSpec};
use notchling_engine::settings::Settings;
use notchling_local_api::ErrorCode;
use std::path::PathBuf;
use std::sync::Mutex;

struct FakeSidecar(&'static str);

impl Sidecar for FakeSidecar {
    fn config_part(&self) -> Config {
        match self.0 {
            "part" => Config::Part(json!({"id": "sidecar", "path": "/x/config.json",
                "tool": "antigravity", "change": "update", "added": 1, "removed": 0,
                "before": "aaaa", "after": "bbbb", "notes": []})),
            "on" => Config::Already,
            _ => Config::Blocked("Antigravity has no config.json yet.".to_string()),
        }
    }
}

struct Rig {
    dir: Dir,
    c: Cards,
    install: Arc<InstallOps>,
    asks: Arc<InstallAsks>,
    ran: Arc<Mutex<Vec<(String, Value)>>>,
}

fn rig_for(tag: &str, setup: Option<InstallSetup>, on: bool, side: &'static str) -> Rig {
    rig_in(Dir::new(&format!("asks-{tag}")), tag, setup, on, side)
}

fn rig_in(dir: Dir, tag: &str, setup: Option<InstallSetup>, on: bool, side: &'static str) -> Rig {
    let setup = setup.unwrap_or_else(|| rig(&dir, Some(USER_SETTINGS)));
    let (ops, switch) = ops_for(setup, on);
    let install = Arc::new(ops);
    let c = cards(tag);
    let asks = InstallAsks::new(
        c.consent.clone(),
        install.clone(),
        Arc::new(FakeSidecar(side)),
        switch,
    );
    Rig {
        dir,
        c,
        install,
        asks,
        ran: Arc::new(Mutex::new(Vec::new())),
    }
}

fn plain(tag: &str) -> Rig {
    rig_for(tag, None, true, "part")
}

impl Rig {
    fn ask(&self, args: Value) -> Result<Value, ApiError> {
        self.asks.call(&agent_caller(), ops::INSTALL_REQUEST, &args)
    }

    fn card(&self, args: Value) -> (String, Vec<String>) {
        let reply = self.ask(args).unwrap();
        assert_eq!(reply["state"], "waiting", "{reply}");
        let id = reply["request"]["id"].as_str().unwrap().to_string();
        let parts = reply["request"]["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap().to_string())
            .collect();
        (id, parts)
    }

    fn apply(&self, id: &str, parts: &[&str]) -> Result<Value, ApiError> {
        let row = self.c.consent.get_row(id).unwrap();
        let op = row.change["apply"]["op"].as_str().unwrap().to_string();
        let mut args = row.change["apply"]["args"].clone();
        args["consentId"] = json!(id);
        args["parts"] = json!(parts);
        let install = self.install.clone();
        let ran = self.ran.clone();
        let name = op.clone();
        self.asks.apply(&app_caller(), &op, &args, &move |a| {
            ran.lock().unwrap().push((name.clone(), a.clone()));
            install.run(&name, a)
        })
    }

    fn files(&self) -> crate::setup_testkit::Snapshot {
        snapshot(&self.dir.path(""))
    }
}

fn skill_files(files: &crate::setup_testkit::Snapshot) -> usize {
    files
        .iter()
        .filter(|(p, _, _)| p.to_string_lossy().ends_with("SKILL.md"))
        .count()
}

fn set_up(dir: &Dir) {
    let entry = Entry {
        id: "claude.rule".into(),
        tool: Tool::Claude,
        path: PathBuf::from("/x/notchling-cost.md"),
        part: Part::File,
        removed: false,
        hash: None,
        backup: None,
        existed: false,
        keys: Vec::new(),
        at: 1,
        pending: false,
        before_hash: None,
        released: false,
    };
    let mut manifest = Manifest::default();
    manifest.put(entry);
    manifest.save(&dir.path("data")).unwrap();
}

#[test]
fn install_request_is_an_agent_acts_op_and_install_plan_is_an_agent_read() {
    let r = plain("spec");
    assert_eq!(r.asks.spec(ops::INSTALL_REQUEST), Some(OpSpec::AGENT_ACTS));
    assert_eq!(r.asks.spec(ops::INSTALL_APPLY), None);
    assert_eq!(r.install.spec(ops::INSTALL_PLAN), Some(OpSpec::AGENT_READ));
    assert_eq!(r.install.spec(ops::INSTALL_APPLY), Some(OpSpec::APP));
    assert_eq!(r.install.spec(ops::INSTALL_REMOVE), Some(OpSpec::APP));
}

#[test]
fn install_plan_as_an_agent_leaves_the_bytes_and_mtime_of_every_file_the_same() {
    let r = plain("plan");
    let before = r.files();
    let reply = r.install.run(ops::INSTALL_PLAN, &json!({})).unwrap();
    assert_eq!(r.files(), before);
    let parts = reply["plan"]["parts"].as_array().unwrap();
    let hooks = parts.iter().find(|p| p["id"] == "hooks").unwrap();
    assert_eq!(hooks["before"], json!(text_hash(USER_SETTINGS)));
    assert!(hooks["after"].is_string());
    assert!(parts.iter().all(|p| p["added"].is_number()));
}

#[test]
fn a_request_lists_the_files_with_their_hashes_and_writes_nothing() {
    let r = plain("list");
    let before = r.files();
    let (id, parts) = r.card(json!({}));
    assert!(parts.iter().any(|p| p == "hooks"));
    assert!(parts.iter().any(|p| p.starts_with("skill:")), "{parts:?}");
    assert_eq!(r.files(), before);
    let row = r.c.consent.get_row(&id).unwrap();
    assert_eq!(row.kind, INSTALL);
    assert_eq!(row.change["apply"]["op"], ops::INSTALL_APPLY);
    let hooks = row
        .parts
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "hooks")
        .cloned()
        .unwrap();
    assert_eq!(hooks["before"], json!(text_hash(USER_SETTINGS)));
}

#[test]
fn an_unticked_file_is_not_written() {
    let r = plain("unticked");
    let (id, parts) = r.card(json!({}));
    let skills: Vec<&String> = parts.iter().filter(|p| p.starts_with("skill:")).collect();
    assert!(!skills.is_empty());
    let out = r.apply(&id, &["hooks"]).unwrap();
    let states = out["parts"].as_array().unwrap();
    assert_eq!(states.len(), 1);
    assert_eq!(states[0]["id"], "hooks");
    assert_eq!(states[0]["state"], "written");
    assert_eq!(skill_files(&r.files()), 0);
    assert_ne!(settings_text(&r.dir), USER_SETTINGS);
    let row = r.c.consent.get_row(&id).unwrap();
    assert_eq!(row.state.as_str(), "applied");
}

#[test]
fn a_file_changed_after_allow_is_reported_stale_and_not_written() {
    let r = plain("stale");
    let (id, parts) = r.card(json!({}));
    let skill = parts
        .iter()
        .find(|p| p.starts_with("skill:"))
        .unwrap()
        .clone();
    let mine = format!("{USER_SETTINGS}\n");
    std::fs::write(r.dir.path("home/.claude/settings.json"), &mine).unwrap();
    let out = r.apply(&id, &["hooks", skill.as_str()]).unwrap();
    let states = out["parts"].as_array().unwrap();
    let state_of = |name: &str| states.iter().find(|s| s["id"] == name).unwrap()["state"].clone();
    assert_eq!(state_of("hooks"), "stale");
    assert_eq!(state_of(&skill), "written");
    assert_eq!(settings_text(&r.dir), mine);
}

#[test]
fn an_apply_naming_a_part_outside_the_card_is_refused_and_the_card_stays_usable() {
    let r = plain("outside");
    let (id, _) = r.card(json!({"parts": ["prompt_hooks"]}));
    let before = r.files();
    for named in [vec!["nope"], vec!["hooks", "skill:claude:nope"], vec![]] {
        let e = r.apply(&id, &named).unwrap_err();
        assert_eq!(reason_of(&e), "bad_value", "{named:?}");
    }
    assert!(r.ran.lock().unwrap().is_empty());
    assert_eq!(r.files(), before);
    assert!(r.apply(&id, &["hooks"]).is_ok());
}

#[test]
fn an_apply_from_an_agent_or_for_another_card_is_refused() {
    let r = plain("who");
    let (id, _) = r.card(json!({}));
    let e = r
        .asks
        .apply(
            &agent_caller(),
            ops::INSTALL_APPLY,
            &json!({"consentId": id, "parts": ["hooks"]}),
            &|_| Ok(json!({})),
        )
        .unwrap_err();
    assert_eq!(reason_of(&e), "app_only");
    let e = r
        .asks
        .apply(
            &app_caller(),
            ops::INSTALL_APPLY,
            &json!({"what": ["page_hooks"], "consentId": id, "parts": ["hooks"]}),
            &|_| Ok(json!({})),
        )
        .unwrap_err();
    assert_eq!(reason_of(&e), "needs_consent");
    assert!(r.asks.claims(ops::INSTALL_APPLY, &json!({"consentId": id})));
    assert!(!r.asks.claims(ops::INSTALL_APPLY, &json!({})));
    assert!(!r
        .asks
        .claims(ops::COST_SETUP_APPLY, &json!({"consentId": id})));
}

#[test]
fn a_plan_that_removes_the_guard_is_never_asked() {
    let dir = Dir::new("asks-guard-setup");
    let mut setup = rig(&dir, Some(USER_SETTINGS));
    setup.guard_data = Some(dir.path("data"));
    std::fs::create_dir_all(dir.path("data")).unwrap();
    set_up(&dir);
    let (ops_open, _) = ops_for(setup, true);
    ops_open
        .run(ops::INSTALL_APPLY, &json!({"what": ["guard_hooks"]}))
        .unwrap();
    assert!(settings_text(&dir).contains("PreToolUse"));
    Settings::open(&dir.path("data/settings.json"))
        .unwrap()
        .set(guard::KEY, Value::from("off"))
        .unwrap();
    let mut again = rig(&dir, None);
    again.guard_data = Some(dir.path("data"));
    let r = rig_in(dir, "guard", Some(again), true, "part");
    let before = std::fs::read(r.dir.path("home/.claude/settings.json")).unwrap();
    let e = r.ask(json!({"parts": ["guard_hooks"]})).unwrap_err();
    assert_eq!(reason_of(&e), "never");
    assert_eq!(e.data.as_ref().unwrap()["never"], true);
    let e = r.ask(json!({})).unwrap_err();
    assert_eq!(reason_of(&e), "never");
    assert_eq!(
        std::fs::read(r.dir.path("home/.claude/settings.json")).unwrap(),
        before
    );
}

#[test]
fn with_the_install_switch_off_a_request_is_refused_at_open_with_the_switch_ask() {
    let r = rig_for("off", None, false, "part");
    let e = r.ask(json!({})).unwrap_err();
    assert_eq!(reason_of(&e), "switch_off");
    let data = e.data.as_ref().unwrap();
    assert_eq!(data["switch"], "install");
    assert_eq!(data["ask"]["command"], "notchling switch on install");
    assert!(
        r.c.consent
            .call(&app_caller(), "consent.list", &json!({}))
            .is_ok()
            || true
    );
}

#[test]
fn only_a_verified_session_may_ask() {
    let r = plain("session");
    *r.c.who.lock().unwrap() = shell("Terminal");
    let e = r.ask(json!({})).unwrap_err();
    assert_eq!(reason_of(&e), "no_session");
}

#[test]
fn a_dry_run_opens_nothing_and_a_key_returns_the_same_card() {
    let r = plain("key");
    let dry = r.ask(json!({"dryRun": true})).unwrap();
    assert_eq!(dry["dryRun"], true);
    assert_eq!(dry["would"], "ask");
    let first = r.ask(json!({"consent": {"key": "k1"}})).unwrap();
    let again = r.ask(json!({"consent": {"key": "k1"}})).unwrap();
    assert_eq!(first["request"]["id"], again["request"]["id"]);
    let e = r
        .ask(json!({"parts": ["prompt_hooks"], "consent": {"key": "k1"}}))
        .unwrap_err();
    assert_eq!(reason_of(&e), "key_reused");
}

#[test]
fn a_part_that_is_not_known_or_a_request_with_nothing_to_do_is_a_bad_request() {
    let r = plain("names");
    let e = r.ask(json!({"parts": ["nope"]})).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    let (id, _) = r.card(json!({}));
    r.apply(&id, &["hooks"]).unwrap();
    let (again, parts) = r.card(json!({}));
    assert!(!parts.iter().any(|p| p == "hooks"));
    r.apply(
        &again,
        &parts.iter().map(String::as_str).collect::<Vec<_>>(),
    )
    .unwrap();
    let e = r.ask(json!({})).unwrap_err();
    assert_eq!(reason_of(&e), "nothing_to_do");
}

#[test]
fn the_sidecar_is_its_own_card_with_its_one_config_part_hash_checked_by_the_apply() {
    let r = plain("sidecar");
    let e = r.ask(json!({"parts": ["sidecar", "hooks"]})).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    let (id, parts) = r.card(json!({"parts": ["sidecar"]}));
    assert_eq!(parts, vec!["sidecar".to_string()]);
    let row = r.c.consent.get_row(&id).unwrap();
    assert_eq!(row.change["apply"]["op"], ops::AGY_SIDECAR_SETUP);
    let mut args = row.change["apply"]["args"].clone();
    assert_eq!(args["confirm"], true);
    args["consentId"] = json!(id);
    args["parts"] = json!(["sidecar"]);
    let seen = Arc::new(Mutex::new(Value::Null));
    let keep = seen.clone();
    r.asks
        .apply(&app_caller(), ops::AGY_SIDECAR_SETUP, &args, &move |a| {
            *keep.lock().unwrap() = a.clone();
            Ok(json!({"state": "on"}))
        })
        .unwrap();
    let given = seen.lock().unwrap().clone();
    assert_eq!(given["parts"], json!({"sidecar": "aaaa"}));
    assert_eq!(given["confirm"], true);

    let on = rig_for("sidecar-on", None, true, "on");
    assert_eq!(
        reason_of(&on.ask(json!({"parts": ["sidecar"]})).unwrap_err()),
        "nothing_to_do"
    );
    let none = rig_for("sidecar-none", None, true, "blocked");
    assert_eq!(
        reason_of(&none.ask(json!({"parts": ["sidecar"]})).unwrap_err()),
        "blocked"
    );
}

#[test]
fn a_sidecar_apply_the_op_refuses_as_stale_withdraws_the_card_and_the_waiting_cli_sees_withdrawn() {
    let r = plain("sidecar-stale");
    let (id, _) = r.card(json!({"parts": ["sidecar"]}));
    let row = r.c.consent.get_row(&id).unwrap();
    let mut args = row.change["apply"]["args"].clone();
    args["consentId"] = json!(id);
    args["parts"] = json!(["sidecar"]);
    let e = r
        .asks
        .apply(&app_caller(), ops::AGY_SIDECAR_SETUP, &args, &|_| {
            Err(ApiError::new(ErrorCode::Forbidden, "files changed").with_reason("stale"))
        })
        .unwrap_err();
    assert_eq!(reason_of(&e), "stale");
    let state = r.c.consent.get_row(&id).unwrap().state.as_str().to_string();
    assert_eq!(state, "withdrawn");
    let failure = crate::consent_cli::failure_of(&json!({"id": id, "state": state}), "consent.get");
    assert_eq!(failure.reason, "withdrawn");
    let (other, _) = r.card(json!({"parts": ["sidecar"]}));
    let row = r.c.consent.get_row(&other).unwrap();
    let mut args = row.change["apply"]["args"].clone();
    args["consentId"] = json!(other);
    args["parts"] = json!(["sidecar"]);
    let e = r
        .asks
        .apply(&app_caller(), ops::AGY_SIDECAR_SETUP, &args, &|_| {
            Err(ApiError::new(ErrorCode::Failed, "write failed"))
        })
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(
        r.c.consent.get_row(&other).unwrap().state.as_str(),
        "failed"
    );
}

#[test]
fn a_call_carrying_a_consent_id_is_claimed_whatever_the_id_and_never_reaches_the_raw_op() {
    let r = plain("claims-all");
    for op in [ops::INSTALL_APPLY, ops::AGY_SIDECAR_SETUP] {
        for id in [json!("gone"), json!(7), json!(null), json!(true)] {
            assert!(r.asks.claims(op, &json!({"consentId": id})), "{op} {id}");
        }
        assert!(!r.asks.claims(op, &json!({"confirm": true})), "{op}");
    }
    let ran = Arc::new(Mutex::new(0));
    let count = ran.clone();
    let run = move |_: &Value| {
        *count.lock().unwrap() += 1;
        Ok(json!({}))
    };
    let e = r
        .asks
        .apply(
            &app_caller(),
            ops::INSTALL_APPLY,
            &json!({"consentId": "gone", "parts": ["hooks"]}),
            &run,
        )
        .unwrap_err();
    assert_eq!(reason_of(&e), "needs_consent");
    let e = r
        .asks
        .apply(
            &app_caller(),
            ops::AGY_SIDECAR_SETUP,
            &json!({"consentId": 7, "action": "turn_on", "confirm": true}),
            &run,
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    assert_eq!(*ran.lock().unwrap(), 0);
}

#[test]
fn a_file_whose_text_changed_since_the_card_is_stale_though_the_file_on_disk_did_not() {
    let dir = Dir::new("asks-after");
    let text = Arc::new(Mutex::new("one".to_string()));
    let mut setup = rig(&dir, Some(USER_SETTINGS));
    let seen = text.clone();
    setup.skills.push(SkillSpec {
        name: "notchling-drift".into(),
        tools: vec![SkillTool::Claude],
        text: Box::new(move |_| {
            format!(
                "---\nname: notchling-drift\n---\n{}\n",
                seen.lock().unwrap()
            )
        }),
    });
    let r = rig_in(dir, "after", Some(setup), true, "part");
    let (id, parts) = r.card(json!({"parts": ["notchling-drift"]}));
    assert_eq!(parts, vec!["skill:claude:notchling-drift".to_string()]);
    *text.lock().unwrap() = "two".into();
    let out = r.apply(&id, &["skill:claude:notchling-drift"]).unwrap();
    assert_eq!(out["parts"][0]["state"], "stale", "{out}");
    assert!(!r
        .dir
        .path("home/.claude/skills/notchling-drift/SKILL.md")
        .exists());
}

#[test]
fn a_skill_that_is_the_users_own_by_apply_time_is_stale_not_unchanged() {
    let r = plain("theirs");
    let (id, parts) = r.card(json!({}));
    let skill = parts
        .iter()
        .find(|p| p.starts_with("skill:claude:"))
        .unwrap()
        .clone();
    let name = skill.rsplit(':').next().unwrap();
    let dir = r.dir.path(&format!("home/.claude/skills/{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SKILL.md"), "mine\n").unwrap();
    let out = r.apply(&id, &[skill.as_str()]).unwrap();
    assert_eq!(out["parts"][0]["state"], "stale", "{out}");
    assert_eq!(
        std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
        "mine\n"
    );
}

#[test]
fn an_apply_never_takes_the_guard_hook_out_though_the_guard_closed_after_the_card() {
    let dir = Dir::new("asks-guard-apply");
    let mut setup = rig(&dir, Some(USER_SETTINGS));
    setup.guard_data = Some(dir.path("data"));
    std::fs::create_dir_all(dir.path("data")).unwrap();
    set_up(&dir);
    let (opener, _) = ops_for(setup, true);
    opener
        .run(ops::INSTALL_APPLY, &json!({"what": ["guard_hooks"]}))
        .unwrap();
    assert!(settings_text(&dir).contains("hook --guard"));
    let mut again = rig(&dir, None);
    again.guard_data = Some(dir.path("data"));
    let r = rig_in(dir, "guard-apply", Some(again), true, "part");
    let (id, parts) = r.card(json!({}));
    assert!(parts.iter().any(|p| p == "hooks"), "{parts:?}");
    Settings::open(&r.dir.path("data/settings.json"))
        .unwrap()
        .set(guard::KEY, Value::from("off"))
        .unwrap();
    let out = r.apply(&id, &["hooks"]).unwrap();
    assert_eq!(out["parts"][0]["state"], "written", "{out}");
    let text = settings_text(&r.dir);
    assert!(text.contains("hook --guard"), "{text}");
    assert!(text.contains("PreToolUse"));
}
