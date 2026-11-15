use super::*;
use notchling_engine::cluster::grant_files::GrantValue;
use notchling_engine::cluster::grants::{encode_env, SecretAllow};
use notchling_engine::cluster::wire::OriginTier;
use notchling_engine::cluster::{fingerprint, Desired, JobState, MemberState, Term};
use notchling_engine::vault::{Kind, VaultEntry};
use std::collections::BTreeMap;
use std::sync::Mutex;

const NOW: i64 = 1_000_000;

type Hook = Box<dyn FnOnce(&Fake) + Send>;

fn id(name: &str) -> NodeId {
    NodeId::from_fingerprint(&fingerprint(name.as_bytes())).expect("a node id")
}

fn member(name: &str) -> Member {
    Member {
        id: id(name),
        name: name.to_string(),
        fingerprint: fingerprint(name.as_bytes()),
        addrs: Vec::new(),
        port: 47823,
        client: false,
        can_be_master: false,
        reference: false,
        accepts_delegation: false,
        headless: false,
        state: MemberState::Active,
        versions: None,
        desired: Desired::default(),
        joined_ms: 0,
        secrets: SecretAllow::default(),
    }
}

#[derive(Default)]
struct Inner {
    log: Vec<String>,
    sends: Vec<(String, String)>,
    lookups: usize,
    releases: usize,
    seen: BTreeMap<String, (bool, bool)>,
    release_err: Option<ApiError>,
    audit_down: bool,
    fail_op: Option<&'static str>,
    fail_after: Option<(&'static str, usize)>,
    state: Option<ClusterState>,
    me: Option<NodeId>,
    waiting: Vec<String>,
    cleared: Vec<String>,
    value_reads: usize,
    during_approval: Option<Hook>,
    after_read: Option<Hook>,
    after_send: Option<Hook>,
}

#[derive(Default)]
pub(super) struct Fake {
    inner: Mutex<Inner>,
}

impl Fake {
    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        f(&mut self.inner.lock().expect("lock"))
    }

    fn expect(&self, name: &str, asks: bool) {
        self.with(|i| i.seen.insert(name.to_string(), (true, asks)));
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
}

fn entry(name: &str, asks: bool) -> VaultEntry {
    VaultEntry {
        ask: Some(asks),
        expires: None,
        how_to: None,
        kind: Kind::Token,
        name: name.to_string(),
        note: String::new(),
        secret_var: "TOKEN".to_string(),
        updated: String::new(),
        url: None,
        user_var: String::new(),
    }
}

impl Vault for Fake {
    fn lookup(&self, names: &[String]) -> Result<Vec<Looked>, ApiError> {
        self.with(|i| {
            i.lookups += 1;
            i.log.push("lookup".to_string());
            Ok(names
                .iter()
                .map(|n| {
                    let (exists, asks) = i.seen.get(n).copied().unwrap_or((false, false));
                    Looked { exists, asks }
                })
                .collect())
        })
    }

    fn release(
        &self,
        _who: &ReleaseFor,
        names: &[String],
        still: &Still<'_>,
    ) -> Result<Released, ApiError> {
        let hook = self.with(|i| {
            i.releases += 1;
            i.log.push("release".to_string());
            i.during_approval.take()
        });
        if let Some(hook) = hook {
            hook(self);
        }
        if let Some(e) = self.with(|i| i.release_err.clone()) {
            return Err(e);
        }
        still().map_err(|why| ApiError::new(ErrorCode::Forbidden, why))?;
        let released = self.with(|i| {
            i.value_reads += 1;
            let mut values = Vec::new();
            for (n, name) in names.iter().enumerate() {
                let mut env = BTreeMap::new();
                env.insert("TOKEN".to_string(), format!("NL-MARKER-{}", n + 1));
                let asks = i.seen.get(name).is_some_and(|s| s.1);
                values.push((entry(name, asks), encode_env(&env).expect("env")));
            }
            Released {
                values,
                asked: false,
                by: None,
                device: None,
                waited_ms: None,
            }
        });
        if let Some(hook) = self.with(|i| i.after_read.take()) {
            hook(self);
        }
        Ok(released)
    }
}

impl Link for Fake {
    fn call(&self, node: &NodeId, op: &str, args: Value) -> Result<Value, ApiError> {
        let sent = self.with(|i| {
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            i.log.push(format!("send:{op}:{name}"));
            let earlier = i.sends.iter().filter(|s| s.0.ends_with(op)).count();
            i.sends.push((format!("{node}:{op}"), args.to_string()));
            if i.fail_after == Some((op_name(op), earlier)) {
                return Err(ApiError::new(ErrorCode::Failed, "the node is offline"));
            }
            if i.fail_op == Some(op_name(op)) {
                return Err(ApiError::new(ErrorCode::Failed, "the node is offline"));
            }
            Ok(json!({}))
        });
        if let Some(hook) = self.with(|i| i.after_send.take()) {
            hook(self);
        }
        sent
    }
}

fn op_name(op: &str) -> &'static str {
    NODE_OPS.iter().copied().find(|n| *n == op).unwrap_or("")
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

impl Notices for Fake {
    fn waiting(&self, job: &Job, _node_name: &str, _vault_name: &str) {
        self.with(|i| i.waiting.push(job.id.clone()));
    }

