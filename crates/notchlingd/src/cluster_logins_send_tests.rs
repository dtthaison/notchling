use super::*;
use crate::cluster_testkit::Dir;
use crate::grant_use::tests::{cluster, id};
use crate::vault_ops::{Looked, Released, Still};
use notchling_engine::cluster::grants::{encode_env, SecretAllow};
use notchling_engine::cluster::Versions;
use notchling_engine::vault::Kind as VaultKind;
use notchling_engine::versions::Version;
use notchling_local_api::ErrorCode;
use std::sync::atomic::AtomicI64;

const MARKER: &str = "NL-MARKER-820";
const TOKEN_VAR: &str = "CLAUDE_CODE_OAUTH_TOKEN";
const DAY: &str = "2026-12-31";

fn noon(day: &str) -> i64 {
    day_number(day).expect("a day") * DAY_MS + DAY_MS / 2
}

type Hook = Box<dyn FnOnce(&Fake) + Send>;

#[derive(Default)]
struct Inner {
    log: Vec<String>,
    sends: Vec<(NodeId, String, String)>,
    releases: usize,
    release_err: Option<ApiError>,
    audit_down: bool,
    link_down: bool,
    entry: Option<VaultEntry>,
    state: Option<ClusterState>,
    me: Option<NodeId>,
    online: BTreeSet<NodeId>,
    raised: Vec<Item>,
    withdrawn: Vec<String>,
    during_release: Option<Hook>,
}

#[derive(Default)]
struct Fake {
    inner: Mutex<Inner>,
}

impl Fake {
    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        f(&mut self.inner.lock().expect("lock"))
    }

    fn log(&self) -> Vec<String> {
        self.with(|i| i.log.clone())
    }

    fn position(&self, entry: &str) -> usize {
        self.log()
            .iter()
            .position(|l| l == entry)
            .unwrap_or(usize::MAX)
    }

    fn edit_entry(&self, f: impl FnOnce(&mut VaultEntry)) {
        self.with(|i| f(i.entry.as_mut().expect("an entry")));
    }

    fn edit_state(&self, f: impl FnOnce(&mut ClusterState)) {
        self.with(|i| f(i.state.as_mut().expect("a state")));
    }

    fn sends(&self) -> Vec<(NodeId, String, String)> {
        self.with(|i| i.sends.clone())
    }
}

fn entry(expires: Option<&str>, updated: &str) -> VaultEntry {
    VaultEntry {
        ask: Some(false),
        expires: expires.map(str::to_string),
        how_to: None,
        kind: VaultKind::Token,
        name: LOGIN_ENTRY.to_string(),
        note: String::new(),
        secret_var: TOKEN_VAR.to_string(),
        updated: updated.to_string(),
        url: None,
        user_var: String::new(),
    }
}

impl Vault for Fake {
    fn lookup(&self, names: &[String]) -> Result<Vec<Looked>, ApiError> {
        Ok(names
            .iter()
            .map(|_| Looked {
                exists: true,
                asks: false,
            })
            .collect())
    }

    fn release(
        &self,
        _who: &ReleaseFor,
        _names: &[String],
        still: &Still<'_>,
    ) -> Result<Released, ApiError> {
        let hook = self.with(|i| {
            i.releases += 1;
            i.log.push("release".to_string());
            i.during_release.take()
        });
        if let Some(hook) = hook {
            hook(self);
        }
        if let Some(e) = self.with(|i| i.release_err.clone()) {
            return Err(e);
        }
        still().map_err(|why| ApiError::new(ErrorCode::Forbidden, why))?;
        let held = self.with(|i| i.entry.clone()).expect("an entry");
        let mut env = BTreeMap::new();
        env.insert(TOKEN_VAR.to_string(), MARKER.to_string());
        Ok(Released {
            values: vec![(held, encode_env(&env).expect("env"))],
            asked: false,
            by: None,
            device: None,
            waited_ms: None,
        })
    }
}

impl Entries for Fake {
    fn entry(&self, _name: &str) -> Result<Option<VaultEntry>, ApiError> {
        Ok(self.with(|i| i.entry.clone()))
    }
}

