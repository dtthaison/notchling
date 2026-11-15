use std::sync::{Arc, Mutex};

use notchling_engine::cluster::grants::{SecretAllow, MAX_ALLOWED_NAMES};
use notchling_engine::vault::entry::{Kind, VaultEntry, VaultValue};
use notchling_local_api::{ops, ApiError, ClientError, ErrorCode};
use serde_json::{json, Value};

use super::model::{ClusterModel, Login};
use super::renders::{allow, status, vault_names, ME, STANDBY, WORKER};
use super::secrets::{
    save_request, store_for, summary, target, vault_names as read_names, SecretsModel,
    SecretsPicker, Target,
};
use crate::actions::Transport;
use crate::switches::Switches;
use crate::vault::store::Vault;

const MARKER: &str = "NL-MARKER-9";

fn model_of(status: &Value) -> ClusterModel {
    ClusterModel::from_status(status, ME)
}

fn names() -> Vec<String> {
    vault_names()
}

fn without_secrets_field() -> Value {
    let mut status = status();
    for member in status["members"].as_array_mut().expect("members") {
        member.as_object_mut().expect("member").remove("secrets");
    }
    status
}

#[test]
fn new_machine_allows_nothing_and_any_is_off() {
    let model = model_of(&without_secrets_field());
    let row = model.row(STANDBY).expect("standby");
    assert_eq!(row.secrets, SecretAllow::default());
    let edit = SecretsModel::new(&names(), &row.secrets);
    assert!(edit.rows().iter().all(|r| !r.on && r.in_vault));
    assert!(!edit.any());
    assert!(!edit.is_changed());
    assert_eq!(edit.request(), json!({"names": [], "any": false}));
    assert_eq!(summary(&row.secrets), "No secrets");
    let listed = model_of(&status());
    assert_eq!(
        summary(&listed.row(WORKER).expect("worker").secrets),
        "2 secrets"
    );
    assert_eq!(
        summary(&listed.row(STANDBY).expect("standby").secrets),
        "No secrets"
    );
}

#[test]
fn toggle_builds_allow_request() {
    let model = model_of(&status());
    let row = model.row(WORKER).expect("worker");
    let mut edit = SecretsModel::new(&names(), &row.secrets);
    assert_eq!(
        edit.request(),
        json!({"names": ["github", "registry"], "any": false})
    );
    edit.toggle("github");
    edit.toggle("NPM");
    assert!(edit.is_changed());
    let request = model.set_secrets(WORKER, &edit).expect("worker offers it");
    assert_eq!(request.op, ops::CLUSTER_ALLOW_SET);
    assert_eq!(
        request.args,
        json!({"node": WORKER, "names": ["npm", "registry"], "any": false})
    );
    edit.toggle("github");
    edit.toggle("npm");
    assert!(!edit.is_changed());
    assert_eq!(model.set_secrets(ME, &edit), None);
    assert_eq!(model.set_secrets("0000000000000000", &edit), None);
}

#[test]
fn a_name_gone_from_the_vault_stays_until_it_is_unchecked() {
    let mut status = status();
    allow(&mut status, WORKER, &["github", "retired"], false);
    let model = model_of(&status);
    let mut edit = SecretsModel::new(&names(), &model.row(WORKER).expect("worker").secrets);
    let gone: Vec<&str> = edit
        .rows()
        .iter()
        .filter(|r| !r.in_vault)
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(gone, ["retired"]);
    assert_eq!(
        edit.request(),
        json!({"names": ["github", "retired"], "any": false})
    );
    edit.toggle("retired");
    assert_eq!(edit.request(), json!({"names": ["github"], "any": false}));
    assert!(edit.rows().iter().all(|r| r.in_vault));
}

#[test]
fn the_list_stops_at_the_daemons_cap() {
    let many: Vec<String> = (0..MAX_ALLOWED_NAMES + 3)
        .map(|n| format!("s{n}"))
        .collect();
    let mut edit = SecretsModel::new(&many, &SecretAllow::default());
    for name in &many {
        edit.toggle(name);
    }
    let sent = edit.request()["names"].as_array().expect("names").len();
    assert_eq!(sent, MAX_ALLOWED_NAMES);
}

