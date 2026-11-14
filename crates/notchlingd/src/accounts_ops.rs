use crate::accounts_late::{self, ticket_arg, Tickets, REPLY_WAIT, RESULT_WAIT};
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
use notchling_engine::cost::account_log::{AccountSwitches, NoSwitches};
use notchling_engine::lanes::{start_command, LaneStore};
use notchling_events::{Emit, NoEmit};
use notchling_local_api::{ops, topics, ApiError, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub const ACCOUNT_OPS: [&str; 11] = [
    ops::ACCOUNTS_LIST,
    ops::ACCOUNTS_SWITCH,
    ops::ACCOUNTS_ADD_BEGIN,
    ops::ACCOUNTS_ADD_FINISH,
    ops::ACCOUNTS_REMOVE,
    ops::ACCOUNTS_MOVE_ALL,
    ops::ACCOUNTS_RESULT,
    ops::LANES_LIST,
    ops::LANES_CREATE,
    ops::LANES_REMOVE,
    ops::LANES_SWITCH,
];

pub const FINISH_WAIT: Duration = Duration::from_millis(4000);

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

pub type Random = Arc<dyn Fn() -> Option<[u8; 64]> + Send + Sync>;

#[derive(Clone)]
struct Ctx {
    flag: Arc<SwitchFlag>,
    store: Arc<ClaudeAccounts>,
    room: Arc<dyn Room>,
    acting: Arc<OnceLock<Arc<ActingOps>>>,
}

pub struct AccountsOps {
    ctx: Ctx,
    signins: Arc<SignIns>,
    random: Random,
    tickets: Arc<Tickets>,
    reply_wait_ms: AtomicU64,
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
        flag.subscribe(move |on| {
            if !on {
                off.cancel_all();
            }
        });
        AccountsOps {
            ctx: Ctx {
                flag,
                store,
                room,
                acting: Arc::new(OnceLock::new()),
            },
            signins,
            random,
            tickets: Arc::new(Tickets::default()),
            reply_wait_ms: AtomicU64::new(REPLY_WAIT.as_millis() as u64),
        }
    }

    pub fn for_daemon(layout: &Layout, flag: Arc<SwitchFlag>, room: Arc<dyn Room>) -> AccountsOps {
        AccountsOps::recording(layout, flag, room, Arc::new(NoEmit), Arc::new(NoSwitches))
    }

    pub fn recording(
        layout: &Layout,
        flag: Arc<SwitchFlag>,
        room: Arc<dyn Room>,
        events: Arc<dyn Emit>,
        history: Arc<dyn AccountSwitches>,
    ) -> AccountsOps {
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
                    inner: runner.clone(),
                }),
                user: user_name(),
            },
            undo_keys: Keys {
                runner,
                user: user_name(),
            },
            http: Arc::new(GatedHttp {
                flag: flag.clone(),
                inner: http,
            }),
            now_ms: Arc::new(crate::acting_ops::now_ms),
            config_file: home.join(".claude.json"),
            data_dir: layout.data_dir.clone(),
            list_file: layout.data_dir.join("daemon").join("accounts.json"),
            lanes: LaneStore::new(&home, &layout.data_dir),
            events,
            history,
        };
        AccountsOps::new(
            flag,
            Arc::new(ClaudeAccounts::new(env)),
            room,
            Arc::new(notchling_engine::accounts::claude::signin::random_seed),
        )
    }

    pub fn attach_acting(&self, acting: Arc<ActingOps>) {
        let _ = self.ctx.acting.set(acting);
    }

    pub fn store(&self) -> Arc<ClaudeAccounts> {
        self.ctx.store.clone()
    }

    pub fn set_reply_wait(&self, wait: Duration) {
        self.reply_wait_ms
            .store(wait.as_millis() as u64, Ordering::SeqCst);
    }

    fn slow(
        &self,
        op: &'static str,
        job: impl FnOnce(Ctx) -> Result<Value, ApiError> + Send + 'static,
    ) -> Result<Value, ApiError> {
        let ctx = self.ctx.clone();
        let scope = notchling_events::scope::current();
        let wait = Duration::from_millis(self.reply_wait_ms.load(Ordering::SeqCst));
        accounts_late::run(&self.tickets, &self.ctx.room, wait, op, move || {
            scope.run(|| job(ctx))
        })
    }

    fn add_begin(&self) -> Result<Value, ApiError> {
        let random =
            (self.random)().ok_or_else(|| ApiError::internal("no random bytes for the sign-in"))?;
        let room = self.ctx.room.clone();
        let store = self.ctx.store.clone();
        let flag = self.ctx.flag.clone();
        let added: Added = Arc::new(move || {
            let snap = store.snapshot();
            let accounts: Vec<Value> = snap.accounts.iter().map(account_value).collect();
            let data = json!({"on": flag.is_on(), "active": snap.active, "accounts": accounts});
            room.publish(topics::ACCOUNTS, topics::ACCOUNTS_CHANGED, &data);
        });
        self.signins.begin(
            self.ctx.store.clone(),
            self.ctx.flag.clone(),
            self.ctx.room.clone(),
            random,
            added,
        )
    }
}

