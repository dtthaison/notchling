use std::collections::BTreeMap;

use notchling_engine::machines::forms::is_user;
use notchling_engine::machines::registry::Machine;
use notchling_engine::machines::tailscale::{
    available, import, other_networks, Network, Peer, PeerFilter, Picked, Status,
};
use notchling_locale as t;

use super::add::commit;
use super::add_parts::AddParts;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Read {
    Reading,
    Failed(String),
    Ready,
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
        }
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
            })
            .collect()
    }

    pub fn toggle(&mut self, host: &str) {
        if self.chosen.remove(host).is_none() {
            self.chosen
                .insert(host.to_owned(), self.default_user.clone());
        }
        self.error = None;
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
            *slot = user.trim().to_owned();
        }
        self.error = None;
    }

    pub fn picked(&self) -> Vec<Picked> {
        self.chosen
            .iter()
            .map(|(host, user)| Picked {
                host: host.clone(),
                user: user.clone(),
            })
            .collect()
    }

    pub fn can_import(&self) -> bool {
        self.read == Read::Ready
            && !self.chosen.is_empty()
            && self.chosen.values().all(|u| is_user(u))
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
) -> Result<usize, String> {
    let mut count = 0;
    commit(&*parts.registry, |list| {
        let made = import(status, picked, list, None)?;
        count = made.len();
        if made.is_empty() {
            return Ok(false);
        }
        list.extend(made);
        Ok(true)
    })?;
    Ok(count)
}