#[test]
fn any_hides_the_list() {
    let mut edit = SecretsModel::new(&names(), &SecretAllow::new(["github".to_owned()], false));
    assert!(edit.shows_list());
    edit.set_any(true);
    assert!(!edit.shows_list());
    assert!(edit.is_changed());
    assert_eq!(
        edit.request(),
        json!({"names": ["github"], "any": true}),
        "the checked names are kept for when Any Secret goes off"
    );
    edit.set_any(false);
    assert!(edit.shows_list());
    assert!(!edit.is_changed());
    assert_eq!(
        summary(&SecretAllow::new(Vec::new(), true)),
        "Any secret",
        "the row says so"
    );
}

#[test]
fn picker_offers_only_allowed_names() {
    let allowed = SecretAllow::new(["github".to_owned(), "retired".to_owned()], false);
    let mut picker = SecretsPicker::new(&names(), &allowed, None);
    assert_eq!(picker.offered(), ["github"]);
    assert!(!picker.is_empty());
    picker.toggle("npm");
    assert!(picker.attach().is_empty(), "npm isn't offered");
    picker.toggle("GitHub");
    assert!(picker.is_on("github"));
    assert_eq!(picker.attach(), ["github"]);
    let args = picker.with_secrets(json!({"node": WORKER, "key": "ns-1"}));
    assert_eq!(args["secrets"], json!(["github"]));
    picker.toggle("github");
    assert_eq!(
        picker.with_secrets(json!({"node": WORKER})),
        json!({"node": WORKER}),
        "nothing chosen sends no list"
    );
    let any = SecretsPicker::new(&names(), &SecretAllow::new(Vec::new(), true), None);
    assert_eq!(any.offered(), names().as_slice());
    let none = SecretsPicker::new(&names(), &SecretAllow::default(), None);
    assert!(none.is_empty());
}

#[test]
fn picker_stops_at_sixteen() {
    let many: Vec<String> = (0..20).map(|n| format!("s{n}")).collect();
    let mut picker = SecretsPicker::new(&many, &SecretAllow::new(Vec::new(), true), None);
    for name in &many {
        picker.toggle(name);
    }
    assert_eq!(picker.attach().len(), 16);
}

#[test]
fn picker_says_new_secrets_wait_off_primary() {
    let primary = model_of(&status());
    let vault = primary.vault_node().expect("vault node");
    assert!(vault.primary && vault.me);
    let picker = SecretsPicker::new(&names(), &SecretAllow::default(), primary.vault_node());
    assert_eq!(picker.note(), None);

    let mut moved = status();
    moved["cluster"]["home"] = json!(STANDBY);
    let after = model_of(&moved);
    let vault = after.vault_node().expect("vault node");
    assert!(!vault.primary);
    let picker = SecretsPicker::new(&names(), &SecretAllow::default(), after.vault_node());
    assert_eq!(picker.note(), Some("New secrets wait for MacBook"));
    assert_eq!(
        SecretsPicker::new(&names(), &SecretAllow::default(), None).note(),
        None,
        "no cluster, no wait"
    );
}

#[test]
fn node_row_shows_login_states() {
    let model = model_of(&status());
    let worker = model.row(WORKER).expect("worker");
    assert_eq!(
        worker.logins,
        [
            ("Claude Code".to_owned(), Login::NotInstalled),
            ("Codex".to_owned(), Login::Unknown),
            ("Antigravity".to_owned(), Login::SignedOut),
        ]
    );
    let standby = model.row(STANDBY).expect("standby");
    assert_eq!(
        standby.logins,
        [
            ("Claude Code".to_owned(), Login::Unknown),
            ("Codex".to_owned(), Login::SignedIn),
            ("Antigravity".to_owned(), Login::SignedIn),
        ],
        "Claude has no entry on the node: unknown, never faked"
    );
    let texts: Vec<String> = standby.login_texts().into_iter().map(|(t, _)| t).collect();
    assert_eq!(
        texts,
        [
            "Claude Code: Unknown",
            "Codex: Signed In",
            "Antigravity: Signed In"
        ]
    );
    let words: Vec<&str> = [
        Login::SignedIn,
        Login::SignedOut,
        Login::NotInstalled,
        Login::Unknown,
    ]
    .map(Login::word)
    .to_vec();
    assert_eq!(
        words,
        ["Signed In", "Signed Out", "Not Installed", "Unknown"]
    );
    let mut silent = status();
    silent["members"][0]["versions"] = Value::Null;
    let model = model_of(&silent);
    assert!(model.rows().iter().any(|r| r.logins.is_empty()));
}