impl Link for Fake {
    fn call(&self, node: &NodeId, op: &str, args: Value) -> Result<Value, ApiError> {
        self.with(|i| {
            i.log.push(format!("send:{op}"));
            i.sends
                .push((node.clone(), op.to_string(), args.to_string()));
            if i.link_down {
                return Err(ApiError::new(ErrorCode::Failed, "the node is offline"));
            }
            Ok(json!({"stored": "claude"}))
        })
    }
}

impl Audit for Fake {
    fn released(&self, release: &Release<'_>) -> Result<(), String> {
        self.with(|i| {
            if i.audit_down {
                return Err("down".to_string());
            }
            i.log.push(format!("audit:{}", release.name));
            Ok(())
        })
    }

    fn refused(&self, _node: &NodeId, _who: &ReleaseFor, name: &str) {
        self.with(|i| i.log.push(format!("refused:{name}")));
    }
}

impl View for Fake {
    fn state(&self) -> Option<ClusterState> {
        self.with(|i| i.state.clone())
    }

    fn me(&self) -> Option<NodeId> {
        self.with(|i| i.me.clone())
    }
}

impl Online for Fake {
    fn online(&self, node: &NodeId) -> bool {
        self.with(|i| i.online.contains(node))
    }
}

impl Notes for Fake {
    fn raise(&self, item: Item) {
        self.with(|i| i.raised.push(item));
    }

    fn withdraw(&self, id: &str) {
        self.with(|i| i.withdrawn.push(id.to_string()));
    }
}

fn versions(with_claude: bool, logged_in: bool) -> Versions {
    let mut tools = BTreeMap::new();
    if with_claude {
        tools.insert(Tool::Claude, Version::parse("2.1.324").expect("version"));
    }
    let mut logins = BTreeMap::new();
    if logged_in {
        logins.insert(Tool::Claude, true);
    }
    Versions {
        notchling: "1".to_string(),
        proto: 1,
        features: Vec::new(),
        schema: 1,
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
        tools,
        logins,
    }
}

struct Rig {
    fake: Arc<Fake>,
    deliverer: Arc<LoginDeliverer>,
    clock: Arc<AtomicI64>,
}

fn state_with_workers() -> ClusterState {
    let mut state = cluster("vault", &[]);
    for name in ["worker", "other"] {
        state.members.get_mut(&id(name)).expect("member").versions = Some(versions(true, false));
    }
    state
}

fn rig_with(stamps: Stamps) -> Rig {
    let fake = Arc::new(Fake::default());
    fake.with(|i| {
        i.entry = Some(entry(Some(DAY), "t1"));
        i.state = Some(state_with_workers());
        i.me = Some(id("vault"));
        i.online = [id("worker"), id("other")].into_iter().collect();
    });
    let clock = Arc::new(AtomicI64::new(noon("2026-11-15")));
    let read = clock.clone();
    let deliverer = LoginDeliverer::new(
        Parts {
            vault: fake.clone(),
            entries: fake.clone(),
            link: fake.clone(),
            audit: fake.clone(),
            view: fake.clone(),
            online: fake.clone(),
            notes: fake.clone(),
        },
        stamps,
        Arc::new(move || read.load(Ordering::SeqCst)),
    );
    Rig {
        fake,
        deliverer,
        clock,
    }
}

fn rig() -> Rig {
    rig_with(Stamps::in_memory())
}

impl Rig {
    fn pass(&self, ms: i64) {
        self.clock.fetch_add(ms, Ordering::SeqCst);
    }

    fn only_worker(&self) {
        self.fake.edit_state(|s| {
            s.members.remove(&id("other"));
        });
    }
}

