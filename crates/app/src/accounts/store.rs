//! What the app knows about accounts, lanes, the other providers, limits and cost, and what it
//! asks the daemon (Milestone 6, U5). Every call blocks, so the caller makes it off the UI thread;
//! [`Accounts`] keeps a [`Snapshot`] of what the daemon last said and the views draw from that,
//! never from a socket. A render has no transport ([`Accounts::fixed`]).
//!
//! The lists (`accounts.list`, `lanes.list`, `providers.list`, `limits.status`, `cost.summary`)
//! are read whether the `accounts` switch is on or off: off, the daemon lists its own saved names
//! and nothing else. Every other op is sent only when the switch is on ([`Accounts::gate`]), so a
//! control that slipped past its disabled state still reaches no login.

use std::collections::BTreeMap;
use std::sync::Mutex;

use notchling_engine::limits::report::UsageReport;
use notchling_local_api::{ClientError, SOCKET_NAME};
use serde_json::{json, Value};

use super::wire::{
    parse_add, parse_claude_list, parse_cost, parse_lanes, parse_limits, parse_move_plan,
    parse_outcome, parse_providers, parse_usage, AddProgress, Call, ClaudeList, CostReply, Lane,
    LimitsStatus, MovePlan, Outcome, ProviderList,
};
use crate::actions::{Socket, Transport};
use crate::chat::data::DataRoot;
use crate::switches::{self, Switch, Switches};

/// The capability name a provider has when `providers.usage` reports for it.
const USAGE: &str = "usage";

/// Everything the Accounts window and Chat's header draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// A daemon answered the last read.
    pub attached: bool,
    pub claude: ClaudeList,
    pub lanes: Vec<Lane>,
    pub providers: ProviderList,
    /// Limit reports by provider id, then account id.
    pub usage: BTreeMap<String, BTreeMap<String, UsageReport>>,
    pub limits: LimitsStatus,
    pub cost: Option<CostReply>,
    /// A Claude Code sign-in under way, or how the last one ended.
    pub add: Option<AddProgress>,
    /// The last failure to show, or none.
    pub error: Option<String>,
}

/// How often, at most, a refresh syncs the live logins and reads the providers' limits (the Swift
/// app's rate). Refreshes between, on a tick or an event, read the lists without syncing.
pub const SYNC_EVERY: f64 = 30.0;

/// One refresh at a time; one asked for meanwhile runs once after.
#[derive(Default)]
struct Flight {
    running: bool,
    again: bool,
}

pub struct Accounts {
    transport: Option<Box<dyn Transport>>,
    snapshot: Mutex<Snapshot>,
    flight: Mutex<Flight>,
    /// When the last syncing refresh went out (seconds since 1970).
    last_sync: Mutex<Option<f64>>,
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
    /// Over a transport; detached until the daemon answers.
    pub fn over(transport: Box<dyn Transport>) -> Accounts {
        Accounts {
            transport: Some(transport),
            snapshot: Mutex::new(Snapshot::default()),
            flight: Mutex::default(),
            last_sync: Mutex::new(None),
        }
    }

    /// No daemon behind it, showing `snapshot` as given: a render's.
    pub fn fixed(snapshot: Snapshot) -> Accounts {
        Accounts {
            transport: None,
            snapshot: Mutex::new(snapshot),
            flight: Mutex::default(),
            last_sync: Mutex::new(None),
        }
    }

    /// The running app's: the daemon's socket in the data folder, or nothing for a fixture root.
    pub fn for_run(root: Option<&DataRoot>) -> Accounts {
        match root.filter(|r| r.fixture.is_none()) {
            Some(root) => Accounts::over(Box::new(Socket {
                socket: root.data_dir.join(SOCKET_NAME),
                version: env!("CARGO_PKG_VERSION").to_string(),
            })),
            None => Accounts::fixed(Snapshot::default()),
        }
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

    /// One call. A lost daemon detaches (and clears what was known).
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

    /// Refuses, unsent, an op the `accounts` switch doesn't allow; the error is the line to show.
    fn gate(&self, switches: &Switches) -> Result<(), String> {
        if self.transport.is_none() {
            return Err(crate::acting::DETACHED_MESSAGE.into());
        }
        match switches.message(Switch::Accounts) {
            Some(why) => Err(why.to_owned()),
            None => Ok(()),
        }
    }

    /// A call the switch allows; its failure is kept as the snapshot's error and returned.
    fn send(&self, switches: &Switches, call: &Call) -> Result<Value, String> {
        self.gate(switches)?;
        self.call(call).map_err(|e| {
            let line = message(&e);
            if !switches::detaches(&e) {
                self.update(|s| s.error = Some(line.clone()));
            }
            line
        })
    }

    /// Reads every list. Blocking. See [`Accounts::refresh_at`].
    pub fn refresh(&self, switches: &Switches) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        self.refresh_at(switches, now);
    }