#[test]
fn claude_is_shown_as_reported() {
    let mut reported = status();
    for member in reported["members"].as_array_mut().expect("members") {
        if member["id"] == STANDBY {
            member["versions"]["logins"]["claude"] = json!(false);
        }
    }
    let model = model_of(&reported);
    let claude = model.row(STANDBY).expect("standby").logins[0].1;
    assert_eq!(claude, Login::SignedOut);
}

#[test]
fn only_nodes_that_run_jobs_offer_secrets() {
    let model = model_of(&status());
    assert!(model.row(WORKER).expect("worker").offers_secrets());
    assert!(model.row(STANDBY).expect("standby").offers_secrets());
    assert!(!model.row(ME).expect("me").offers_secrets());
}

#[test]
fn remote_vault_tab_only_when_not_vault_node() {
    let here = model_of(&status());
    assert!(here.remote_vault().is_none(), "this Mac holds the vault");
    assert!(here.vault_node().is_some_and(|v| v.me));

    let mut elsewhere = status();
    elsewhere["cluster"]["vault"] = json!(STANDBY);
    let model = ClusterModel::from_status(&elsewhere, WORKER);
    let remote = model.remote_vault().expect("the vault is on mac-mini");
    assert_eq!(
        (remote.id.as_str(), remote.name.as_str()),
        (STANDBY, "mac-mini")
    );
    assert!(remote.online);

    let none = ClusterModel::from_status(&crate::cluster::renders::empty(), ME);
    assert!(none.remote_vault().is_none());
    assert!(none.vault_node().is_none());
    assert!(ClusterModel::default().remote_vault().is_none());
}

type Calls = Arc<Mutex<Vec<(String, Value)>>>;

struct Relay {
    calls: Calls,
    fail: Option<&'static str>,
}

impl Transport for Relay {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        self.calls
            .lock()
            .expect("calls")
            .push((op.to_owned(), args.clone()));
        if let Some(message) = self.fail {
            return Err(ClientError::Api(ApiError::new(ErrorCode::Failed, message)));
        }
        Ok(match op {
            "vault.list" => json!({"entries": [
                VaultEntry::new("npm", Kind::Token),
                VaultEntry::new("github", Kind::Token)
            ]}),
            "vault.save" => json!({"entry": args["entry"], "answered": false}),
            _ => json!({"removed": true}),
        })
    }
}

fn relay(fail: Option<&'static str>) -> (Arc<Vault>, Calls) {
    let calls: Calls = Arc::default();
    let transport: Arc<dyn Transport> = Arc::new(Relay {
        calls: calls.clone(),
        fail,
    });
    (Arc::new(Vault::remote_over(transport)), calls)
}

#[test]
fn remote_vault_names_the_node_in_every_call_and_ignores_this_macs_switch() {
    let (vault, calls) = relay(None);
    let switches = Switches::all_off();
    assert!(vault.is_remote());
    assert_eq!(
        vault.save(
            &switches,
            &VaultEntry::new("x", Kind::Token),
            None,
            None,
            None
        ),
        Err("This Mac doesn't know which machine holds the vault.".to_owned()),
        "no node, no call"
    );
    assert!(calls.lock().expect("calls").is_empty());

    vault.set_node(Some(STANDBY));
    vault.refresh(&switches);
    let entry = VaultEntry::new("github", Kind::Token);
    let value = VaultValue {
        username: String::new(),
        secret: MARKER.to_owned(),
    };
    vault
        .save(&switches, &entry, Some(&value), None, None)
        .expect("saved");
    vault.remove(&switches, "npm").expect("removed");
    let seen = calls.lock().expect("calls");
    let sent: Vec<&str> = seen.iter().map(|c| c.0.as_str()).collect();
    assert_eq!(sent, ["vault.list", "vault.save", "vault.remove"]);
    assert!(seen.iter().all(|(_, args)| args["node"] == STANDBY));
    assert_eq!(seen[1].1["value"]["secret"], MARKER);
    drop(seen);
    let shown = format!("{:?}", vault.snapshot());
    assert!(!shown.contains(MARKER), "{shown}");
}

