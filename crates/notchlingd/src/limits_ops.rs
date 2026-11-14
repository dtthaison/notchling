use crate::acting_ops::Room;
use crate::cost_ops;
use crate::inbox_ops::InboxOps;
use crate::layout::Layout;
use crate::limits_usage::{ClaudeUsage, LIMITS_USAGE};
use crate::op_group::OpGroup;
use crate::switches::SwitchFlag;
use notchling_engine::catalog::Catalog;
use notchling_engine::cost::CostService;
use notchling_engine::inbox::{Action, ActionId, Item, Kind, Source, State};
use notchling_engine::limits::claude::Announce;
use notchling_engine::limits::mode::{LimitConfig, LimitMode, CLAUDE, PROVIDERS};
use notchling_engine::limits::provider::Setup;
use notchling_engine::limits::sources::{ClaudeSource, ProviderSource, StoppedJobs};
use notchling_engine::limits::{ClaudeWatcher, Observed, ProviderLimitWatcher};
use notchling_local_api::{ApiError, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

pub const LIMITS_STATUS: &str = "limits.status";
pub const LIMITS_SET_MODE: &str = "limits.set_mode";
pub const COST_SUMMARY: &str = "cost.summary";
pub const TOPIC_LIMITS: &str = "limits";
pub const EVENT_NOTICE: &str = "limits.notice";

pub const TICK_EVERY: Duration = Duration::from_secs(10);
pub const SYNC_EVERY: f64 = 5.0 * 60.0;
const NOTICES: usize = 20;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

enum Slot {
    Claude {
        source: Arc<dyn ClaudeSource>,
        watcher: Box<ClaudeWatcher>,
    },
    Provider {
        source: Arc<dyn ProviderSource>,
        watcher: Box<ProviderLimitWatcher>,
    },
}

impl Slot {
    fn id(&self) -> &'static str {
        match self {
            Slot::Claude { .. } => CLAUDE.id,
            Slot::Provider { watcher, .. } => watcher.setup().provider.id,
        }
    }

    fn sync(&self) {
        match self {
            Slot::Claude { source, .. } => source.sync_usage(),
            Slot::Provider { source, .. } => source.sync_usage(),
        }
    }

    fn check(&mut self, now: f64) {
        match self {
            Slot::Claude { watcher, .. } => watcher.check(now),
            Slot::Provider { watcher, .. } => watcher.check(now),
        }
    }

    fn observed(&self) -> Observed {
        match self {
            Slot::Claude { watcher, .. } => watcher.observed(),
            Slot::Provider { watcher, .. } => watcher.observed(),
        }
    }
}

#[derive(Default)]
struct Watchers {
    slots: Vec<Slot>,
    last_sync: Option<f64>,
}

#[derive(Default)]
struct Board {
    registered: Vec<&'static str>,
    seen: HashMap<&'static str, Observed>,
}

pub struct LimitsOps {
    accounts: Arc<SwitchFlag>,
    resync: Arc<AtomicBool>,
    config: Arc<LimitConfig>,
    watchers: Mutex<Watchers>,
    board: Arc<Mutex<Board>>,
    notices: Arc<Mutex<VecDeque<(f64, String)>>>,
    room: Arc<dyn Room>,
    cost: Arc<CostService>,
    unpriced: cost_ops::UnpricedLog,
    claude_usage: ClaudeUsage,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    inbox: Option<Arc<InboxOps>>,
    raised: Arc<AtomicU64>,
    tick: Duration,
    stop: Mutex<Option<Sender<()>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn limit_item(id: String, text: &str, created_ms: i64) -> Item {
    Item {
        id,
        kind: Kind::LimitNotice,
        title: text.to_string(),
        detail: String::new(),
        source: Source::default(),
        created_ms,
        expires_ms: None,
        actions: vec![Action {
            id: ActionId::Done,
            label: "Done".to_string(),
            primary: false,
        }],
        state: State::Open,
        answer: None,
        answered_by: None,
        seen: false,
        open: None,
        closed_ms: None,
    }
}

fn system_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

impl LimitsOps {
    pub fn new(
        accounts: Arc<SwitchFlag>,
        config: Arc<LimitConfig>,
        cost: Arc<CostService>,
        room: Arc<dyn Room>,
    ) -> LimitsOps {
        let resync = Arc::new(AtomicBool::new(false));
        let board = Arc::new(Mutex::new(Board::default()));
        let (flag, seen) = (resync.clone(), Arc::downgrade(&board));
        accounts.subscribe(move |on| {
            if on {
                return;
            }
            flag.store(true, Ordering::SeqCst);
            if let Some(board) = seen.upgrade() {
                lock(&board).seen.clear();
            }
        });
        LimitsOps {
            accounts,
            resync,
            config,
            watchers: Mutex::new(Watchers::default()),
            board,
            notices: Arc::new(Mutex::new(VecDeque::new())),
            room,
            cost,
            unpriced: cost_ops::UnpricedLog::new(),
            claude_usage: ClaudeUsage::default(),
            now: Arc::new(system_now),
            inbox: None,
            raised: Arc::new(AtomicU64::new(0)),
            tick: TICK_EVERY,
            stop: Mutex::new(None),
            thread: Mutex::new(None),
        }
    }

