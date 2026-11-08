//! Milestone 6's Claude Code accounts and lanes (E8a) as daemon ops, over
//! `notchling_engine::accounts::claude` and `notchling_engine::lanes`. Built and switched off:
//! while `accounts` is off every op but the two list reads answers `forbidden` with
//! [`ACCOUNTS_OFF`], and nothing reads the Keychain, a token or the network, or writes a file.
//! The two reads then use the daemon's own files only (the saved accounts list, `lanes.json`).
//! Besides the check at the top of [`AccountsOps::call`], the store's Keychain runner and HTTP
//! client are gated on the switch too ([`GatedSecurity`], [`GatedHttp`]), so a path that forgot
//! the check still reaches nothing.
//!
//! | Op | Tier | Args | Reply |
//! | --- | --- | --- | --- |
//! | `accounts.list` | app | `{sync?: bool}` | `{on, active, accounts: [{id, email, plan, needsLogin, lane}]}`; `sync` reads the live login first (switch on) |
//! | `accounts.switch` | app | `{id}` | as `accounts.list` |
//! | `accounts.add.begin` | app | `{}` | `{login, url}`: open `url` in the browser |
//! | `accounts.add.finish` | app | `{login, cancel?: bool}` | `{login, state: "waiting" \| "added" \| "failed" \| "cancelled", account?, error?}`, after up to 4 s |
//! | `accounts.remove` | app | `{id}` | as `accounts.list` |
//! | `accounts.move_all` | app | `{target: id \| null, dryRun?: bool}` | dry run `{plan: {jobs, closeLane, switchMain, signedOut}}`; else `{closedLane, switchedTo, moved, errors}` |
//! | `lanes.list` | app | `{}` | `{lanes: [{name, path, accountID, email, folder, startCommand}]}` |
//! | `lanes.create` | app | `{account}` | `{lane}` |
//! | `lanes.remove` | app | `{name}` | `{removed: bool}` |
//! | `lanes.switch` | app | `{name, account}` | `{lane}` |
//!
//! A refusal by the store (an account in a lane, no saved login, a Keychain error) is `failed` with
//! words for the user. After a change the list goes out on `accounts` as `accounts.changed`; a
//! sign-in's end as `accounts.add.ended`. Move All stops and resumes lane sessions too, so with any
//! to move it needs `acting` on as well.