#[test]
fn an_online_worker_gets_the_login_after_the_audit_with_the_entrys_expiry() {
    let rig = rig();
    rig.only_worker();
    rig.deliverer.reconcile();
    let sends = rig.fake.sends();
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].0, id("worker"));
    assert_eq!(sends[0].1, "login.deliver");
    let args: Value = serde_json::from_str(&sends[0].2).expect("json");
    assert_eq!(args["tool"], "claude");
    assert_eq!(args["value"], MARKER);
    assert_eq!(args["expiresMs"], day_number(DAY).unwrap() * DAY_MS);
    assert!(rig.fake.position("audit:claude-worker") < rig.fake.position("send:login.deliver"));
    assert!(rig.fake.position("release") < rig.fake.position("audit:claude-worker"));
}

#[test]
fn the_value_is_only_in_the_deliver_call() {
    let rig = rig();
    rig.only_worker();
    rig.deliverer.reconcile();
    let log = rig.fake.log().join("\n");
    assert!(!log.contains("NL-MARKER"), "a log line held the value");
    let items = rig.fake.with(|i| format!("{:?}", i.raised));
    assert!(!items.contains("NL-MARKER"), "an inbox item held the value");
    let sends = rig.fake.sends();
    assert_eq!(
        sends.iter().filter(|s| s.2.contains("NL-MARKER")).count(),
        1
    );
}

#[test]
fn a_delivered_login_is_not_sent_again_until_the_entry_changes() {
    let rig = rig();
    rig.only_worker();
    rig.deliverer.reconcile();
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.sends().len(), 1);
    rig.fake.edit_entry(|e| e.updated = "t2".to_string());
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.sends().len(), 2);
    rig.fake
        .edit_entry(|e| e.expires = Some("2027-01-31".to_string()));
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.sends().len(), 3);
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.sends().len(), 3);
}

#[test]
fn a_worker_that_lost_its_login_gets_it_again_but_one_that_holds_it_does_not() {
    let rig = rig();
    rig.only_worker();
    rig.deliverer.reconcile();
    rig.pass(RESEND_MS + 1);
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.sends().len(), 2);
    rig.fake.edit_state(|s| {
        s.members.get_mut(&id("worker")).unwrap().versions = Some(versions(true, true));
    });
    rig.pass(RESEND_MS + 1);
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.sends().len(), 2);
}

#[test]
fn only_the_vault_node_while_home_delivers() {
    let rig = rig();
    rig.fake.edit_state(|s| s.home = id("other"));
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.releases), 0);
    rig.fake.edit_state(|s| s.home = id("vault"));
    rig.fake.with(|i| i.me = Some(id("worker")));
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.releases), 0);
    rig.fake.with(|i| i.state = None);
    rig.deliverer.reconcile();
    assert!(rig.fake.sends().is_empty());
}

#[test]
fn an_offline_node_costs_no_release_and_no_audit() {
    let rig = rig();
    rig.fake.with(|i| i.online.clear());
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.releases), 0);
    assert!(rig.fake.log().is_empty());
}

#[test]
fn a_node_the_allowlist_a_removal_or_a_missing_tool_excludes_gets_nothing() {
    let rig = rig();
    rig.fake.edit_state(|s| {
        s.members.get_mut(&id("worker")).unwrap().secrets =
            SecretAllow::new(["other".to_string()], false);
        s.members.get_mut(&id("other")).unwrap().versions = Some(versions(false, false));
    });
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.releases), 0);
    rig.fake.edit_state(|s| {
        s.members.get_mut(&id("worker")).unwrap().secrets = SecretAllow::new([], true);
        s.members.get_mut(&id("worker")).unwrap().state = MemberState::Removed;
    });
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.releases), 0);
}

#[test]
fn the_vault_node_itself_is_not_sent_its_own_login() {
    let rig = rig();
    rig.fake.edit_state(|s| {
        s.members.get_mut(&id("vault")).unwrap().versions = Some(versions(true, false));
    });
    rig.fake.with(|i| i.online.insert(id("vault")));
    rig.deliverer.reconcile();
    assert!(rig.fake.sends().iter().all(|s| s.0 != id("vault")));
}

