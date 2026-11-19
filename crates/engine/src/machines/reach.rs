use super::addresses::{AddressKind, MachineAddress, Route};
use super::registry::Machine;
use super::runner::{Call, CommandResult, Runner};
use super::shell::ssh_prefix;
use super::tailscale::{Network, Tailscale};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

pub const PROBE_CONNECT_SECS: u64 = 3;
pub const PROBE_SECS: u64 = 8;
pub const KEEP_SECS: f64 = 180.0;
pub const KEEP_FAILED_SECS: f64 = 15.0;
pub const RECHECK_NETWORK_SECS: f64 = 10.0;
const JUMP_DEPTH: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub index: usize,
    pub address: MachineAddress,
    pub route: Route,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    Reached(Choice),
    WrongNetwork { wanted: String, active: String },
    Unreachable,
}

impl Pick {
    pub fn choice(&self) -> Option<&Choice> {
        match self {
            Pick::Reached(choice) => Some(choice),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Probed {
    Up,
    Reached,
    Down,
}

pub fn classify(result: &CommandResult) -> Probed {
    if result.code == 0 {
        return Probed::Up;
    }
    let text = format!("{}\n{}", result.err, result.out);
    let reached = [
        "Permission denied",
        "Host key verification failed",
        "REMOTE HOST IDENTIFICATION HAS CHANGED",
    ]
    .iter()
    .any(|w| text.contains(w));
    if reached {
        Probed::Reached
    } else {
        Probed::Down
    }
}

type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

struct Entry {
    at: f64,
    checked: f64,
    active: Option<String>,
    print: String,
    pick: Pick,
}

struct Live {
    runner: Arc<dyn Runner>,
    tailscale: Tailscale,
    clock: Clock,
    cache: Mutex<BTreeMap<String, Entry>>,
}

pub struct Router {
    live: Option<Live>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn print_of(m: &Machine) -> String {
    format!("{}|{:?}", m.user, m.address_list())
}

impl Router {
    pub fn off() -> Router {
        Router { live: None }
    }

    pub fn new(runner: Arc<dyn Runner>, tailscale: Tailscale, clock: Clock) -> Router {
        Router {
            live: Some(Live {
                runner,
                tailscale,
                clock,
                cache: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    pub fn tailscale(&self) -> Option<&Tailscale> {
        self.live.as_ref().map(|l| &l.tailscale)
    }

    pub fn forget(&self, name: &str) {
        if let Some(live) = &self.live {
            lock(&live.cache).remove(name);
        }
    }

    pub fn forget_all(&self) {
        if let Some(live) = &self.live {
            lock(&live.cache).clear();
        }
    }

    pub fn cached(&self, m: &Machine) -> Option<Pick> {
        let live = self.live.as_ref()?;
        if !m.has_addresses() {
            return None;
        }
        let now = (live.clock)();
        let cache = lock(&live.cache);
        let entry = cache.get(&m.name)?;
        (entry.print == print_of(m) && now - entry.at < ttl(&entry.pick)).then(|| entry.pick.clone())
    }

    pub fn routed(&self, m: &Machine) -> Machine {
        let mut out = m.clone();
        if let Some(Pick::Reached(choice)) = self.cached(m) {
            out.route = Some(choice.route);
        }
        out
    }

    pub fn reached(&self, m: &Machine, all: &[Machine]) -> Machine {
        let mut out = m.clone();
        if self.live.is_none() {
            return out;
        }
        if let Pick::Reached(choice) = self.pick(m, all) {
            if m.has_addresses() {
                out.route = Some(choice.route);
            }
        }
        out
    }

    pub fn pick(&self, m: &Machine, all: &[Machine]) -> Pick {
        self.pick_in(m, all, 0)
    }

    fn pick_in(&self, m: &Machine, all: &[Machine], depth: usize) -> Pick {
        let list = m.address_list();
        let Some(live) = &self.live else {
            return first_of(m, &list);
        };
        if !m.has_addresses() {
            return first_of(m, &list);
        }
        let now = (live.clock)();
        let print = print_of(m);
        let needs_network = list
            .iter()
            .any(|a| a.kind.is_tailscale() && a.tailnet_name().is_some());
        let kept = {
            let cache = lock(&live.cache);
            cache
                .get(&m.name)
                .filter(|e| e.print == print && now - e.at < ttl(&e.pick))
                .map(|e| (e.pick.clone(), e.checked, e.active.clone()))
        };
        if let Some((pick, checked, was)) = kept {
            if !needs_network || now - checked < RECHECK_NETWORK_SECS {
                return pick;
            }
            let active = live.tailscale.active().map(|n| n.tailnet);
            if active == was {
                if let Some(entry) = lock(&live.cache).get_mut(&m.name) {
                    entry.checked = now;
                }
                return pick;
            }
        }
        let active = if needs_network {
            live.tailscale.active()
        } else {
            None
        };
        let pick = self.choose(live, m, all, &list, active.as_ref(), depth);
        lock(&live.cache).insert(
            m.name.clone(),
            Entry {
                at: now,
                checked: now,
                active: active.map(|n| n.tailnet),
                print,
                pick: pick.clone(),
            },
        );
        pick
    }

    fn choose(
        &self,
        live: &Live,
        m: &Machine,
        all: &[Machine],
        list: &[MachineAddress],
        active: Option<&Network>,
        depth: usize,
    ) -> Pick {
        let mut skipped: Vec<String> = Vec::new();
        let mut candidates: Vec<(usize, &MachineAddress)> = Vec::new();
        for (index, address) in list.iter().enumerate() {
            if let (true, Some(wanted), Some(net)) =
                (address.kind.is_tailscale(), address.tailnet_name(), active)
            {
                if !net.is(wanted) {
                    skipped.push(wanted.to_owned());
                    continue;
                }
            }
            candidates.push((index, address));
        }
        let only_one = candidates.len() == 1 && skipped.is_empty();
        let mut refused: Option<Choice> = None;
        for (index, address) in candidates {
            let Some(route) = self.route_for(m, address, all, depth) else {
                continue;
            };
            let choice = Choice {
                index,
                address: address.clone(),
                route,
            };
            if only_one {
                return Pick::Reached(choice);
            }
            match probe(live, &choice.route) {
                Probed::Up => return Pick::Reached(choice),
                Probed::Reached => {
                    refused.get_or_insert(choice);
                }
                Probed::Down => {}
            }
        }
        if let Some(choice) = refused {
            return Pick::Reached(choice);
        }
        match (skipped.first(), active) {
            (Some(wanted), Some(net)) => Pick::WrongNetwork {
                wanted: wanted.clone(),
                active: net.tailnet.clone(),
            },
            _ => Pick::Unreachable,
        }
    }

    fn route_for(
        &self,
        m: &Machine,
        address: &MachineAddress,
        all: &[Machine],
        depth: usize,
    ) -> Option<Route> {
        let mut route = Route {
            host: address.host.clone(),
            user: m.address_user(address).to_owned(),
            port: address.port,
            jump: None,
        };
        if address.kind == AddressKind::Jump {
            if depth >= JUMP_DEPTH {
                return None;
            }
            let via = all.iter().find(|x| Some(x.name.as_str()) == address.via.as_deref())?;
            let Pick::Reached(inner) = self.pick_in(via, all, depth + 1) else {
                return None;
            };
            route.jump = Some(Box::new(inner.route));
        }
        Some(route)
    }
}

fn ttl(pick: &Pick) -> f64 {
    match pick {
        Pick::Reached(_) => KEEP_SECS,
        _ => KEEP_FAILED_SECS,
    }
}

fn first_of(m: &Machine, list: &[MachineAddress]) -> Pick {
    Pick::Reached(Choice {
        index: 0,
        address: list[0].clone(),
        route: Route::direct(&m.host, &m.user),
    })
}

pub fn probe_argv(route: &Route) -> Vec<String> {
    let mut argv = ssh_prefix(route, PROBE_CONNECT_SECS);
    argv.push("true".to_owned());
    argv
}

fn probe(live: &Live, route: &Route) -> Probed {
    classify(&live.runner.run(&Call::secs(probe_argv(route), PROBE_SECS)))
}
