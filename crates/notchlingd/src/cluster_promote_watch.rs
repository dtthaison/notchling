use crate::cluster_move::lock;
use crate::cluster_promote::Promoter;
use crate::cluster_promote_items::{action, open_item, Items};
use crate::inbox_ops::Responder;
use notchling_engine::cluster::failover::REACH_MS;
use notchling_engine::cluster::text::{plain, NAME_MAX};
use notchling_engine::cluster::NodeId;
use notchling_engine::inbox::{ActionId, Item, Kind, State};
use notchling_engine::pages::clock::{civil_at, local_offset_secs};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const UNREACHABLE_AFTER_MS: u64 = 60_000;
pub const NO_PRIMARY_AFTER_MS: u64 = 120_000;
pub const EVERY: Duration = Duration::from_secs(5);

pub type StatusFn = Arc<dyn Fn() -> Value + Send + Sync>;
pub type OffsetFn = Arc<dyn Fn(i64) -> i64 + Send + Sync>;

pub fn clock_text(wall_ms: i64, offset_secs: i64) -> String {
    let civil = civil_at(wall_ms as f64 / 1000.0, offset_secs);
    format!("{:02}:{:02}", civil.hour, civil.minute)
}

struct View {
    me: NodeId,
    me_name: String,
    home: NodeId,
    home_name: String,
    term: u64,
    client: bool,
    can_be_master: bool,
    home_online: bool,
    peers: Vec<NodeId>,
    takes_over_at: Option<i64>,
}

fn node_of(value: &Value) -> Option<NodeId> {
    value.as_str().and_then(NodeId::parse)
}

fn view_of(status: &Value) -> Option<View> {
    let me = node_of(&status["me"])?;
    let home = node_of(&status["cluster"]["home"])?;
    let term = status["cluster"]["term"]["n"].as_u64()?;
    let members = status["members"].as_array()?;
    let find = |id: &NodeId| {
        members
            .iter()
            .find(|m| m["id"].as_str() == Some(id.as_str()))
    };
    let mine = find(&me)?;
    let theirs = find(&home);
    let name = |m: &Value, id: &NodeId| {
        m["name"]
            .as_str()
            .map_or_else(|| id.to_string(), |n| plain(n, NAME_MAX))
    };
    let peers = members
        .iter()
        .filter(|m| m["state"] == "active" && m["canBeMaster"] == true && m["online"] == true)
        .filter_map(|m| node_of(&m["id"]))
        .filter(|id| *id != me && *id != home)
        .collect();
    let failover = &status["failover"];
    let on = failover["value"].as_str().is_some_and(|v| v != "off");
    let mine_to_take = on && node_of(&failover["target"]).as_ref() == Some(&me);
    let armed = failover["armed"] == true;
    Some(View {
        me_name: name(mine, &me),
        home_name: theirs.map_or_else(|| home.to_string(), |m| name(m, &home)),
        client: mine["client"] == true,
        can_be_master: mine["canBeMaster"] == true,
        home_online: theirs.is_some_and(|m| m["online"] == true),
        takes_over_at: failover["takesOverAt"]
            .as_i64()
            .filter(|_| mine_to_take && armed),
        me,
        home,
        term,
        peers,
    })
}

#[derive(Default)]
struct Memory {
    since: Option<u64>,
    raised: BTreeMap<String, String>,
}

pub struct Watcher {
    promoter: Arc<Promoter>,
    status: StatusFn,
    offset: OffsetFn,
    memory: Mutex<Memory>,
}

fn unreachable_id(term: u64) -> String {
    format!("primary-unreachable:{term}")
}

fn confirm_id(term: u64) -> String {
    format!("primary-unreachable:{term}:confirm")
}

fn no_primary_id(term: u64) -> String {
    format!("no-primary:{term}")
}

struct Moving {
    promoter: Arc<Promoter>,
    items: Items,
    term: u64,
    me_name: String,
    confirmed: bool,
}

impl Responder for Moving {
    fn answered(&self, _item: &Item, answered: ActionId) -> bool {
        let own = match self.confirmed {
            true => ActionId::MoveAnyway,
            false => ActionId::MoveHere,
        };
        if answered != own || !self.items.acting.is_on() {
            return false;
        }
        let (promoter, items) = (self.promoter.clone(), self.items.clone());
        let (term, name, confirmed) = (self.term, self.me_name.clone(), self.confirmed);
        std::thread::Builder::new()
            .name("cluster-promote-answer".into())
            .spawn(move || report(&promoter, &items, (term, &name), confirmed))
            .is_ok()
    }
}

fn report(promoter: &Arc<Promoter>, items: &Items, (term, name): (u64, &str), confirmed: bool) {
    let title = format!("Couldn't move to {name}");
    match promoter.start(confirmed) {
        Ok(answer) => {
            if let Some(risk) = answer["needsConfirm"].as_str() {
                match confirmed {
                    false => raise_confirm(promoter, items, (term, name), risk),
                    true => items.failed(&format!("promote-start:{term}"), &title, risk),
                }
            }
        }
        Err(e) => items.failed(&format!("promote-start:{term}"), &title, &e.message),
    }
}