#[test]
fn an_entry_with_no_expiry_is_never_sent_and_says_so() {
    let rig = rig();
    rig.fake.edit_entry(|e| e.expires = None);
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.releases), 0);
    let raised = rig.fake.with(|i| i.raised.clone());
    assert_eq!(raised.len(), 1);
    assert_eq!(raised[0].kind, Kind::Notice);
    assert!(raised[0].title.contains("no expiry"));
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.raised.len()), 1);
}

#[test]
fn an_entry_that_has_expired_is_never_sent() {
    let rig = rig();
    rig.pass(60 * DAY_MS);
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.releases), 0);
}

#[test]
fn a_refused_approval_waits_before_asking_again_and_a_changed_entry_asks_at_once() {
    let rig = rig();
    rig.only_worker();
    rig.fake.with(|i| {
        i.release_err = Some(ApiError::new(
            ErrorCode::NeedsApproval,
            "The user didn't allow it.",
        ))
    });
    rig.deliverer.reconcile();
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.releases), 1);
    rig.pass(HOLD_MS + 1);
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.releases), 2);
    rig.fake.edit_entry(|e| e.updated = "t9".to_string());
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.releases), 3);
    assert!(rig.fake.sends().is_empty());
}

#[test]
fn a_failed_send_leaves_no_stamp_and_is_tried_again_later() {
    let rig = rig();
    rig.only_worker();
    rig.fake.with(|i| i.link_down = true);
    rig.deliverer.reconcile();
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.sends().len(), 1);
    rig.fake.with(|i| i.link_down = false);
    rig.pass(HOLD_MS + 1);
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.sends().len(), 2);
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.sends().len(), 2);
}

#[test]
fn a_log_that_cannot_be_written_stops_the_send() {
    let rig = rig();
    rig.only_worker();
    rig.fake.with(|i| i.audit_down = true);
    rig.deliverer.reconcile();
    assert!(rig.fake.sends().is_empty());
}

#[test]
fn a_change_while_the_vault_waits_for_approval_stops_the_send() {
    let rig = rig();
    rig.only_worker();
    rig.fake.with(|i| {
        i.during_release = Some(Box::new(|f: &Fake| f.edit_state(|s| s.home = id("other"))))
    });
    rig.deliverer.reconcile();
    assert!(rig.fake.sends().is_empty());
    assert!(!rig.fake.log().iter().any(|l| l.starts_with("audit:")));
}

#[test]
fn a_machine_removed_while_the_vault_waits_gets_nothing() {
    let rig = rig();
    rig.only_worker();
    rig.fake.with(|i| {
        i.during_release = Some(Box::new(|f: &Fake| {
            f.edit_state(|s| s.members.get_mut(&id("worker")).unwrap().state = MemberState::Removed)
        }))
    });
    rig.deliverer.reconcile();
    assert!(rig.fake.sends().is_empty());
}

#[test]
fn an_entry_without_a_usable_token_is_refused_and_audited_as_refused() {
    let rig = rig();
    rig.only_worker();
    rig.fake
        .edit_entry(|e| e.secret_var = "OTHER_VAR".to_string());
    rig.deliverer.reconcile();
    assert!(rig.fake.sends().is_empty());
    assert!(rig
        .fake
        .log()
        .contains(&"refused:claude-worker".to_string()));
    assert!(!rig.fake.log().iter().any(|l| l.starts_with("audit:")));
}

#[test]
fn fourteen_days_before_the_expiry_each_worker_gets_a_login_needed_notice() {
    let rig = rig();
    rig.only_worker();
    rig.deliverer.reconcile();
    assert!(rig.fake.with(|i| i.raised.is_empty()));
    rig.clock.store(noon("2026-12-17") + 1, Ordering::SeqCst);
    rig.deliverer.reconcile();
    rig.deliverer.reconcile();
    let raised = rig.fake.with(|i| i.raised.clone());
    assert_eq!(raised.len(), 1);
    let item = &raised[0];
    assert_eq!(item.kind, Kind::LoginNeeded);
    assert_eq!(
        item.title,
        format!("Claude Code's token for worker expires on {DAY}")
    );
    assert_eq!(item.source.node.as_deref(), Some(id("worker").as_str()));
    assert_eq!(item.actions.len(), 1);
    assert_eq!(item.actions[0].id, ActionId::Done);
    assert_eq!(item.state, State::Open);
    assert!(!format!("{item:?}").contains("NL-MARKER"));
}