#[test]
fn a_local_vault_never_adds_a_node() {
    let calls: Calls = Arc::default();
    let vault = Vault::over(Box::new(Relay {
        calls: calls.clone(),
        fail: None,
    }));
    vault.refresh(&Switches::all_on());
    assert!(calls.lock().expect("calls")[0].1.get("node").is_none());
    assert!(!vault.is_remote());
}

#[test]
fn the_names_come_from_the_vault_node() {
    let local_calls: Calls = Arc::default();
    let local = Vault::over(Box::new(Relay {
        calls: local_calls.clone(),
        fail: None,
    }));
    let (remote, remote_calls) = relay(None);

    let here = model_of(&status());
    let names = read_names(&here, &local, &remote, &Switches::all_on()).expect("names");
    assert_eq!(names, ["github", "npm"]);
    assert!(remote_calls.lock().expect("calls").is_empty());
    assert!(local_calls.lock().expect("calls")[0]
        .1
        .get("node")
        .is_none());

    let mut elsewhere = status();
    elsewhere["cluster"]["vault"] = json!(STANDBY);
    let there = ClusterModel::from_status(&elsewhere, WORKER);
    let names = read_names(&there, &local, &remote, &Switches::all_off()).expect("names");
    assert_eq!(names, ["github", "npm"]);
    assert_eq!(remote_calls.lock().expect("calls")[0].1["node"], STANDBY);
}

#[test]
fn an_offline_vault_says_where_it_is_and_asks_nothing() {
    let (local, _) = relay(None);
    let (remote, remote_calls) = relay(None);
    let mut moved = status();
    moved["cluster"]["vault"] = json!(WORKER);
    let model = model_of(&moved);
    let error = read_names(&model, &local, &remote, &Switches::all_on()).unwrap_err();
    assert_eq!(error, "The vault is on build-box, which is offline");
    assert!(remote_calls.lock().expect("calls").is_empty());
    let none = ClusterModel::default();
    assert!(read_names(&none, &local, &remote, &Switches::all_on()).is_err());
}

#[test]
fn this_macs_vault_switch_still_gates_its_own_vault() {
    let (local, local_calls) = relay(None);
    let (remote, _) = relay(None);
    let error = read_names(&model_of(&status()), &local, &remote, &Switches::all_off());
    assert!(error.is_err());
    assert!(local_calls.lock().expect("calls").is_empty());
}

#[test]
fn a_relay_refusal_is_shown_as_it_came() {
    let (local, _) = relay(None);
    let (remote, _) = relay(Some("The vault is on mac-mini, which is offline"));
    let mut elsewhere = status();
    elsewhere["cluster"]["vault"] = json!(STANDBY);
    let model = ClusterModel::from_status(&elsewhere, WORKER);
    let error = read_names(&model, &local, &remote, &Switches::all_on()).unwrap_err();
    assert_eq!(error, "The vault is on mac-mini, which is offline");
}

fn local_and_remote() -> (Arc<Vault>, Calls, Arc<Vault>, Calls) {
    let local_calls: Calls = Arc::default();
    let local = Arc::new(Vault::over(Box::new(Relay {
        calls: local_calls.clone(),
        fail: None,
    })));
    let (remote, remote_calls) = relay(None);
    (local, local_calls, remote, remote_calls)
}

fn remote_model() -> ClusterModel {
    let mut elsewhere = status();
    elsewhere["cluster"]["vault"] = json!(STANDBY);
    model_of(&elsewhere)
}

#[test]
fn the_target_is_local_remote_or_unreachable() {
    assert_eq!(target(false, &remote_model()), Target::Local);
    assert_eq!(
        target(true, &remote_model()),
        Target::Remote(STANDBY.to_owned())
    );
    assert_eq!(target(true, &ClusterModel::default()), Target::Unreachable);
    assert_eq!(
        target(true, &model_of(&status())),
        Target::Unreachable,
        "this Mac holds the vault: there is no other one to write to"
    );
}