fn raise_confirm(promoter: &Arc<Promoter>, items: &Items, (term, name): (u64, &str), risk: &str) {
    let title = format!("{name} can't reach the primary");
    let mut item = open_item(
        &confirm_id(term),
        Kind::PrimaryUnreachable,
        (title, risk.to_string()),
        items.now(),
    );
    item.actions = vec![action(ActionId::MoveAnyway, "Move Anyway", true)];
    let responder = Arc::new(Moving {
        promoter: promoter.clone(),
        items: items.clone(),
        term,
        me_name: name.to_string(),
        confirmed: true,
    });
    items.inbox.raise(item, Some(responder));
}

impl Watcher {
    pub fn new(promoter: Arc<Promoter>, status: StatusFn) -> Arc<Watcher> {
        Watcher::with_offset(
            promoter,
            status,
            Arc::new(|at| local_offset_secs(at as f64)),
        )
    }

    pub fn with_offset(
        promoter: Arc<Promoter>,
        status: StatusFn,
        offset: OffsetFn,
    ) -> Arc<Watcher> {
        Arc::new(Watcher {
            promoter,
            status,
            offset,
            memory: Mutex::new(Memory::default()),
        })
    }

    fn reaches(&self, view: &View) -> bool {
        if view.home_online {
            return true;
        }
        let wait = Duration::from_millis(REACH_MS);
        view.peers.iter().any(|peer| {
            let args = json!({"node": view.home});
            self.promoter
                .p
                .cluster
                .call(peer, "node.reach", args, wait)
                .is_ok_and(|answer| answer["online"] == true)
        })
    }

    pub fn tick(&self) {
        let now = (self.promoter.p.boot.now)();
        let Some(view) = view_of(&(self.status)()) else {
            self.withdraw_except(None);
            return;
        };
        if view.me == view.home || self.reaches(&view) {
            lock(&self.memory).since = None;
            self.withdraw_except(None);
            return;
        }
        let down = {
            let mut memory = lock(&self.memory);
            now.saturating_sub(*memory.since.get_or_insert(now))
        };
        self.withdraw_except(Some(view.term));
        if view.can_be_master && down >= UNREACHABLE_AFTER_MS {
            self.raise_unreachable(&view);
        } else if view.client && !view.can_be_master && down >= NO_PRIMARY_AFTER_MS {
            self.raise_no_primary(&view);
        }
    }

    fn withdraw_except(&self, term: Option<u64>) {
        let keep = |id: &str| {
            term.is_some_and(|n| {
                [unreachable_id(n), confirm_id(n), no_primary_id(n)]
                    .iter()
                    .any(|own| own == id)
            })
        };
        let gone: Vec<String> = {
            let mut memory = lock(&self.memory);
            let ids: Vec<String> = memory
                .raised
                .keys()
                .filter(|id| !keep(id))
                .cloned()
                .collect();
            for id in &ids {
                memory.raised.remove(id);
            }
            ids
        };
        for id in gone {
            self.promoter
                .items()
                .inbox
                .resolve(&id, State::Withdrawn, None, None);
        }
    }

    fn fresh(&self, id: &str, signature: &str) -> bool {
        let mut memory = lock(&self.memory);
        if memory.raised.get(id).is_some_and(|seen| seen == signature) {
            return false;
        }
        memory.raised.insert(id.to_string(), signature.to_string());
        true
    }

    fn raise_unreachable(&self, view: &View) {
        let id = unreachable_id(view.term);
        let detail = match view.takes_over_at {
            Some(at) => {
                let offset = (self.offset)(at / 1000);
                format!("{} takes over at {}.", view.me_name, clock_text(at, offset))
            }
            None => String::new(),
        };
        if !self.fresh(&id, &detail) {
            return;
        }
        let items = self.promoter.items();
        let title = format!("{} hasn't answered for 1 minute", view.home_name);
        let mut item = open_item(&id, Kind::PrimaryUnreachable, (title, detail), items.now());
        item.actions = vec![action(
            ActionId::MoveHere,
            &format!("Move to {} Now", view.me_name),
            true,
        )];
        let responder = Arc::new(Moving {
            promoter: self.promoter.clone(),
            items: items.clone(),
            term: view.term,
            me_name: view.me_name.clone(),
            confirmed: false,
        });
        items.inbox.raise(item, Some(responder));
    }

    fn raise_no_primary(&self, view: &View) {
        let id = no_primary_id(view.term);
        let detail = format!("{} was the primary.", view.home_name);
        if !self.fresh(&id, &detail) {
            return;
        }
        let items = self.promoter.items();
        let title = "No primary can be reached".to_string();
        let item = open_item(&id, Kind::NoPrimary, (title, detail), items.now());
        items.inbox.raise(item, None);
    }
}

pub struct WatchThread {
    stop: Mutex<Option<Sender<()>>>,
}

impl WatchThread {
    pub fn spawn(watcher: Arc<Watcher>, every: Duration) -> WatchThread {
        let (stop, stopped) = channel::<()>();
        let spawned = std::thread::Builder::new()
            .name("cluster-promote-watch".into())
            .spawn(move || {
                while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(every) {
                    watcher.tick();
                }
            });
        WatchThread {
            stop: Mutex::new(spawned.ok().map(|_| stop)),
        }
    }

    pub fn close(&self) {
        lock(&self.stop).take();
    }
}