    /// Reads every list at `now`. Blocking. Never two at once: one asked for while another runs
    /// makes that one run once more after, and returns at once. With the switch on, the live login
    /// is synced and the providers' limits read at most every [`SYNC_EVERY`] seconds; the
    /// daemon publishes `providers.changed` for each list read, so syncing on every event would
    /// feed itself.
    pub fn refresh_at(&self, switches: &Switches, now: f64) {
        if self.transport.is_none() {
            return;
        }
        {
            let mut flight = lock(&self.flight);
            if flight.running {
                flight.again = true;
                return;
            }
            flight.running = true;
        }
        loop {
            self.read_lists(switches, now);
            let mut flight = lock(&self.flight);
            if flight.again {
                flight.again = false;
                continue;
            }
            flight.running = false;
            return;
        }
    }

    /// Whether this refresh syncs: the switch is on and the last sync is [`SYNC_EVERY`] old.
    fn sync_due(&self, on: bool, now: f64) -> bool {
        let mut last = lock(&self.last_sync);
        if !on {
            *last = None;
            return false;
        }
        let due = last.is_none_or(|at| now - at >= SYNC_EVERY || now < at);
        if due {
            *last = Some(now);
        }
        due
    }

    fn read_lists(&self, switches: &Switches, now: f64) {
        let on = switches.is_on(Switch::Accounts);
        let sync = self.sync_due(on, now);
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
        // A daemon without one of these (older, or a refusal) still has the rest; a daemon that
        // went away mid-read has nothing more to say.
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
                if let Ok(v) = self.call(&Call::providers_usage(&p.id, false)) {
                    usage.insert(p.id.clone(), parse_usage(&v));
                }
            }
        }
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
            } else if !on {
                s.usage.clear();
            }
            s.error = None;
        });
    }

    /// Reads the cost summary for `range` ("7", "30", "all"). Not switched. Blocking.
    pub fn refresh_cost(&self, range: &str) {
        if let Ok(v) = self.call(&Call::cost_summary(range)) {
            let cost = parse_cost(&v);
            self.update(|s| {
                s.attached = true;
                s.cost = Some(cost);
            });
        }
    }

    /// Switches the main login (`accounts.switch`). Blocking.
    pub fn switch_claude(&self, switches: &Switches, id: &str) -> Result<(), String> {
        let v = self.send(switches, &Call::accounts_switch(id))?;
        self.update(|s| s.claude = parse_claude_list(&v));
        Ok(())
    }

    /// Forgets a saved Claude Code login (`accounts.remove`). Blocking.
    pub fn remove_claude(&self, switches: &Switches, id: &str) -> Result<(), String> {
        let v = self.send(switches, &Call::accounts_remove(id))?;
        self.update(|s| s.claude = parse_claude_list(&v));
        Ok(())
    }

    /// Starts a Claude Code sign-in (`accounts.add.begin`): the page to open in the browser.
    pub fn begin_add_claude(&self, switches: &Switches) -> Result<String, String> {
        let v = self.send(switches, &Call::accounts_add_begin())?;
        let progress = parse_add(&v).ok_or("The daemon began a sign-in this app can't read")?;
        let url = progress.url.clone().unwrap_or_default();
        self.update(|s| s.add = Some(progress));
        Ok(url)
    }

    /// Waits a moment for the sign-in to end (`accounts.add.finish`, up to 4 s in the daemon);
    /// `cancel` gives it up. Blocking.
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

    /// Forgets how the last sign-in ended.
    pub fn clear_add(&self) {
        self.update(|s| s.add = None);
    }

    /// Makes a lane for a saved account (`lanes.create`): the lane's config folder. Blocking.
    pub fn create_lane(&self, switches: &Switches, account: &str) -> Result<Lane, String> {
        let v = self.send(switches, &Call::lanes_create(account))?;
        let lane = v.get("lane").cloned().unwrap_or(Value::Null);
        parse_lanes(&json!({ "lanes": [lane] }))
            .pop()
            .ok_or_else(|| "The daemon made a lane this app can't read".to_owned())
    }

    /// Move All: a dry run's plan, or the move itself (`accounts.move_all`). Blocking.
    pub fn move_all(
        &self,
        switches: &Switches,
        target: Option<&str>,
        dry_run: bool,
    ) -> Result<MovePlan, String> {
        let v = self.send(switches, &Call::accounts_move_all(target, dry_run))?;
        Ok(parse_move_plan(&v))
    }

    /// Switches another provider's account (`providers.switch`). Blocking.
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

    /// Starts adding another provider's account (`providers.add.begin`). Blocking.
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

    /// Asks a provider to read its limits again (`providers.usage {refresh: true}`). Blocking.
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