    fn clear(&self, job: &str) {
        self.with(|i| i.cleared.push(job.to_string()));
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

fn broker(fake: &Arc<Fake>) -> GrantBroker {
    GrantBroker::new(
        (fake.clone(), fake.clone(), fake.clone()),
        fake.clone(),
        fake.clone(),
        Arc::new(|| NOW),
    )
}

fn worker(allow: &[&str]) -> Member {
    let mut m = member("worker");
    m.secrets = SecretAllow::new(allow.iter().map(|n| n.to_string()), false);
    m
}

fn job_on(member: &Member, secrets: &[&str], state: JobState) -> Job {
    Job {
        id: "j-1".to_string(),
        key: "k".to_string(),
        spec: notchling_engine::cluster::JobSpec {
            node: member.id.clone(),
            tool: Tool::Shell,
            cwd: "/work".to_string(),
            args: json!({"command": "make deploy"}),
            secrets: secrets.iter().map(|s| s.to_string()).collect(),
        },
        state,
        term: Term {
            n: 1,
            node: id("home"),
        },
        created_ms: 0,
        session: None,
        detail: None,
    }
}

fn facts(member: &Member, primary: bool) -> Facts {
    Facts {
        member: member.clone(),
        vault_name: "vault-mac".to_string(),
        vault_is_primary: primary,
    }
}

fn who() -> ReleaseFor {
    ReleaseFor {
        node_name: "worker".to_string(),
        job: "j-1".to_string(),
        command: "make deploy".to_string(),
        folder: "/work".to_string(),
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn code(r: Result<Grant, ApiError>) -> ErrorCode {
    r.expect_err("an error").code
}

#[test]
fn a_released_secret_is_audited_before_it_is_sent() {
    let fake = Arc::new(Fake::default());
    fake.expect("api-key", true);
    fake.expect("db-pass", false);
    let m = worker(&["api-key", "db-pass"]);
    let job = job_on(&m, &["api-key", "db-pass"], JobState::Running);
    live(&fake, &m, &job);
    let got = broker(&fake)
        .grant(
            &job,
            &facts(&m, true),
            &names(&["api-key", "db-pass"]),
            &who(),
        )
        .expect("granted");
    assert_eq!(got, Grant::Delivered(names(&["api-key", "db-pass"])));
    let log = fake.log();
    assert_eq!(&log[..2], ["lookup", "release"]);
    for name in ["api-key", "db-pass"] {
        let audit = fake.position(&format!("audit:{name}"));
        let send = fake.position(&format!("send:grant.deliver:{name}"));
        assert!(audit < send, "{name} was sent before its audit");
    }
    assert_eq!(fake.with(|i| i.releases), 1);
}

#[test]
fn a_value_travels_only_in_the_deliver_call() {
    let fake = Arc::new(Fake::default());
    fake.expect("api-key", false);
    let m = worker(&["api-key"]);
    let job = job_on(&m, &["api-key"], JobState::Running);
    live(&fake, &m, &job);
    let got = broker(&fake).grant(&job, &facts(&m, true), &names(&["api-key"]), &who());
    assert!(!format!("{got:?}").contains("NL-MARKER"));
    let sends = fake.with(|i| i.sends.clone());
    assert_eq!(sends.len(), 1);
    assert!(sends[0].0.ends_with(":grant.deliver"));
    assert!(sends[0].1.contains("NL-MARKER-1"));
    let log = fake.log().join("\n");
    assert!(!log.contains("NL-MARKER"));
    let args: Value = serde_json::from_str(&sends[0].1).expect("json");
    assert_eq!(args["job"], "j-1");
    assert_eq!(args["name"], "api-key");
    assert_eq!(args["expiresMs"], expires_ms(NOW));
}

#[test]
fn a_name_off_the_allowlist_is_refused_before_the_vault_is_read() {
    let fake = Arc::new(Fake::default());
    fake.expect("api-key", false);
    fake.expect("other", false);
    let m = worker(&["api-key"]);
    let job = job_on(&m, &["api-key"], JobState::Running);
    live(&fake, &m, &job);
    let r = broker(&fake).grant(
        &job,
        &facts(&m, true),
        &names(&["api-key", "other"]),
        &who(),
    );
    assert_eq!(code(r), ErrorCode::Forbidden);
    assert_eq!(
        fake.with(|i| (i.lookups, i.releases, i.sends.len())),
        (0, 0, 0)
    );
    assert_eq!(fake.log(), ["refused:other"]);
}

#[test]
fn nothing_is_read_or_sent_while_another_node_is_primary() {
    let fake = Arc::new(Fake::default());
    fake.expect("api-key", true);
    let m = worker(&["api-key"]);
    let job = job_on(&m, &["api-key"], JobState::Dispatching);
    live(&fake, &m, &job);
    let got = broker(&fake)
        .grant(&job, &facts(&m, false), &names(&["api-key"]), &who())
        .expect("waits");
    match got {
        Grant::Waiting(why) => assert!(why.contains("vault-mac")),
        other => panic!("expected waiting, got {other:?}"),
    }
    assert_eq!(
        fake.with(|i| (i.lookups, i.releases, i.sends.len())),
        (0, 0, 0)
    );
    assert!(fake.log().is_empty());
}

#[test]
fn a_secret_the_vault_lacks_is_refused_with_nothing_released() {
    let fake = Arc::new(Fake::default());
    let m = worker(&["ghost"]);
    let job = job_on(&m, &["ghost"], JobState::Running);
    live(&fake, &m, &job);
    let r = broker(&fake).grant(&job, &facts(&m, true), &names(&["ghost"]), &who());
    assert_eq!(code(r), ErrorCode::Forbidden);
    assert_eq!(
        fake.with(|i| (i.lookups, i.releases, i.sends.len())),
        (1, 0, 0)
    );
    assert!(fake.log().contains(&"refused:ghost".to_string()));
}

#[test]
fn a_denied_approval_sends_nothing_and_audits_no_release() {
    let fake = Arc::new(Fake::default());
    fake.expect("api-key", true);
    fake.with(|i| {
        i.release_err = Some(ApiError::new(
            ErrorCode::NeedsApproval,
            "The user didn't allow it.",
        ))
    });
    let m = worker(&["api-key"]);
    let job = job_on(&m, &["api-key"], JobState::Running);
    live(&fake, &m, &job);
    let r = broker(&fake).grant(&job, &facts(&m, true), &names(&["api-key"]), &who());
    assert_eq!(code(r), ErrorCode::NeedsApproval);
    assert!(fake.with(|i| i.sends.is_empty()));
    assert!(!fake.log().iter().any(|l| l.starts_with("audit:")));
}

#[test]
fn a_down_audit_log_stops_the_release_before_any_send() {
    let fake = Arc::new(Fake::default());
    fake.expect("api-key", false);
    fake.with(|i| i.audit_down = true);
    let m = worker(&["api-key"]);
    let job = job_on(&m, &["api-key"], JobState::Running);
    live(&fake, &m, &job);
    let r = broker(&fake).grant(&job, &facts(&m, true), &names(&["api-key"]), &who());
    let e = r.expect_err("refused");
    assert_eq!(e.message, LOG_DOWN);
    assert!(fake.with(|i| i.sends.is_empty()));
}

#[test]
fn a_failed_send_revokes_what_the_job_already_has() {
    let fake = Arc::new(Fake::default());
    fake.expect("api-key", false);
    fake.with(|i| i.fail_op = Some(GRANT_DELIVER));
    let m = worker(&["api-key"]);
    let job = job_on(&m, &["api-key"], JobState::Running);
    live(&fake, &m, &job);
    let r = broker(&fake).grant(&job, &facts(&m, true), &names(&["api-key"]), &who());
    assert_eq!(code(r), ErrorCode::Failed);
    let sends = fake.with(|i| i.sends.clone());
    assert_eq!(sends.len(), 2);
    assert!(sends[1].0.ends_with(":grant.revoke"));
    assert!(!sends[1].1.contains("NL-MARKER"));
    let revoked: Value = serde_json::from_str(&sends[1].1).unwrap();
    assert_eq!(revoked["names"], json!(["api-key"]));
}

#[test]
fn a_failed_second_send_revokes_the_names_of_this_call() {
    let fake = Arc::new(Fake::default());
    fake.expect("api-key", false);
    fake.expect("db-pass", false);
    fake.with(|i| i.fail_after = Some((GRANT_DELIVER, 1)));
    let m = worker(&["api-key", "db-pass"]);
    let job = job_on(&m, &["api-key", "db-pass"], JobState::Running);
    live(&fake, &m, &job);
    let r = broker(&fake).grant(
        &job,
        &facts(&m, true),
        &names(&["api-key", "db-pass"]),
        &who(),
    );
    assert_eq!(code(r), ErrorCode::Failed);
    let sends = fake.with(|i| i.sends.clone());
    assert_eq!(sends.len(), 3);
    let revoked: Value = serde_json::from_str(&sends[2].1).unwrap();
    assert_eq!(revoked["names"], json!(["api-key", "db-pass"]));
    assert_eq!(revoked["job"], "j-1");
}

#[test]
fn a_finished_job_gets_nothing() {
    let fake = Arc::new(Fake::default());
    fake.expect("api-key", false);
    let m = worker(&["api-key"]);
    let job = job_on(&m, &["api-key"], JobState::Done);
    live(&fake, &m, &job);
    let r = broker(&fake).grant(&job, &facts(&m, true), &names(&["api-key"]), &who());
    assert_eq!(code(r), ErrorCode::Forbidden);
    assert_eq!(
        fake.with(|i| (i.lookups, i.releases, i.sends.len())),
        (0, 0, 0)
    );
}

#[test]
fn names_are_lowercased_deduplicated_and_capped() {
    let fake = Arc::new(Fake::default());
    fake.expect("api-key", false);
    let m = worker(&["api-key"]);
    let job = job_on(&m, &["api-key"], JobState::Running);
    live(&fake, &m, &job);
    let got = broker(&fake)
        .grant(
            &job,
            &facts(&m, true),
            &names(&["API-KEY", "api-key"]),
            &who(),
        )
        .expect("granted");
    assert_eq!(got, Grant::Delivered(names(&["api-key"])));
    let many: Vec<String> = (0..=MAX_NAMES).map(|n| format!("s{n}")).collect();
    let fake = Arc::new(Fake::default());
    let r = broker(&fake).grant(&job, &facts(&m, true), &many, &who());
    assert_eq!(code(r), ErrorCode::BadRequest);
    let r = broker(&fake).grant(&job, &facts(&m, true), &[], &who());
    assert_eq!(code(r), ErrorCode::BadRequest);
    assert_eq!(fake.with(|i| i.lookups), 0);
}

fn cluster(home: &Member, vault: &Member, worker: &Member, job: Job) -> ClusterState {
    let mut members = BTreeMap::new();
    for m in [home, vault, worker] {
        members.insert(m.id.clone(), m.clone());
    }
    let mut jobs = BTreeMap::new();
    jobs.insert(job.id.clone(), job);
    ClusterState {
        id: "c".to_string(),
        name: "c".to_string(),
        term: Term {
            n: 1,
            node: home.id.clone(),
        },
        home: home.id.clone(),
        vault: vault.id.clone(),
        members,
        jobs,
        seq: 1,
    }
}

fn live(fake: &Fake, worker: &Member, job: &Job) {
    let vault = member("vault-mac");
    let state = cluster(&vault, &vault, worker, job.clone());
    fake.with(|i| {
        i.state = Some(state);
        i.me = Some(vault.id.clone());
    });
}

#[test]
fn the_node_ops_are_the_ones_this_module_answers() {
    assert_eq!(
        NODE_OPS,
        [
            "grant.deliver",
            "grant.revoke",
            "grant.request",
            "grant.attach",
            "grant.revoke_job"
        ]
    );
}

#[path = "grant_broker_recheck_tests.rs"]
mod recheck;
#[path = "grant_broker_request_tests.rs"]
mod requests;

#[test]
fn a_grant_value_prints_hidden() {
    let v = GrantValue::new("NL-MARKER-9".to_string());
    assert!(!format!("{v:?}{v}").contains("NL-MARKER"));
}