    pub fn for_daemon(
        layout: &Layout,
        accounts: Arc<SwitchFlag>,
        room: Arc<dyn Room>,
        catalog: Arc<Catalog>,
        inbox: Arc<InboxOps>,
        history: Arc<dyn notchling_engine::cost::account_log::AccountSwitches>,
    ) -> LimitsOps {
        let config = if layout.socket_given {
            LimitConfig::in_memory()
        } else {
            LimitConfig::open(&layout.data_dir)
        };
        LimitsOps::new(
            accounts,
            Arc::new(config),
            cost_ops::CostSetup::from_layout(layout, catalog).service_with(history),
            room,
        )
        .with_inbox(inbox)
    }

    pub fn with_inbox(mut self, inbox: Arc<InboxOps>) -> LimitsOps {
        self.inbox = Some(inbox);
        self
    }

    pub fn with_clock(mut self, now: Arc<dyn Fn() -> f64 + Send + Sync>) -> LimitsOps {
        self.now = now;
        self
    }

    pub fn with_unpriced_sink(mut self, sink: Box<dyn Fn(&str) + Send + Sync>) -> LimitsOps {
        self.unpriced = cost_ops::UnpricedLog::with_sink(sink);
        self
    }

    pub fn with_tick(mut self, tick: Duration) -> LimitsOps {
        self.tick = tick;
        self
    }

    pub fn config(&self) -> &Arc<LimitConfig> {
        &self.config
    }

    pub fn cost(&self) -> &Arc<CostService> {
        &self.cost
    }

    pub(crate) fn announcer(&self) -> Announce {
        let notices = self.notices.clone();
        let room = self.room.clone();
        let now = self.now.clone();
        let inbox = self.inbox.clone();
        let raised = self.raised.clone();
        Arc::new(move |text: &str| {
            let at = now();
            let mut list = lock(&notices);
            list.push_back((at, text.to_string()));
            while list.len() > NOTICES {
                list.pop_front();
            }
            drop(list);
            room.publish(TOPIC_LIMITS, EVENT_NOTICE, &json!({"at": at, "text": text}));
            if let Some(inbox) = &inbox {
                let created_ms = (at * 1000.0) as i64;
                let seq = raised.fetch_add(1, Ordering::SeqCst);
                inbox.raise(
                    limit_item(format!("limit:{created_ms}:{seq}"), text, created_ms),
                    None,
                );
            }
        })
    }

    pub fn set_claude(&self, source: Arc<dyn ClaudeSource>, jobs: Arc<dyn StoppedJobs>) {
        self.claude_usage.set_source(source.clone());
        let watcher =
            ClaudeWatcher::new(source.clone(), jobs, self.config.clone(), self.announcer());
        self.put(Slot::Claude {
            source,
            watcher: Box::new(watcher),
        });
    }

    pub fn set_codex(
        &self,
        source: Arc<dyn ProviderSource>,
        idle: Box<dyn Fn() -> bool + Send + Sync>,
    ) {
        let config = self.config.clone();
        let watcher = ProviderLimitWatcher::new(
            source.clone(),
            Setup::codex(),
            self.config.clone(),
            Box::new(move || config.switch_at()),
            idle,
            self.announcer(),
        );
        self.put(Slot::Provider {
            source,
            watcher: Box::new(watcher),
        });
    }

    pub fn set_antigravity(
        &self,
        source: Arc<dyn ProviderSource>,
        idle: Box<dyn Fn() -> bool + Send + Sync>,
    ) {
        let config = self.config.clone();
        let watcher = ProviderLimitWatcher::new(
            source.clone(),
            Setup::antigravity(),
            self.config.clone(),
            Box::new(move || config.switch_at()),
            idle,
            self.announcer(),
        );
        self.put(Slot::Provider {
            source,
            watcher: Box::new(watcher),
        });
    }

    fn put(&self, slot: Slot) {
        let id = slot.id();
        let mut w = lock(&self.watchers);
        w.slots.retain(|s| s.id() != id);
        w.slots.push(slot);
        drop(w);
        let mut board = lock(&self.board);
        if !board.registered.contains(&id) {
            board.registered.push(id);
        }
        board.seen.remove(id);
    }

    pub fn tick_at(&self, now: f64) -> bool {
        if !self.accounts.is_on() {
            return false;
        }
        let mut w = lock(&self.watchers);
        if self.resync.swap(false, Ordering::SeqCst) {
            w.last_sync = None;
        }
        if w.slots.is_empty() {
            return true;
        }
        if w.last_sync.is_none_or(|t| now - t >= SYNC_EVERY) {
            for slot in &w.slots {
                slot.sync();
            }
            w.last_sync = Some(now);
        }
        for slot in &mut w.slots {
            if !self.accounts.is_on() {
                break;
            }
            slot.check(now);
            let seen = slot.observed();
            lock(&self.board).seen.insert(slot.id(), seen);
        }
        true
    }

