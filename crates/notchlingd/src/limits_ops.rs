//! E8c in the daemon (Milestone 6): the limit watchers and their ops. Built and switched off.
//!
//! While the `accounts` switch is off the watcher loop does nothing at all: it calls no source (so
//! no Keychain read, no usage request, no switch), runs no check and says nothing. The switch is
//! the daemon's [`SwitchFlag`], in memory only and off at every start; turning it off also drops
//! what the watchers last saw, so the first round after it comes back on syncs afresh. Everything
//! that reads or writes a login sits behind the traits in
//! [`notchling_engine::limits::sources`]; the daemon's implementations over the real stores are
//! [`crate::limits_sources`].
//!
//! | Op | Tier | Args | Reply |
//! | --- | --- | --- | --- |
//! | `limits.status` | app | `{}` | see below |
//! | `limits.set_mode` | app | `{provider, mode?, switch_at?}` | `{"provider", "mode", "switch_at"}` |
//! | `cost.summary` | agent | `{range?}` | see [`crate::cost_ops`] |
//!
//! ```json
//! {"accounts_on": false, "watching": false, "switch_at": 95,
//!  "providers": [{"id": "claude", "label": "Claude Code", "mode": "notifyAndSwitch",
//!                 "modes": ["off", "notify", "notifyAndSwitch"], "default": "notifyAndSwitch",
//!                 "registered": false, "active": "me@example.com" | null,
//!                 "windows": [{"name": "5-hour", "percent": 41.0, "resets_at": 1794150245.0}],
//!                 "last_switch": 1794150245.0 | null, "checked": 1794150245.0 | null}],
//!  "notices": [{"at": 1794150245.0, "text": "me@example.com is at 91% of its 5-hour limit."}]}
//! ```
//!
//! - `providers` are `claude`, `codex` and `antigravity`. `registered` says a store feeds that
//!   watcher; `active`, `windows`, `last_switch` and `checked` are what its last check saw (`null`
//!   or empty before one). `notices` are the last 20 things the watchers said, newest last.
//! - `limits.status` and `limits.set_mode` work while `accounts` is off: they read and write only
//!   the daemon's own memory and its own `daemon/limits` document in `<data>/notchling.db` (the modes and the switch
//!   point, under the Swift keys), never a login.
//! - `limits.set_mode` takes a `provider` and a `mode` (`off`, `notify`, `notifyAndSwitch`) or a
//!   `switch_at` (80 to 99, refused outside it) or both. Antigravity refuses `notifyAndSwitch`.
//! - Every message the watchers say goes out as `limits.notice {"at", "text"}` on the `limits`
//!   topic (app tier), for the app to show.