use crate::accounts_signin::{Added, SignIns};
use crate::acting::ACTING_OFF;
use crate::acting_ops::{bad, forbidden, text_arg, ActingOps, Room};
use crate::layout::Layout;
use crate::op_group::OpGroup;
use crate::switches::{SwitchFlag, ACCOUNTS_OFF};
use notchling_engine::accounts::claude::{
    account_value, AccountError, ClaudeAccounts, ClaudeEnv, Snapshot,
};
use notchling_engine::accounts::http::{
    HttpClient, HttpRequest, HttpResponse, LiveHttp, NoNetwork,
};
use notchling_engine::accounts::keychain::{
    KeychainError, Keys, NoKeychain, SecurityOutput, SecurityRunner, SystemSecurity,
};
use notchling_engine::control::plan_move_all_jobs;
use notchling_engine::lanes::{start_command, LaneStore};
use notchling_local_api::{ops, topics, ApiError, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// Every op here.
pub const ACCOUNT_OPS: [&str; 10] = [
    ops::ACCOUNTS_LIST,
    ops::ACCOUNTS_SWITCH,
    ops::ACCOUNTS_ADD_BEGIN,
    ops::ACCOUNTS_ADD_FINISH,
    ops::ACCOUNTS_REMOVE,
    ops::ACCOUNTS_MOVE_ALL,
    ops::LANES_LIST,
    ops::LANES_CREATE,
    ops::LANES_REMOVE,
    ops::LANES_SWITCH,
];

/// How long `accounts.add.finish` waits, under a client's 5 s call timeout.
pub const FINISH_WAIT: Duration = Duration::from_millis(4000);

/// The Keychain only while `accounts` is on.
pub struct GatedSecurity {
    pub flag: Arc<SwitchFlag>,
    pub inner: Arc<dyn SecurityRunner>,
}

impl SecurityRunner for GatedSecurity {
    fn run(&self, args: &[String], stdin: Option<&[u8]>) -> Result<SecurityOutput, KeychainError> {
        if !self.flag.is_on() {
            return Err(KeychainError::Unsupported(ACCOUNTS_OFF.into()));
        }
        self.inner.run(args, stdin)
    }
}

/// The network only while `accounts` is on.
pub struct GatedHttp {
    pub flag: Arc<SwitchFlag>,
    pub inner: Arc<dyn HttpClient>,
}

impl HttpClient for GatedHttp {
    fn send(&self, request: &HttpRequest) -> HttpResponse {
        if !self.flag.is_on() {
            return HttpResponse {
                status: 0,
                body: Vec::new(),
            };
        }
        self.inner.send(request)
    }
}

/// 64 random bytes for a sign-in's verifier and state.
pub type Random = Arc<dyn Fn() -> Option<[u8; 64]> + Send + Sync>;

pub struct AccountsOps {
    flag: Arc<SwitchFlag>,
    store: Arc<ClaudeAccounts>,
    room: Arc<dyn Room>,
    acting: OnceLock<Arc<ActingOps>>,
    signins: Arc<SignIns>,
    random: Random,
}

impl AccountsOps {
    pub fn new(
        flag: Arc<SwitchFlag>,
        store: Arc<ClaudeAccounts>,
        room: Arc<dyn Room>,
        random: Random,
    ) -> AccountsOps {
        let signins = Arc::new(SignIns::default());
        let off = signins.clone();
        // A sign-in listening when the switch goes off stops at once.
        flag.subscribe(move |on| {
            if !on {
                off.cancel_all();
            }
        });
        AccountsOps {
            flag,
            store,
            room,
            acting: OnceLock::new(),
            signins,
            random,
        }
    }

    /// The daemon's: the real Keychain and network, both gated on `flag`. Under a fixture root
    /// neither is reached, whatever the switch says.
    pub fn for_daemon(layout: &Layout, flag: Arc<SwitchFlag>, room: Arc<dyn Room>) -> AccountsOps {
        let (runner, http): (Arc<dyn SecurityRunner>, Arc<dyn HttpClient>) = if layout.fixture {
            (
                Arc::new(NoKeychain("a fixture root reaches no Keychain")),
                Arc::new(NoNetwork),
            )
        } else {
            (Arc::new(SystemSecurity), Arc::new(LiveHttp))
        };
        let home = layout.home();
        let env = ClaudeEnv {
            keys: Keys {
                runner: Arc::new(GatedSecurity {
                    flag: flag.clone(),
                    inner: runner,
                }),
                user: user_name(),
            },
            http: Arc::new(GatedHttp {
                flag: flag.clone(),
                inner: http,
            }),
            now_ms: Arc::new(crate::acting_ops::now_ms),
            config_file: home.join(".claude.json"),
            list_file: layout.data_dir.join("daemon").join("accounts.json"),
            lanes: LaneStore::new(&home, &layout.data_dir),
        };
        AccountsOps::new(
            flag,
            Arc::new(ClaudeAccounts::new(env)),
            room,
            Arc::new(notchling_engine::accounts::claude::signin::random_seed),
        )
    }

    /// The session ops, for Move All's stops and resumes. Once.
    pub fn attach_acting(&self, acting: Arc<ActingOps>) {
        let _ = self.acting.set(acting);
    }

    /// The store (the limits watcher asks it for tokens).
    pub fn store(&self) -> Arc<ClaudeAccounts> {
        self.store.clone()
    }

    fn snapshot_json(&self, snap: &Snapshot) -> Value {
        let lanes = self.store.env().lanes.all();
        let accounts: Vec<Value> = snap
            .accounts
            .iter()
            .map(|a| {
                let mut v = account_value(a);
                v["lane"] = json!(lanes
                    .iter()
                    .find(|l| l.account_id == a.id)
                    .map(|l| l.name.clone()));
                v
            })
            .collect();
        json!({"on": self.flag.is_on(), "active": snap.active, "accounts": accounts})
    }

    fn lanes_json(&self) -> Value {
        let lanes: Vec<Value> = self.store.env().lanes.all().iter().map(lane_json).collect();
        json!({"lanes": lanes})
    }

    /// Publishes the list after a change (no Keychain: the list file and the last known login).
    fn changed(&self) {
        let mut data = self.snapshot_json(&self.store.snapshot());
        data["lanes"] = self.lanes_json()["lanes"].clone();
        self.room
            .publish(topics::ACCOUNTS, topics::ACCOUNTS_CHANGED, &data);
    }

    fn done(&self, value: Value) -> Result<Value, ApiError> {
        self.changed();
        Ok(value)
    }

    fn move_all(&self, args: &Value) -> Result<Value, ApiError> {
        let target = match args.get("target") {
            None | Some(Value::Null) => None,
            Some(Value::String(id)) => Some(id.as_str()),
            Some(_) => return Err(bad("target is an account id or null")),
        };
        let acting = self
            .acting
            .get()
            .ok_or_else(|| ApiError::internal("no session ops"))?;
        let plan = self
            .store
            .move_all_plan(&acting.agents_now(), target)
            .map_err(failed)?;
        if args.get("dryRun").and_then(Value::as_bool).unwrap_or(false) {
            let jobs: Vec<&str> = plan.jobs.iter().map(|j| j.agent_id.as_str()).collect();
            return Ok(json!({"plan": {"jobs": jobs, "closeLane": plan.close_lane,
                "switchMain": plan.switch_main, "signedOut": plan.signed_out}}));
        }
        if !plan.jobs.is_empty() && !acting.is_on() {
            return Err(forbidden(ACTING_OFF));
        }
        let mut errors = BTreeMap::new();
        let mut stopped = Vec::new();
        for job in plan_move_all_jobs(&plan, &acting.roots) {
            match acting.run_raw(&job.stop) {
                Ok(_) => stopped.push(job),
                Err(e) => {
                    errors.insert(job.agent_id.clone(), format!("Couldn't move it: {e}"));
                }
            }
        }
        let accounts = self.store.move_all_accounts(&plan);
        if !accounts.errors.is_empty() {
            errors.insert("accounts".into(), accounts.errors.join(" "));
        }
        // The sessions still resume, on the main login's current account, rather than stay stopped.
        let mut moved = Vec::new();
        for job in stopped {
            match acting.run_raw(&job.resume) {
                Ok(_) => moved.push(job.agent_id),
                Err(e) => {
                    errors.insert(
                        job.agent_id,
                        format!("Stopped, but couldn't resume it on the main login: {e}"),
                    );
                }
            }
        }
        self.done(
            json!({"closedLane": accounts.closed_lane, "switchedTo": accounts.switched_to,
            "moved": moved, "errors": errors}),
        )
    }

    fn add_begin(&self) -> Result<Value, ApiError> {
        let random =
            (self.random)().ok_or_else(|| ApiError::internal("no random bytes for the sign-in"))?;
        let room = self.room.clone();
        let store = self.store.clone();
        let flag = self.flag.clone();
        let added: Added = Arc::new(move || {
            let snap = store.snapshot();
            let accounts: Vec<Value> = snap.accounts.iter().map(account_value).collect();
            let data = json!({"on": flag.is_on(), "active": snap.active, "accounts": accounts});
            room.publish(topics::ACCOUNTS, topics::ACCOUNTS_CHANGED, &data);
        });
        self.signins.begin(
            self.store.clone(),
            self.flag.clone(),
            self.room.clone(),
            random,
            added,
        )
    }
}

fn failed(e: AccountError) -> ApiError {
    ApiError::new(ErrorCode::Failed, e.0)
}

fn lane_json(lane: &notchling_engine::lanes::Lane) -> Value {
    let mut v = serde_json::to_value(lane).unwrap_or_default();
    v["startCommand"] = json!(start_command(lane));
    v
}

/// The login name Keychain items are filed under (`NSUserName()`).
fn user_name() -> String {
    ["USER", "LOGNAME"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default()
}

impl OpGroup for AccountsOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        ACCOUNT_OPS.contains(&op).then_some(OpSpec::APP)
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        let on = self.flag.is_on();
        match op {
            ops::ACCOUNTS_LIST => {
                let sync = args.get("sync").and_then(Value::as_bool).unwrap_or(false);
                let snap = if on && sync {
                    self.store.sync().map_err(failed)?
                } else {
                    self.store.snapshot()
                };
                return Ok(self.snapshot_json(&snap));
            }
            ops::LANES_LIST => return Ok(self.lanes_json()),
            _ if !on => return Err(self.flag.refusal()),
            _ => {}
        }
        match op {
            ops::ACCOUNTS_SWITCH => {
                let snap = self
                    .store
                    .switch_to(text_arg(args, "id")?)
                    .map_err(failed)?;
                self.done(self.snapshot_json(&snap))
            }
            ops::ACCOUNTS_REMOVE => {
                let snap = self.store.remove(text_arg(args, "id")?).map_err(failed)?;
                self.done(self.snapshot_json(&snap))
            }
            ops::ACCOUNTS_ADD_BEGIN => self.add_begin(),
            ops::ACCOUNTS_ADD_FINISH => {
                let cancel = args.get("cancel").and_then(Value::as_bool).unwrap_or(false);
                self.signins
                    .finish(text_arg(args, "login")?, cancel, FINISH_WAIT)
            }
            ops::ACCOUNTS_MOVE_ALL => self.move_all(args),
            ops::LANES_CREATE => {
                let lane = self
                    .store
                    .open_lane(text_arg(args, "account")?)
                    .map_err(failed)?;
                self.done(json!({"lane": lane_json(&lane)}))
            }
            ops::LANES_SWITCH => {
                let lane = self
                    .store
                    .switch_lane(text_arg(args, "name")?, text_arg(args, "account")?)
                    .map_err(failed)?;
                self.done(json!({"lane": lane_json(&lane)}))
            }
            ops::LANES_REMOVE => {
                let removed = self
                    .store
                    .close_lane(text_arg(args, "name")?)
                    .map_err(failed)?;
                self.done(json!({"removed": removed}))
            }
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn topics(&self) -> &[&'static str] {
        &[topics::ACCOUNTS]
    }

    fn topic_tier(&self, topic: &str) -> Option<Tier> {
        (topic == topics::ACCOUNTS).then_some(Tier::App)
    }
}
