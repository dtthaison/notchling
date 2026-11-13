use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use notchling_engine::limits::report::UsageReport;
use notchling_local_api::{ClientError, SOCKET_NAME};
use serde_json::{json, Value};

use super::wire::{
    parse_add, parse_claude_list, parse_claude_usage, parse_cost, parse_lanes, parse_limits,
    parse_move_plan, parse_move_result, parse_outcome, parse_providers, parse_usage,
    still_refreshing, working_ticket, AddProgress, Call, ClaudeList, ClaudeUsage, CostReply, Lane,
    LimitsStatus, MovePlan, MoveResult, Outcome, ProviderList,
};
use crate::actions::{Socket, Transport};
use crate::chat::data::DataRoot;
use crate::switches::{self, Switch, Switches};

const SETTLE_LIMIT: Duration = Duration::from_secs(150);

const USAGE: &str = "usage";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub attached: bool,
    pub claude: ClaudeList,
    pub lanes: Vec<Lane>,
    pub providers: ProviderList,
    pub usage: BTreeMap<String, BTreeMap<String, UsageReport>>,
    pub claude_usage: ClaudeUsage,
    pub limits: LimitsStatus,
    pub cost: Option<CostReply>,
    pub add: Option<AddProgress>,
    pub error: Option<String>,
}

pub const SYNC_EVERY: f64 = 30.0;

const SETTLE_POLLS: u32 = 12;
const SETTLE_EVERY: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Flight {
    running: bool,
    again: bool,
    force_again: bool,
}

#[derive(Default)]
struct Forced {
    providers: BTreeMap<String, BTreeMap<String, UsageReport>>,
    claude: Option<ClaudeUsage>,
}

pub struct Accounts {
    transport: Option<Box<dyn Transport>>,
    snapshot: Mutex<Snapshot>,
    flight: Mutex<Flight>,
    last_sync: Mutex<Option<f64>>,
    add_polling: AtomicBool,
    settle_every: Duration,
}