use crate::acting_ops::Room;
use crate::cost_ops;
use crate::layout::Layout;
use crate::op_group::OpGroup;
use crate::switches::SwitchFlag;
use notchling_engine::cost::CostService;
use notchling_engine::limits::claude::Announce;
use notchling_engine::limits::mode::{LimitConfig, LimitMode, CLAUDE, PROVIDERS};
use notchling_engine::limits::provider::Setup;
use notchling_engine::limits::sources::{ClaudeSource, ProviderSource, StoppedJobs};
use notchling_engine::limits::{ClaudeWatcher, Observed, ProviderLimitWatcher};
use notchling_local_api::{ApiError, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

pub const LIMITS_STATUS: &str = "limits.status";
pub const LIMITS_SET_MODE: &str = "limits.set_mode";
pub const COST_SUMMARY: &str = "cost.summary";
/// The topic and event the watchers' messages go out on.
pub const TOPIC_LIMITS: &str = "limits";
pub const EVENT_NOTICE: &str = "limits.notice";

/// How often the loop looks, and how often it asks the stores for fresh limits (the Swift
/// `AccountStore`'s own round).
pub const TICK_EVERY: Duration = Duration::from_secs(10);
pub const SYNC_EVERY: f64 = 5.0 * 60.0;
/// Messages kept for `limits.status`.
const NOTICES: usize = 20;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A watcher and the store that feeds it.
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

/// What `limits.status` shows, kept apart from the watchers so a status call never waits for a
/// check (which may be a switch).
#[derive(Default)]
struct Board {
    registered: Vec<&'static str>,
    seen: HashMap<&'static str, Observed>,
}

pub struct LimitsOps {
    accounts: Arc<SwitchFlag>,
    /// Set when `accounts` goes off: the next round syncs before it checks.
    resync: Arc<AtomicBool>,
    config: Arc<LimitConfig>,
    watchers: Mutex<Watchers>,
    board: Arc<Mutex<Board>>,
    notices: Arc<Mutex<VecDeque<(f64, String)>>>,
    room: Arc<dyn Room>,
    cost: Arc<CostService>,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    tick: Duration,
    stop: Mutex<Option<Sender<()>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
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
        // Off: forget what the stores said (it may be stale by the time accounts come back), and
        // never wait here for a round that may be mid-switch.
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
            now: Arc::new(system_now),
            tick: TICK_EVERY,
            stop: Mutex::new(None),
            thread: Mutex::new(None),
        }
    }

    /// The daemon's: its own limits document (`daemon/limits` in `<data>/notchling.db`, in memory for a daemon
    /// given `--socket`) and the cost tracker over the layout's folders.
    pub fn for_daemon(
        layout: &Layout,
        accounts: Arc<SwitchFlag>,
        room: Arc<dyn Room>,
    ) -> LimitsOps {
        let config = if layout.socket_given {
            LimitConfig::in_memory()
        } else {
            LimitConfig::open(&layout.data_dir)
        };
        LimitsOps::new(
            accounts,
            Arc::new(config),
            cost_ops::CostSetup::from_layout(layout).service(),
            room,
        )
    }

    /// A fixed clock (tests).
    pub fn with_clock(mut self, now: Arc<dyn Fn() -> f64 + Send + Sync>) -> LimitsOps {
        self.now = now;
        self
    }

    /// A shorter tick for the loop (tests).
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

    /// Where the watchers' messages go: remembered for `limits.status`, and sent as an event.
    fn announcer(&self) -> Announce {
        let notices = self.notices.clone();
        let room = self.room.clone();
        let now = self.now.clone();
        Arc::new(move |text: &str| {
            let at = now();
            let mut list = lock(&notices);
            list.push_back((at, text.to_string()));
            while list.len() > NOTICES {
                list.pop_front();
            }
            drop(list);
            room.publish(TOPIC_LIMITS, EVENT_NOTICE, &json!({"at": at, "text": text}));
        })
    }

    /// Feeds the Claude Code watcher from the account store and the jobs that stopped on a limit.
    pub fn set_claude(&self, source: Arc<dyn ClaudeSource>, jobs: Arc<dyn StoppedJobs>) {
        let watcher =
            ClaudeWatcher::new(source.clone(), jobs, self.config.clone(), self.announcer());
        self.put(Slot::Claude {
            source,
            watcher: Box::new(watcher),
        });
    }

    /// Feeds Codex's watcher. `idle` says nothing would be interrupted by a switch (no `codex`
    /// process, the ChatGPT app closed); it runs only once a target exists.
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

    /// Feeds Antigravity's watcher: Off or Notify only, at the default switch point.
    pub fn set_antigravity(&self, source: Arc<dyn ProviderSource>) {
        let watcher = ProviderLimitWatcher::new(
            source.clone(),
            Setup::antigravity(),
            self.config.clone(),
            Box::new(|| notchling_engine::limits::logic::DEFAULT_SWITCH_AT),
            Box::new(|| true),
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

    /// One round of the loop at `now`. With `accounts` off it does nothing and touches no source;
    /// true when it ran.
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

    /// Starts the loop on a thread of its own: one wake-up per tick, and each tick is a flag check
    /// while `accounts` is off. [`LimitsOps::close`] ends it.
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
                // A timeout is a tick; a message or a closed channel ends the loop.
                while let Err(RecvTimeoutError::Timeout) = stop_rx.recv_timeout(ops.tick) {
                    ops.tick_at((ops.now)());
                }
            })
            .ok();
    }

    /// Stops the loop and saves the cost cache.
    pub fn close(&self) {
        let stop = lock(&self.stop).take();
        drop(stop);
        let thread = lock(&self.thread).take();
        if let Some(handle) = thread {
            let _ = handle.join();
        }
        self.cost.save((self.now)());
    }

    // MARK: Ops

    pub fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            LIMITS_STATUS | LIMITS_SET_MODE => Some(OpSpec::APP),
            COST_SUMMARY => Some(OpSpec::AGENT),
            _ => None,
        }
    }

    pub fn call(&self, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            LIMITS_STATUS => Ok(self.status()),
            LIMITS_SET_MODE => self.set_mode(args),
            COST_SUMMARY => cost_ops::call(&self.cost, args, (self.now)()),
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
        // Everything is checked before anything is written.
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