#[test]
fn feed_lost_means_no_local_save_or_remove() {
    let (local, local_calls, remote, remote_calls) = local_and_remote();
    let pinned = Target::Remote(STANDBY.to_owned());
    let lost = ClusterModel::default();
    assert!(store_for(Some(&pinned), true, &lost, &local, &remote).is_err());
    assert!(store_for(None, true, &lost, &local, &remote).is_err());
    let mut other = status();
    other["cluster"]["vault"] = json!(WORKER);
    let moved = model_of(&other);
    assert!(store_for(Some(&pinned), true, &moved, &local, &remote).is_err());
    assert!(local_calls.lock().expect("calls").is_empty());
    assert!(remote_calls.lock().expect("calls").is_empty());
}

#[test]
fn a_pinned_target_that_still_holds_gets_its_own_store() {
    let (local, _, remote, _) = local_and_remote();
    let here = model_of(&status());
    let got = store_for(Some(&Target::Local), false, &here, &local, &remote).expect("local");
    assert!(Arc::ptr_eq(&got, &local));
    let pinned = Target::Remote(STANDBY.to_owned());
    let got = store_for(Some(&pinned), true, &remote_model(), &local, &remote).expect("remote");
    assert!(Arc::ptr_eq(&got, &remote));
    let switched = store_for(Some(&Target::Local), true, &remote_model(), &local, &remote);
    assert!(
        switched.is_err(),
        "a form opened on This Mac never goes remote"
    );
}

#[test]
fn a_lost_daemon_is_an_error_not_an_empty_vault() {
    struct Gone;
    impl Transport for Gone {
        fn call(&self, _: &str, _: Value) -> Result<Value, ClientError> {
            Err(ClientError::Closed)
        }
    }
    let (local, _, _, _) = local_and_remote();
    let remote = Vault::remote_over(Arc::new(Gone));
    let error = read_names(&remote_model(), &local, &remote, &Switches::all_on());
    assert_eq!(error, Err(crate::acting::DETACHED_MESSAGE.to_owned()));
}

#[test]
fn save_says_why_it_cannot_go() {
    let model = model_of(&status());
    let edit = SecretsModel::new(&names(), &SecretAllow::default());
    assert!(save_request(&model, WORKER, &edit, true).is_ok());
    assert_eq!(
        save_request(&model, WORKER, &edit, false),
        Err(crate::acting::DETACHED_MESSAGE.to_owned())
    );
    assert_eq!(
        save_request(&model, ME, &edit, true),
        Err("This machine can't take secrets any more, so nothing was saved.".to_owned())
    );
    let mut gone = status();
    gone["members"][0]["state"] = json!("removed");
    assert!(save_request(&model_of(&gone), WORKER, &edit, true).is_err());
    assert!(save_request(&ClusterModel::default(), WORKER, &edit, true).is_err());
}

#[test]
fn the_caps_are_visible() {
    let many: Vec<String> = (0..MAX_ALLOWED_NAMES + 1)
        .map(|n| format!("s{n}"))
        .collect();
    let mut edit = SecretsModel::new(&many, &SecretAllow::default());
    assert!(!edit.at_cap());
    for name in &many {
        edit.toggle(name);
    }
    assert!(edit.at_cap());
    let some: Vec<String> = (0..17).map(|n| format!("s{n}")).collect();
    let mut picker = SecretsPicker::new(&some, &SecretAllow::new(Vec::new(), true), None);
    assert!(!picker.at_cap());
    for name in &some {
        picker.toggle(name);
    }
    assert!(picker.at_cap());
}

#[test]
fn a_new_vault_node_never_shows_the_old_nodes_list() {
    let (remote, _) = relay(None);
    let on = Switches::all_on();
    remote.set_node(Some(STANDBY));
    remote.refresh(&on);
    assert_eq!(remote.snapshot().entries.len(), 2);
    remote.set_node(Some(WORKER));
    assert!(remote.snapshot().entries.is_empty());
    remote.refresh(&on);
    remote.set_node(None);
    assert!(remote.snapshot().entries.is_empty());
    let fixed = Vault::remote_fixed(vec![VaultEntry::new("kept", Kind::Token)]);
    fixed.set_node(Some(STANDBY));
    assert_eq!(
        fixed.snapshot().entries.len(),
        1,
        "a render keeps its entries"
    );
}
