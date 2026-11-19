use std::collections::{BTreeMap, VecDeque};

use notchling_engine::machines::forms::is_user;
use notchling_engine::machines::registry::Machine;
use notchling_engine::machines::ssh_key::{shape_ok, KeyIssue, KeyStatus};
use notchling_engine::machines::ssh_test::{SshTest, TEST_PARALLEL};
use notchling_engine::machines::tailscale::{
    available, import, other_networks, Network, Peer, PeerFilter, Picked, Status,
};
use notchling_engine::organizations::PERSONAL_ID;
use notchling_locale as t;
use serde_json::Value;

use crate::organizations::model::OrganizationsModel;

use super::add::commit;
use super::add_parts::AddParts;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Read {
    Reading,
    Failed(String),
    Ready,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestState {
    Running(u64),
    Done(SshTest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestJob {
    pub host: String,
    pub user: String,
    pub key: Option<String>,
    pub tailnet: String,
    pub round: u64,
}

#[derive(Clone, Debug)]
pub struct FromTailscale {
    pub read: Read,
    pub status: Status,
    pub networks: Vec<Network>,
    pub filter: PeerFilter,
    pub chosen: BTreeMap<String, String>,
    pub default_user: String,
    pub existing: Vec<Machine>,
    pub error: Option<String>,
    pub done: Option<usize>,
    pub organizations: Vec<(String, String)>,
    pub organization: String,
    pub keys: BTreeMap<String, String>,
    pub key_status: BTreeMap<String, KeyStatus>,
    pub tests: BTreeMap<String, TestState>,
    pub waiting: VecDeque<String>,
    pub lanes: usize,
    round: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerRow {
    pub host: String,
    pub title: String,
    pub detail: String,
    pub owner: String,
    pub online: bool,
    pub chosen: bool,
    pub user: String,
    pub key: Option<String>,
    pub key_status: Option<KeyStatus>,
    pub key_issue: Option<KeyIssue>,
    pub test: Option<TestState>,
}

impl FromTailscale {
    pub fn new(default_user: &str, existing: Vec<Machine>) -> FromTailscale {
        FromTailscale {
            read: Read::Reading,
            status: Status {
                tailnet: String::new(),
                running: false,
                peers: Vec::new(),
            },
            networks: Vec::new(),
            filter: PeerFilter::default(),
            chosen: BTreeMap::new(),
            default_user: default_user.to_owned(),
            existing,
            error: None,
            done: None,
            organizations: Vec::new(),
            organization: PERSONAL_ID.to_owned(),
            keys: BTreeMap::new(),
            key_status: BTreeMap::new(),
            tests: BTreeMap::new(),
            waiting: VecDeque::new(),
            lanes: 0,
            round: 0,
        }
    }

    pub fn set_organizations(&mut self, reply: Result<Value, String>) {
        let mut model = OrganizationsModel::default();
        model.finish(reply);
        self.organizations = model
            .rows
            .into_iter()
            .map(|row| (row.id, row.name))
            .collect();
        if !self
            .organizations
            .iter()
            .any(|(id, _)| *id == self.organization)
        {
            self.organization = PERSONAL_ID.to_owned();
        }
    }

    pub fn organization_name(&self) -> String {
        self.organizations
            .iter()
            .find(|(id, _)| *id == self.organization)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| t::cluster::tailscale_personal().to_owned())
    }

    pub fn next_organization(&mut self) {
        let at = self
            .organizations
            .iter()
            .position(|(id, _)| *id == self.organization);
        if let Some(next) = at
            .map(|i| (i + 1) % self.organizations.len())
            .or_else(|| (!self.organizations.is_empty()).then_some(0))
        {
            self.organization = self.organizations[next].0.clone();
        }
    }

    pub fn organization_choice(&self) -> Option<String> {
        (self.organization != PERSONAL_ID).then(|| self.organization.clone())
    }

    pub fn offers_organizations(&self) -> bool {
        self.organizations.len() > 1
    }

    pub fn loaded(&mut self, status: Result<Status, String>, networks: Vec<Network>) {
        self.networks = networks;
        match status {
            Ok(status) if !status.running => {
                self.read = Read::Failed(t::cluster::tailscale_not_running().to_owned());
            }
            Ok(status) => {
                self.status = status;
                self.read = Read::Ready;
            }
            Err(why) => self.read = Read::Failed(why),
        }
    }

    pub fn network_name(&self) -> String {
        if !self.status.tailnet.is_empty() {
            return self.status.tailnet.clone();
        }
        self.networks
            .iter()
            .find(|n| n.active)
            .map(|n| n.tailnet.clone())
            .unwrap_or_default()
    }

    pub fn offered(&self) -> Vec<&Peer> {
        available(&self.status, &self.existing)
    }

    pub fn shown(&self) -> Vec<&Peer> {
        self.offered()
            .into_iter()
            .filter(|p| self.filter.matches(p))
            .collect()
    }

    pub fn hidden_count(&self) -> usize {
        self.status.peers.len().saturating_sub(self.offered().len())
    }

    pub fn systems(&self) -> Vec<String> {
        let offered: Vec<Peer> = self.offered().into_iter().cloned().collect();
        notchling_engine::machines::tailscale::systems(&offered)
    }

    pub fn rows(&self) -> Vec<PeerRow> {
        self.shown()
            .into_iter()
            .map(|p| PeerRow {
                host: p.host().to_owned(),
                title: p.short().to_owned(),
                detail: detail(p),
                owner: owner_line(p),
                online: p.online,
                chosen: self.chosen.contains_key(p.host()),
                user: self
                    .chosen
                    .get(p.host())
                    .cloned()
                    .unwrap_or_else(|| self.default_user.clone()),
                key: self.keys.get(p.host()).cloned(),
                key_status: self.key_status.get(p.host()).copied(),
                key_issue: self.key_issue_of(p.host()),
                test: self.tests.get(p.host()).cloned(),
            })
            .collect()
    }

    pub fn toggle(&mut self, host: &str) {
        if self.chosen.remove(host).is_none() {
            self.chosen
                .insert(host.to_owned(), self.default_user.clone());
        } else {
            self.forget_host(host);
        }
        self.error = None;
    }

    fn forget_host(&mut self, host: &str) {
        self.keys.remove(host);
        self.key_status.remove(host);
        self.tests.remove(host);
        self.waiting.retain(|h| h != host);
    }

    pub fn all_chosen(&self) -> bool {
        let shown = self.shown();
        !shown.is_empty() && shown.iter().all(|p| self.chosen.contains_key(p.host()))
    }

    pub fn select_all(&mut self) {
        let hosts: Vec<String> = self.shown().iter().map(|p| p.host().to_owned()).collect();
        if self.all_chosen() {
            for host in hosts {
                self.chosen.remove(&host);
                self.forget_host(&host);
            }
        } else {
            for host in hosts {
                let user = self.default_user.clone();
                self.chosen.entry(host).or_insert(user);
            }
        }
        self.error = None;
    }

    pub fn set_user(&mut self, host: &str, user: &str) {
        if let Some(slot) = self.chosen.get_mut(host) {
            let user = user.trim().to_owned();
            if *slot != user {
                *slot = user;
                self.tests.remove(host);
                self.waiting.retain(|h| h != host);
            }
        }
        self.error = None;
    }

    pub fn set_key(&mut self, host: &str, key: Option<&str>, status: Option<KeyStatus>) {
        if !self.chosen.contains_key(host) {
            return;
        }
        match key.map(str::trim).filter(|k| !k.is_empty()) {
            Some(key) => {
                self.keys.insert(host.to_owned(), key.to_owned());
                match status {
                    Some(status) => self.key_status.insert(host.to_owned(), status),
                    None => self.key_status.remove(host),
                };
            }
            None => {
                self.keys.remove(host);
                self.key_status.remove(host);
            }
        }
        self.tests.remove(host);
        self.waiting.retain(|h| h != host);
        self.error = None;
    }

    fn key_issue_of(&self, host: &str) -> Option<KeyIssue> {
        match self.keys.get(host) {
            Some(key) if !shape_ok(key) => Some(KeyIssue::BadShape),
            Some(_) => self.key_status.get(host).and_then(|s| s.issue),
            None => None,
        }
    }

    pub fn key_issue(&self) -> Option<(String, KeyIssue)> {
        self.chosen
            .keys()
            .find_map(|host| self.key_issue_of(host).map(|i| (host.clone(), i)))
    }

    pub fn request_test(&mut self, host: &str) -> bool {
        if !self.chosen.contains_key(host) || self.read != Read::Ready {
            return false;
        }
        if matches!(self.tests.get(host), Some(TestState::Running(_))) {
            return false;
        }
        let user = self.chosen.get(host).cloned().unwrap_or_default();
        if !is_user(&user) {
            return false;
        }
        if let Some((_, issue)) = self.key_issue().filter(|(h, _)| h == host) {
            self.tests
                .insert(host.to_owned(), TestState::Done(SshTest::Key(issue)));
            return false;
        }
        self.round += 1;
        self.tests
            .insert(host.to_owned(), TestState::Running(self.round));
        self.waiting.push_back(host.to_owned());
        true
    }

    pub fn request_selected(&mut self) -> usize {
        let hosts: Vec<String> = self.chosen.keys().cloned().collect();
        hosts.iter().filter(|h| self.request_test(h)).count()
    }

    pub fn can_test(&self) -> bool {
        self.read == Read::Ready && !self.chosen.is_empty()
    }

    pub fn lanes_to_start(&self) -> usize {
        TEST_PARALLEL
            .saturating_sub(self.lanes)
            .min(self.waiting.len())
    }

    pub fn next_job(&mut self) -> Option<TestJob> {
        while let Some(host) = self.waiting.pop_front() {
            let Some(TestState::Running(round)) = self.tests.get(&host).cloned() else {
                continue;
            };
            let Some(user) = self.chosen.get(&host).cloned() else {
                continue;
            };
            return Some(TestJob {
                key: self.keys.get(&host).cloned(),
                tailnet: self.network_name(),
                host,
                user,
                round,
            });
        }
        None
    }

    pub fn finish_test(&mut self, host: &str, round: u64, result: SshTest) {
        if self.tests.get(host) == Some(&TestState::Running(round)) {
            self.tests.insert(host.to_owned(), TestState::Done(result));
        }
    }

    pub fn picked(&self) -> Vec<Picked> {
        self.chosen
            .iter()
            .map(|(host, user)| Picked {
                host: host.clone(),
                user: user.clone(),
                key: self.keys.get(host).cloned(),
            })
            .collect()
    }

    pub fn can_import(&self) -> bool {
        self.read == Read::Ready
            && !self.chosen.is_empty()
            && self.chosen.values().all(|u| is_user(u))
            && self.key_issue().is_none()
    }

    pub fn other_networks_note(&self) -> Option<String> {
        let others: Vec<String> = other_networks(&self.networks)
            .into_iter()
            .map(|n| n.tailnet.clone())
            .collect();
        if others.is_empty() {
            return None;
        }
        Some(t::cluster::tailscale_other_networks(&others.join(", ")))
    }

    pub fn imported(&mut self, count: usize) {
        self.done = Some(count);
        self.chosen.clear();
        self.keys.clear();
        self.key_status.clear();
        self.tests.clear();
        self.waiting.clear();
    }
}

fn detail(peer: &Peer) -> String {
    let ips = peer.ips.join(", ");
    if peer.os.is_empty() {
        t::cluster::tailscale_peer_line_unknown(&ips)
    } else {
        t::cluster::tailscale_peer_line(&peer.os, &ips)
    }
}

fn owner_line(peer: &Peer) -> String {
    if peer.tags.is_empty() {
        if peer.owner.is_empty() {
            String::new()
        } else {
            t::cluster::tailscale_peer_owner(&peer.owner)
        }
    } else {
        t::cluster::tailscale_peer_tagged(&peer.owner, &peer.tags.join(", "))
    }
}

pub fn import_machines(
    parts: &AddParts,
    status: &Status,
    picked: &[Picked],
    organization: Option<&str>,
) -> Result<usize, String> {
    let mut count = 0;
    commit(&*parts.registry, |list| {
        let made = import(status, picked, list, organization)?;
        count = made.len();
        if made.is_empty() {
            return Ok(false);
        }
        list.extend(made);
        Ok(true)
    })?;
    Ok(count)
}