#[test]
fn a_renewed_expiry_withdraws_the_old_notice_and_the_notice_is_not_early() {
    let rig = rig();
    rig.only_worker();
    rig.clock.store(noon("2026-12-18") + 1, Ordering::SeqCst);
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.raised.len()), 1);
    rig.fake.edit_entry(|e| {
        e.expires = Some("2027-06-30".to_string());
        e.updated = "t2".to_string();
    });
    rig.deliverer.reconcile();
    let withdrawn = rig.fake.with(|i| i.withdrawn.clone());
    assert_eq!(withdrawn.len(), 1);
    assert!(withdrawn[0].contains(DAY));
    assert_eq!(rig.fake.with(|i| i.raised.len()), 1);
}

#[test]
fn the_notice_is_raised_even_while_the_worker_is_offline() {
    let rig = rig();
    rig.only_worker();
    rig.fake.with(|i| i.online.clear());
    rig.clock.store(noon("2026-12-20"), Ordering::SeqCst);
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.raised.len()), 1);
    assert_eq!(rig.fake.with(|i| i.releases), 0);
}

#[test]
fn sent_stamps_survive_a_restart_so_nothing_is_released_twice() {
    let dir = Dir::new("login-stamps");
    let first = rig_with(Stamps::load(dir.path()));
    first.only_worker();
    first.deliverer.reconcile();
    assert_eq!(first.fake.sends().len(), 1);
    let text = std::fs::read_to_string(dir.path().join("login-sent.json")).expect("stamps");
    assert!(!text.contains("NL-MARKER"), "the stamps held the value");
    let second = rig_with(Stamps::load(dir.path()));
    second.only_worker();
    second.deliverer.reconcile();
    assert_eq!(second.fake.with(|i| i.releases), 0);
}

#[cfg(unix)]
#[test]
fn the_stamp_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Dir::new("login-stamp-mode");
    let rig = rig_with(Stamps::load(dir.path()));
    rig.only_worker();
    rig.deliverer.reconcile();
    let mode = std::fs::metadata(dir.path().join("login-sent.json"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn a_damaged_stamp_file_means_no_stamps() {
    let dir = Dir::new("login-stamp-bad");
    std::fs::write(dir.path().join("login-sent.json"), "{not json").unwrap();
    let stamps = Stamps::load(dir.path());
    assert!(stamps.get(&id("worker")).is_none());
}

#[test]
fn the_expiry_day_is_the_start_of_that_utc_day() {
    let at = expires_of(&entry(Some("2026-12-31"), "t")).expect("a day");
    assert_eq!(at, day_number("2026-12-31").unwrap() * DAY_MS);
    assert_eq!(at % DAY_MS, 0);
    assert!(expires_of(&entry(None, "t")).is_none());
    assert!(expires_of(&entry(Some("soon"), "t")).is_none());
    assert!(expires_of(&entry(Some("2026-02-30"), "t")).is_none());
}

#[test]
fn the_online_flag_reads_the_status() {
    let status = json!({"members": [
        {"id": id("worker").as_str(), "online": true},
        {"id": id("other").as_str(), "online": false},
    ]});
    assert!(flag(&status, &id("worker")));
    assert!(!flag(&status, &id("other")));
    assert!(!flag(&status, &id("vault")));
    assert!(!flag(&json!({}), &id("worker")));
}

#[test]
fn every_eligible_node_gets_the_login_op_and_nothing_else() {
    let rig = rig();
    rig.deliverer.reconcile();
    for send in rig.fake.sends() {
        assert_eq!(send.1, LOGIN_DELIVER);
    }
    let nodes: BTreeSet<NodeId> = rig.fake.sends().into_iter().map(|s| s.0).collect();
    assert_eq!(nodes, [id("worker"), id("other")].into_iter().collect());
}