fn message(e: &ClientError) -> String {
    match e {
        ClientError::Api(api) => api.message.clone(),
        other => other.to_string(),
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Accounts {
    pub fn over(transport: Box<dyn Transport>) -> Accounts {
        Accounts {
            transport: Some(transport),
            snapshot: Mutex::new(Snapshot::default()),
            flight: Mutex::default(),
            last_sync: Mutex::new(None),
            add_polling: AtomicBool::new(false),
            settle_every: SETTLE_EVERY,
        }
    }

    pub fn with_settle_every(mut self, wait: Duration) -> Accounts {
        self.settle_every = wait;
        self
    }

    pub fn fixed(snapshot: Snapshot) -> Accounts {
        Accounts {
            transport: None,
            snapshot: Mutex::new(snapshot),
            flight: Mutex::default(),
            last_sync: Mutex::new(None),
            add_polling: AtomicBool::new(false),
            settle_every: SETTLE_EVERY,
        }
    }

    pub fn for_run(root: Option<&DataRoot>) -> Accounts {
        match root.filter(|r| r.fixture.is_none()) {
            Some(root) => Accounts::over(Box::new(Socket {
                socket: root.data_dir.join(SOCKET_NAME),
                version: env!("CARGO_PKG_VERSION").to_string(),
            })),
            None => Accounts::fixed(Snapshot::default()),
        }
    }

    pub fn send_limit_setting(&self, key: &str, value: &Value) -> Result<(), String> {
        let Some(call) = Call::limits_setting(key, value) else {
            return Ok(());
        };
        self.call(&call).map(|_| ()).map_err(|e| e.to_string())
    }

    pub fn is_live(&self) -> bool {
        self.transport.is_some()
    }

    pub fn snapshot(&self) -> Snapshot {
        lock(&self.snapshot).clone()
    }

    fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        f(&mut lock(&self.snapshot));
    }

    fn call(&self, call: &Call) -> Result<Value, ClientError> {
        let Some(transport) = &self.transport else {
            return Err(ClientError::Closed);
        };
        let result = transport.call(call.op, call.args.clone());
        if let Err(e) = &result {
            if switches::detaches(e) {
                self.update(|s| *s = Snapshot::default());
            }
        }
        result
    }

    fn gate(&self, switches: &Switches) -> Result<(), String> {
        if self.transport.is_none() {
            return Err(crate::acting::DETACHED_MESSAGE.into());
        }
        match switches.message(Switch::Accounts) {
            Some(why) => Err(why.to_owned()),
            None => Ok(()),
        }
    }

    fn send(&self, switches: &Switches, call: &Call) -> Result<Value, String> {
        self.gate(switches)?;
        let mut reply = self.call(call);
        let deadline = Instant::now() + SETTLE_LIMIT;
        while let Ok(ticket) = reply.as_ref().map(working_ticket) {
            let Some(ticket) = ticket else { break };
            if Instant::now() >= deadline {
                reply = Err(ClientError::Api(notchling_local_api::ApiError::new(
                    notchling_local_api::ErrorCode::Failed,
                    "The daemon is still working on it; the list updates when it's done.",
                )));
                break;
            }
            reply = self.call(&Call::accounts_result(ticket));
        }
        reply.map_err(|e| {
            let line = message(&e);
            if !switches::detaches(&e) {
                self.update(|s| s.error = Some(line.clone()));
            }
            line
        })
    }

    pub fn refresh(&self, switches: &Switches) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        self.refresh_at(switches, now);
    }

    pub fn refresh_at(&self, switches: &Switches, now: f64) {
        self.run_refresh(switches, now, false);
    }

    pub fn refresh_all(&self, switches: &Switches) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        self.refresh_all_at(switches, now);
    }

    pub fn refresh_all_at(&self, switches: &Switches, now: f64) {
        self.run_refresh(switches, now, true);
    }

    fn run_refresh(&self, switches: &Switches, now: f64, force: bool) {
        if self.transport.is_none() {
            return;
        }
        {
            let mut flight = lock(&self.flight);
            if flight.running {
                flight.again = true;
                flight.force_again |= force;
                return;
            }
            flight.running = true;
        }
        let mut force = force;
        loop {
            self.read_lists(switches, now, force);
            let mut flight = lock(&self.flight);
            if flight.again {
                flight.again = false;
                force = std::mem::take(&mut flight.force_again);
                continue;
            }
            flight.running = false;
            return;
        }
    }

    fn sync_due(&self, on: bool, now: f64, force: bool) -> bool {
        let mut last = lock(&self.last_sync);
        if !on {
            *last = None;
            return false;
        }
        let due = force || last.is_none_or(|at| now - at >= SYNC_EVERY || now < at);
        if due {
            *last = Some(now);
        }
        due
    }

    fn settled(&self, first: Call, again: Call) -> Option<Value> {
        let mut reply = self.call(&first).ok()?;
        for _ in 0..SETTLE_POLLS {
            if !still_refreshing(&reply) {
                break;
            }
            std::thread::sleep(self.settle_every);
            match self.call(&again) {
                Ok(next) => reply = next,
                Err(_) => break,
            }
        }
        Some(reply)
    }

    fn read_forced(&self) -> Forced {
        let mut known = self.snapshot().providers;
        if known.providers.is_empty() {
            if let Ok(v) = self.call(&Call::providers_list(false)) {
                known = parse_providers(&v);
            }
        }
        let mut forced = Forced::default();
        for p in known.providers.iter().filter(|p| p.can(USAGE)) {
            let first = Call::providers_usage(&p.id, true);
            if let Some(v) = self.settled(first, Call::providers_usage(&p.id, false)) {
                forced.providers.insert(p.id.clone(), parse_usage(&v));
            }
        }
        forced.claude = self
            .settled(Call::limits_usage(true), Call::limits_usage(false))
            .map(|v| parse_claude_usage(&v));
        forced
    }

    fn read_lists(&self, switches: &Switches, now: f64, force: bool) {
        let on = switches.is_on(Switch::Accounts);
        let sync = self.sync_due(on, now, force);
        let mut forced = if sync && force {
            self.read_forced()
        } else {
            Forced::default()
        };
        let claude = match self.call(&Call::accounts_list(sync)) {
            Ok(v) => parse_claude_list(&v),
            Err(e) => {
                if !switches::detaches(&e) {
                    self.update(|s| {
                        s.attached = true;
                        s.error = Some(message(&e));
                    });
                }
                return;
            }
        };
        let lanes = self.call(&Call::lanes_list()).map(|v| parse_lanes(&v));
        let providers = self
            .call(&Call::providers_list(sync))
            .map(|v| parse_providers(&v));
        let limits = self.call(&Call::limits_status()).map(|v| parse_limits(&v));
        if lanes.as_ref().err().is_some_and(switches::detaches)
            || providers.as_ref().err().is_some_and(switches::detaches)
            || limits.as_ref().err().is_some_and(switches::detaches)
        {
            return;
        }
        let mut usage = BTreeMap::new();
        if let (true, Ok(list)) = (sync, &providers) {
            for p in list.providers.iter().filter(|p| p.can(USAGE)) {
                if let Some(reports) = forced.providers.remove(&p.id) {
                    usage.insert(p.id.clone(), reports);
                } else if let Ok(v) = self.call(&Call::providers_usage(&p.id, false)) {
                    usage.insert(p.id.clone(), parse_usage(&v));
                }
            }
        }
        let claude_usage = match (sync, forced.claude.take()) {
            (false, _) => None,
            (true, Some(read)) => Some(read),
            (true, None) => self
                .call(&Call::limits_usage(false))
                .ok()
                .map(|v| parse_claude_usage(&v)),
        };
        self.update(|s| {
            s.attached = true;
            s.claude = claude;
            if let Ok(lanes) = lanes {
                s.lanes = lanes;
            }
            if let Ok(providers) = providers {
                s.providers = providers;
            }
            if let Ok(limits) = limits {
                s.limits = limits;
            }
            if sync {
                s.usage = usage;
                if let Some(read) = claude_usage {
                    s.claude_usage = read;
                }
            } else if !on {
                s.usage.clear();
                s.claude_usage = ClaudeUsage::default();
            }
            s.error = None;
        });
    }

    pub fn refresh_cost(&self, range: &str) {
        if let Ok(v) = self.call(&Call::cost_summary(range)) {
            let cost = parse_cost(&v);
            self.update(|s| {
                s.attached = true;
                s.cost = Some(cost);
            });
        }
    }

    pub fn switch_claude(&self, switches: &Switches, id: &str) -> Result<(), String> {
        let v = self.send(switches, &Call::accounts_switch(id))?;
        self.update(|s| s.claude = parse_claude_list(&v));
        Ok(())
    }

    pub fn remove_claude(&self, switches: &Switches, id: &str) -> Result<(), String> {
        let v = self.send(switches, &Call::accounts_remove(id))?;
        self.update(|s| s.claude = parse_claude_list(&v));
        Ok(())
    }

    pub fn begin_add_claude(&self, switches: &Switches) -> Result<String, String> {
        let v = self.send(switches, &Call::accounts_add_begin())?;
        let progress = parse_add(&v).ok_or("The daemon began a sign-in this app can't read")?;
        let url = progress.url.clone().unwrap_or_default();
        self.update(|s| s.add = Some(progress));
        Ok(url)
    }

    pub fn poll_add_claude(&self, switches: &Switches, cancel: bool) -> Result<(), String> {
        let Some(login) = self.snapshot().add.map(|a| a.login) else {
            return Ok(());
        };
        let v = self.send(switches, &Call::accounts_add_finish(&login, cancel))?;
        if let Some(progress) = parse_add(&v) {
            self.update(|s| s.add = Some(progress));
        }
        Ok(())
    }

    pub fn claim_add_poll(&self) -> bool {
        self.add_polling
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub fn add_poll_step(&self, switches: &Switches) -> bool {
        if !self.add_waiting() {
            return false;
        }
        self.poll_add_claude(switches, false).is_ok() && self.add_waiting()
    }

    pub fn end_add_poll(&self, switches: &Switches) {
        if self.add_waiting() {
            let _ = self.poll_add_claude(switches, true);
            self.clear_add();
        }
        self.add_polling.store(false, Ordering::SeqCst);
    }

    fn add_waiting(&self) -> bool {
        self.snapshot().add.is_some_and(|a| a.state == "waiting")
    }

    pub fn clear_add(&self) {
        self.update(|s| s.add = None);
    }

    pub fn create_lane(&self, switches: &Switches, account: &str) -> Result<Lane, String> {
        let v = self.send(switches, &Call::lanes_create(account))?;
        let lane = v.get("lane").cloned().unwrap_or(Value::Null);
        parse_lanes(&json!({ "lanes": [lane] }))
            .pop()
            .ok_or_else(|| "The daemon made a lane this app can't read".to_owned())
    }

    pub fn move_all_plan(
        &self,
        switches: &Switches,
        target: Option<&str>,
    ) -> Result<MovePlan, String> {
        let v = self.send(switches, &Call::accounts_move_all(target, true))?;
        Ok(parse_move_plan(&v))
    }

    pub fn move_all_now(
        &self,
        switches: &Switches,
        target: Option<&str>,
    ) -> Result<MoveResult, String> {
        let v = self.send(switches, &Call::accounts_move_all(target, false))?;
        Ok(parse_move_result(&v))
    }

    pub fn switch_provider(
        &self,
        switches: &Switches,
        provider: &str,
        id: &str,
        confirm: bool,
    ) -> Result<Outcome, String> {
        let v = self.send(switches, &Call::providers_switch(provider, id, confirm))?;
        Ok(parse_outcome(&v))
    }

    pub fn add_provider(
        &self,
        switches: &Switches,
        provider: &str,
        confirm: bool,
    ) -> Result<Outcome, String> {
        let v = self.send(switches, &Call::providers_add_begin(provider, confirm))?;
        Ok(parse_outcome(&v))
    }

    pub fn cancel_add_provider(&self, switches: &Switches, provider: &str) -> Result<(), String> {
        self.send(switches, &Call::providers_add_cancel(provider))
            .map(|_| ())
    }

    pub fn remove_provider(
        &self,
        switches: &Switches,
        provider: &str,
        id: &str,
    ) -> Result<(), String> {
        self.send(switches, &Call::providers_remove(provider, id))
            .map(|_| ())
    }

    pub fn refresh_usage(&self, switches: &Switches, provider: &str) -> Result<(), String> {
        let v = self.send(switches, &Call::providers_usage(provider, true))?;
        let reports = parse_usage(&v);
        self.update(|s| {
            s.usage.insert(provider.to_owned(), reports);
        });
        Ok(())
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