impl Ctx {
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
            .cloned()
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
}

fn failed(e: AccountError) -> ApiError {
    ApiError::new(ErrorCode::Failed, e.0)
}

fn lane_json(lane: &notchling_engine::lanes::Lane) -> Value {
    let mut v = serde_json::to_value(lane).unwrap_or_default();
    v["startCommand"] = json!(start_command(lane));
    v
}

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
        let ctx = &self.ctx;
        let on = ctx.flag.is_on();
        match op {
            ops::ACCOUNTS_LIST => {
                let sync = args.get("sync").and_then(Value::as_bool).unwrap_or(false);
                if !(on && sync) {
                    return Ok(ctx.snapshot_json(&ctx.store.snapshot()));
                }
                let reply = self.slow(ops::ACCOUNTS_LIST, |ctx| {
                    let snap = ctx.store.sync().map_err(failed)?;
                    ctx.changed();
                    Ok(ctx.snapshot_json(&snap))
                })?;
                if reply.get("state").and_then(Value::as_str) == Some("working") {
                    let mut list = ctx.snapshot_json(&ctx.store.snapshot());
                    list["syncing"] = json!(true);
                    return Ok(list);
                }
                return Ok(reply);
            }
            ops::LANES_LIST => return Ok(ctx.lanes_json()),
            _ if !on => return Err(ctx.flag.refusal()),
            _ => {}
        }
        match op {
            ops::ACCOUNTS_SWITCH => {
                let id = text_arg(args, "id")?.to_string();
                self.slow(ops::ACCOUNTS_SWITCH, move |ctx| {
                    let snap = ctx.store.switch_to(&id).map_err(failed)?;
                    ctx.done(ctx.snapshot_json(&snap))
                })
            }
            ops::ACCOUNTS_REMOVE => {
                let id = text_arg(args, "id")?.to_string();
                self.slow(ops::ACCOUNTS_REMOVE, move |ctx| {
                    let snap = ctx.store.remove(&id).map_err(failed)?;
                    ctx.done(ctx.snapshot_json(&snap))
                })
            }
            ops::ACCOUNTS_ADD_BEGIN => self.add_begin(),
            ops::ACCOUNTS_ADD_FINISH => {
                let cancel = args.get("cancel").and_then(Value::as_bool).unwrap_or(false);
                self.signins
                    .finish(text_arg(args, "login")?, cancel, FINISH_WAIT)
            }
            ops::ACCOUNTS_MOVE_ALL => {
                let args = args.clone();
                self.slow(ops::ACCOUNTS_MOVE_ALL, move |ctx| ctx.move_all(&args))
            }
            ops::ACCOUNTS_RESULT => {
                let ticket = ticket_arg(args)?;
                Ok(self
                    .tickets
                    .result(ticket, RESULT_WAIT)?
                    .unwrap_or_else(|| accounts_late::working(ticket, ops::ACCOUNTS_RESULT)))
            }
            ops::LANES_CREATE => {
                let account = text_arg(args, "account")?.to_string();
                self.slow(ops::LANES_CREATE, move |ctx| {
                    let lane = ctx.store.open_lane(&account).map_err(failed)?;
                    ctx.done(json!({"lane": lane_json(&lane)}))
                })
            }
            ops::LANES_SWITCH => {
                let name = text_arg(args, "name")?.to_string();
                let account = text_arg(args, "account")?.to_string();
                self.slow(ops::LANES_SWITCH, move |ctx| {
                    let lane = ctx.store.switch_lane(&name, &account).map_err(failed)?;
                    ctx.done(json!({"lane": lane_json(&lane)}))
                })
            }
            ops::LANES_REMOVE => {
                let name = text_arg(args, "name")?.to_string();
                self.slow(ops::LANES_REMOVE, move |ctx| {
                    let removed = ctx.store.close_lane(&name).map_err(failed)?;
                    ctx.done(json!({"removed": removed}))
                })
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
