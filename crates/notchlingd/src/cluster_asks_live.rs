use crate::cluster_asks::Admin;
use crate::cluster_node::ClusterNode;
use notchling_engine::machines::node_id_of;
use notchling_engine::machines::reach::Router;
use notchling_engine::machines::registry::{Machine, RegistryFiles, RegistryState};
use notchling_engine::machines::runner::{Call, CommandResult, Runner};
use notchling_engine::machines::tailscale::{default_cli, Tailscale};
use notchling_engine::organizations::{Kind, Organizations};
use serde_json::{json, Value};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub const PROBE_CAP: Duration = Duration::from_secs(5);
const MIN_LEFT_SECS: f64 = 0.05;

pub type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

struct Budget {
    inner: Arc<dyn Runner>,
    clock: Clock,
    until: f64,
}

impl Runner for Budget {
    fn run(&self, call: &Call) -> CommandResult {
        let left = self.until - (self.clock)();
        if left < MIN_LEFT_SECS {
            return CommandResult::new(255, "", "out of time");
        }
        let mut short = call.clone();
        short.timeout = short.timeout.min(Duration::from_secs_f64(left));
        self.inner.run(&short)
    }
}

fn mono_secs() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

pub struct LiveAdmin {
    node: Result<Arc<ClusterNode>, String>,
    registry: RegistryFiles,
    orgs: Organizations,
    runner: Arc<dyn Runner>,
    clock: Clock,
}

impl LiveAdmin {
    pub fn new(
        node: Result<Arc<ClusterNode>, String>,
        registry: RegistryFiles,
        orgs: Organizations,
        runner: Arc<dyn Runner>,
    ) -> LiveAdmin {
        LiveAdmin {
            node,
            registry,
            orgs,
            runner,
            clock: Arc::new(mono_secs),
        }
    }

    pub fn with_clock(mut self, clock: Clock) -> LiveAdmin {
        self.clock = clock;
        self
    }

    fn all(&self) -> Vec<Machine> {
        match self.registry.load() {
            RegistryState::Loaded { machines, .. } => machines,
            _ => Vec::new(),
        }
    }

    fn only_in<'a>(machines: &'a [Machine], name: &str) -> Option<&'a Machine> {
        let mut found = machines
            .iter()
            .filter(|m| m.name.eq_ignore_ascii_case(name));
        match (found.next(), found.next()) {
            (Some(one), None) => Some(one),
            _ => None,
        }
    }

    fn only<T>(&self, name: &str, read: impl Fn(&Machine) -> T) -> Option<T> {
        Self::only_in(&self.all(), name).map(read)
    }
}

impl Admin for LiveAdmin {
    fn status(&self) -> Value {
        match &self.node {
            Ok(node) => node.status(),
            Err(_) => json!({"cluster": Value::Null, "joining": Value::Null, "members": []}),
        }
    }

    fn own_organization(&self) -> Option<String> {
        let view = self.orgs.view();
        (!view.is_closed()).then(|| view.machine(None).id)
    }

    fn organization_of(&self, machine: &str) -> Option<String> {
        self.only(machine, |m| m.organization_id().to_string())
    }

    fn host_of(&self, machine: &str) -> Option<String> {
        self.only(machine, |m| m.host.clone())
    }

    fn matches(&self, machine: &str) -> usize {
        let name = machine;
        self.all()
            .iter()
            .filter(|m| m.name.eq_ignore_ascii_case(name))
            .count()
    }

    fn node_of(&self, machine: &str) -> Option<String> {
        let machines = self.all();
        let found = Self::only_in(&machines, machine)?;
        let budget: Arc<dyn Runner> = Arc::new(Budget {
            inner: self.runner.clone(),
            clock: self.clock.clone(),
            until: (self.clock)() + PROBE_CAP.as_secs_f64(),
        });
        let router = Router::new(
            budget.clone(),
            Tailscale::new(budget.clone(), &default_cli()),
            self.clock.clone(),
        );
        let routed = router.reached(found, &machines);
        if routed.has_addresses() && (routed.route.is_none() || routed.refused.is_some()) {
            return None;
        }
        node_id_of(budget.as_ref(), &routed)
    }

    fn secret_organization(&self, name: &str) -> Option<String> {
        let wanted = Kind::Vault.normal(name);
        self.orgs
            .listing(Kind::Vault)?
            .iter()
            .any(|(key, _)| *key == wanted)
            .then(|| self.orgs.resolve(Kind::Vault, name).id)
    }
}

#[cfg(test)]
#[path = "cluster_asks_live_tests.rs"]
mod tests;