    pub fn spawn_loop(self: &Arc<Self>) {
        let mut thread = lock(&self.thread);
        if thread.is_some() {
            return;
        }
        let (stop_tx, stop_rx) = channel::<()>();
        *lock(&self.stop) = Some(stop_tx);
        let ops = self.clone();
        *thread = std::thread::Builder::new()
            .name("notchling-limits".into())
            .spawn(move || {
                while let Err(RecvTimeoutError::Timeout) = stop_rx.recv_timeout(ops.tick) {
                    ops.tick_at((ops.now)());
                }
            })
            .ok();
    }

    pub fn close(&self) {
        let stop = lock(&self.stop).take();
        drop(stop);
        let thread = lock(&self.thread).take();
        if let Some(handle) = thread {
            let _ = handle.join();
        }
        self.cost.save((self.now)());
    }

    pub fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            LIMITS_STATUS | LIMITS_SET_MODE | LIMITS_USAGE => Some(OpSpec::APP),
            COST_SUMMARY => Some(OpSpec::AGENT),
            _ => None,
        }
    }

    pub fn call(&self, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            LIMITS_STATUS => Ok(self.status()),
            LIMITS_SET_MODE => self.set_mode(args),
            LIMITS_USAGE => self.claude_usage.call(self.accounts.is_on(), args),
            COST_SUMMARY => cost_ops::call(&self.cost, &self.unpriced, args, (self.now)()),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn status(&self) -> Value {
        let on = self.accounts.is_on();
        let board = lock(&self.board);
        let providers: Vec<Value> = PROVIDERS
            .iter()
            .map(|p| {
                let registered = board.registered.contains(&p.id);
                let seen = board.seen.get(p.id).cloned().unwrap_or_default();
                json!({
                    "id": p.id,
                    "label": p.label,
                    "mode": self.config.mode(p).as_str(),
                    "modes": p.modes.iter().map(|m| m.as_str()).collect::<Vec<_>>(),
                    "default": p.default.as_str(),
                    "registered": registered,
                    "active": seen.active_email,
                    "active_id": seen.active_id,
                    "windows": seen.windows.iter().map(|(name, percent, resets_at)| {
                        json!({"name": name, "percent": percent, "resets_at": resets_at})
                    }).collect::<Vec<_>>(),
                    "last_switch": seen.last_switch,
                    "checked": seen.checked,
                })
            })
            .collect();
        let notices: Vec<Value> = lock(&self.notices)
            .iter()
            .map(|(at, text)| json!({"at": at, "text": text}))
            .collect();
        json!({
            "accounts_on": on,
            "watching": on && !board.registered.is_empty(),
            "switch_at": self.config.switch_at(),
            "providers": providers,
            "notices": notices,
        })
    }

    fn set_mode(&self, args: &Value) -> Result<Value, ApiError> {
        let provider = args
            .get("provider")
            .and_then(Value::as_str)
            .and_then(|id| PROVIDERS.iter().find(|p| p.id == id))
            .ok_or_else(|| {
                ApiError::bad_request("provider is \"claude\", \"codex\" or \"antigravity\"")
            })?;
        let mode = match args.get("mode") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(LimitMode::parse(s).ok_or_else(|| {
                ApiError::bad_request("mode is \"off\", \"notify\" or \"notifyAndSwitch\"")
            })?),
            Some(_) => return Err(ApiError::bad_request("mode is a string")),
        };
        let switch_at = match args.get("switch_at") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                v.as_i64()
                    .ok_or_else(|| ApiError::bad_request("switch_at is a whole number"))?,
            ),
        };
        if mode.is_none() && switch_at.is_none() {
            return Err(ApiError::bad_request(
                "limits.set_mode takes a mode or a switch_at",
            ));
        }
        if let Some(m) = mode.filter(|m| !provider.modes.contains(m)) {
            return Err(ApiError::bad_request(format!(
                "{} can't be set to {}",
                provider.label,
                m.title()
            )));
        }
        if let Some(n) = switch_at {
            let (low, high) = notchling_engine::limits::logic::SWITCH_RANGE;
            if !(low..=high).contains(&n) {
                return Err(ApiError::bad_request(format!(
                    "switch_at is {low} to {high}"
                )));
            }
            self.config
                .set_switch_at(n)
                .map_err(|e| ApiError::new(ErrorCode::Failed, e))?;
        }
        if let Some(m) = mode {
            self.config
                .set_mode(provider, m)
                .map_err(|e| ApiError::new(ErrorCode::Failed, e))?;
        }
        Ok(json!({
            "provider": provider.id,
            "mode": self.config.mode(provider).as_str(),
            "switch_at": self.config.switch_at(),
        }))
    }
}

impl OpGroup for LimitsOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        LimitsOps::spec(self, op)
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        LimitsOps::call(self, op, args)
    }

    fn topics(&self) -> &[&'static str] {
        &[TOPIC_LIMITS]
    }

    fn topic_tier(&self, topic: &str) -> Option<Tier> {
        (topic == TOPIC_LIMITS).then_some(Tier::App)
    }
}
