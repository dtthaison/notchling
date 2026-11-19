use super::addresses::{AddressKind, MachineAddress};
use super::forms::name_problem;
use super::registry::{Machine, MachineOs};
use super::runner::{Call, Runner};
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const APP_CLI: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";
pub const LISTING_SECS: u64 = 10;
pub const SWITCH_SECS: u64 = 30;

pub fn default_cli() -> String {
    if std::path::Path::new(APP_CLI).is_file() {
        APP_CLI.to_owned()
    } else {
        "tailscale".to_owned()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Network {
    pub id: String,
    pub tailnet: String,
    pub account: String,
    pub active: bool,
}

impl Network {
    pub fn is(&self, tailnet: &str) -> bool {
        let wanted = tailnet.trim();
        self.tailnet.eq_ignore_ascii_case(wanted) || self.account.eq_ignore_ascii_case(wanted)
    }
}

pub fn parse_networks(text: &str) -> Vec<Network> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let id = words.next()?;
            if id == "ID" {
                return None;
            }
            let tailnet = words.next()?;
            let account = words.next()?;
            let active = account.ends_with('*');
            Some(Network {
                id: id.to_owned(),
                tailnet: tailnet.to_owned(),
                account: account.trim_end_matches('*').to_owned(),
                active,
            })
        })
        .collect()
}

pub fn is_network_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

pub fn switch_argv(cli: &str, id: &str) -> Option<Vec<String>> {
    is_network_id(id).then(|| vec![cli.to_owned(), "switch".to_owned(), id.to_owned()])
}

pub fn list_argv(cli: &str) -> Vec<String> {
    vec![cli.to_owned(), "switch".to_owned(), "--list".to_owned()]
}

pub fn status_argv(cli: &str) -> Vec<String> {
    vec![cli.to_owned(), "status".to_owned(), "--json".to_owned()]
}

#[derive(Clone)]
pub struct Tailscale {
    runner: Arc<dyn Runner>,
    cli: String,
}

impl Tailscale {
    pub fn new(runner: Arc<dyn Runner>, cli: &str) -> Tailscale {
        Tailscale {
            runner,
            cli: cli.to_owned(),
        }
    }

    pub fn cli(&self) -> &str {
        &self.cli
    }

    pub fn networks(&self) -> Result<Vec<Network>, String> {
        let result = self
            .runner
            .run(&Call::secs(list_argv(&self.cli), LISTING_SECS));
        if result.code != 0 {
            return Err(failure(&result.err, "tailscale switch --list"));
        }
        Ok(parse_networks(&result.out))
    }

    pub fn active(&self) -> Option<Network> {
        self.networks().ok()?.into_iter().find(|n| n.active)
    }

    pub fn status(&self) -> Result<Status, String> {
        let result = self
            .runner
            .run(&Call::secs(status_argv(&self.cli), LISTING_SECS));
        if result.code != 0 {
            return Err(failure(&result.err, "tailscale status"));
        }
        parse_status(&result.out)
    }

    pub fn switch(&self, id: &str) -> Result<(), String> {
        let argv =
            switch_argv(&self.cli, id).ok_or_else(|| "That isn't a network id".to_owned())?;
        let result = self.runner.run(&Call::secs(argv, SWITCH_SECS));
        if result.code == 0 {
            Ok(())
        } else {
            Err(failure(&result.err, "tailscale switch"))
        }
    }
}

fn failure(err: &str, what: &str) -> String {
    let line = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    if line.is_empty() {
        format!("{what} failed")
    } else {
        format!("{what}: {}", line.trim())
    }
}

fn nullable<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Deserialize)]
struct RawTailnet {
    #[serde(rename = "Name", default, deserialize_with = "nullable")]
    name: String,
}

#[derive(Deserialize)]
struct RawPeer {
    #[serde(rename = "HostName", default, deserialize_with = "nullable")]
    host_name: String,
    #[serde(rename = "DNSName", default, deserialize_with = "nullable")]
    dns_name: String,
    #[serde(rename = "OS", default, deserialize_with = "nullable")]
    os: String,
    #[serde(rename = "UserID", default, deserialize_with = "nullable")]
    user_id: i64,
    #[serde(rename = "TailscaleIPs", default, deserialize_with = "nullable")]
    ips: Vec<String>,
    #[serde(rename = "Online", default, deserialize_with = "nullable")]
    online: bool,
    #[serde(rename = "Tags", default, deserialize_with = "nullable")]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct RawUser {
    #[serde(rename = "LoginName", default, deserialize_with = "nullable")]
    login: String,
}

#[derive(Deserialize)]
struct RawStatus {
    #[serde(rename = "BackendState", default, deserialize_with = "nullable")]
    backend: String,
    #[serde(rename = "CurrentTailnet", default)]
    current: Option<RawTailnet>,
    #[serde(rename = "Peer", default, deserialize_with = "nullable")]
    peers: BTreeMap<String, RawPeer>,
    #[serde(rename = "User", default, deserialize_with = "nullable")]
    users: BTreeMap<String, RawUser>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub name: String,
    pub dns: String,
    pub os: String,
    pub ips: Vec<String>,
    pub online: bool,
    pub tags: Vec<String>,
    pub owner: String,
}

impl Peer {
    pub fn ipv4(&self) -> Option<&str> {
        self.ips
            .iter()
            .find(|ip| ip.parse::<std::net::Ipv4Addr>().is_ok())
            .map(String::as_str)
    }

    pub fn short(&self) -> &str {
        let first = self.dns.split('.').next().unwrap_or("");
        if first.is_empty() {
            &self.name
        } else {
            first
        }
    }

    pub fn host(&self) -> &str {
        if self.dns.is_empty() {
            self.ipv4().unwrap_or(&self.name)
        } else {
            &self.dns
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub tailnet: String,
    pub running: bool,
    pub peers: Vec<Peer>,
}

pub fn parse_status(text: &str) -> Result<Status, String> {
    let raw: RawStatus =
        serde_json::from_str(text).map_err(|e| format!("tailscale status can't be read: {e}"))?;
    let mut peers: Vec<Peer> = raw
        .peers
        .into_values()
        .map(|p| Peer {
            owner: raw
                .users
                .get(&p.user_id.to_string())
                .map(|u| u.login.clone())
                .unwrap_or_default(),
            name: p.host_name,
            dns: p.dns_name.trim_end_matches('.').to_owned(),
            os: p.os,
            ips: p.ips,
            online: p.online,
            tags: p.tags,
        })
        .collect();
    peers.sort_by(|a, b| a.short().cmp(b.short()).then(a.dns.cmp(&b.dns)));
    Ok(Status {
        tailnet: raw.current.map(|c| c.name).unwrap_or_default(),
        running: raw.backend == "Running",
        peers,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeerFilter {
    pub text: String,
    pub os: Option<String>,
    pub online_only: bool,
}

impl PeerFilter {
    pub fn matches(&self, peer: &Peer) -> bool {
        if self.online_only && !peer.online {
            return false;
        }
        if let Some(os) = &self.os {
            if !peer.os.eq_ignore_ascii_case(os) {
                return false;
            }
        }
        let typed = self.text.trim().to_ascii_lowercase();
        if typed.is_empty() {
            return true;
        }
        let mut hay = vec![
            peer.name.to_ascii_lowercase(),
            peer.dns.to_ascii_lowercase(),
            peer.owner.to_ascii_lowercase(),
        ];
        hay.extend(peer.ips.iter().cloned());
        hay.extend(peer.tags.iter().map(|t| t.to_ascii_lowercase()));
        hay.iter().any(|h| h.contains(&typed))
    }
}

pub fn systems(peers: &[Peer]) -> Vec<String> {
    let mut list: Vec<String> = peers
        .iter()
        .map(|p| p.os.clone())
        .filter(|os| !os.is_empty())
        .collect();
    list.sort_by_key(|os| os.to_ascii_lowercase());
    list.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    list
}

fn same_host(a: &str, b: &str) -> bool {
    a.trim_end_matches('.')
        .eq_ignore_ascii_case(b.trim_end_matches('.'))
}

pub fn already_added(peer: &Peer, machines: &[Machine]) -> bool {
    machines.iter().any(|m| {
        let mut hosts: Vec<&str> = vec![m.host.as_str()];
        hosts.extend(m.addresses.iter().map(|a| a.host.as_str()));
        hosts.iter().any(|host| {
            same_host(host, &peer.dns)
                || peer.ips.iter().any(|ip| same_host(host, ip))
                || (!host.contains('.')
                    && !peer.short().is_empty()
                    && host.eq_ignore_ascii_case(peer.short()))
        })
    })
}

pub fn available<'a>(status: &'a Status, machines: &[Machine]) -> Vec<&'a Peer> {
    status
        .peers
        .iter()
        .filter(|p| !already_added(p, machines))
        .collect()
}

fn base_name(peer: &Peer) -> String {
    let mapped: String = peer
        .short()
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed: String = mapped.trim_matches('-').chars().take(36).collect();
    let trimmed = trimmed.trim_matches('-').to_owned();
    if trimmed.is_empty() {
        "machine".to_owned()
    } else {
        trimmed
    }
}

fn free_name(base: &str, taken: &[String]) -> String {
    let mut at = 1;
    loop {
        let name = if at == 1 {
            base.to_owned()
        } else {
            format!("{base}-{at}")
        };
        if name_problem(&name).is_none() && !taken.contains(&name) {
            return name;
        }
        at += 1;
    }
}

pub fn machine_for(
    peer: &Peer,
    user: &str,
    tailnet: &str,
    taken: &[String],
    organization: Option<&str>,
) -> Machine {
    let name = free_name(&base_name(peer), taken);
    let mut machine = Machine::new(&name, peer.host(), user);
    let mut addresses = Vec::new();
    if !peer.dns.is_empty() {
        addresses.push(tailscale(AddressKind::TailscaleName, &peer.dns, tailnet));
    }
    if let Some(ip) = peer.ipv4() {
        addresses.push(tailscale(AddressKind::TailscaleIp, ip, tailnet));
    }
    if addresses.is_empty() {
        addresses.push(tailscale(AddressKind::TailscaleName, peer.host(), tailnet));
    }
    machine.set_addresses(addresses);
    machine.os = match peer.os.as_str() {
        "macOS" => Some(MachineOs::MacOs),
        "linux" => Some(MachineOs::Linux),
        _ => None,
    };
    machine.organization = organization
        .map(str::trim)
        .filter(|o| !o.is_empty())
        .map(str::to_owned);
    machine
}

fn tailscale(kind: AddressKind, host: &str, tailnet: &str) -> MachineAddress {
    let address = MachineAddress::new(kind, host);
    if tailnet.is_empty() {
        address
    } else {
        address.on_tailnet(tailnet)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picked {
    pub host: String,
    pub user: String,
}

pub fn import(
    status: &Status,
    picked: &[Picked],
    existing: &[Machine],
    organization: Option<&str>,
) -> Result<Vec<Machine>, String> {
    let mut taken: Vec<String> = existing.iter().map(|m| m.name.clone()).collect();
    let mut out = Vec::new();
    for pick in picked {
        let Some(peer) = status.peers.iter().find(|p| p.host() == pick.host) else {
            return Err(format!("{} is no longer on this network", pick.host));
        };
        if already_added(peer, existing) {
            continue;
        }
        if !super::forms::is_user(&pick.user) {
            return Err(format!("{}: {}", peer.short(), super::forms::USER_PROBLEM));
        }
        let machine = machine_for(peer, &pick.user, &status.tailnet, &taken, organization);
        taken.push(machine.name.clone());
        out.push(machine);
    }
    Ok(out)
}

pub fn other_networks(networks: &[Network]) -> Vec<&Network> {
    networks.iter().filter(|n| !n.active).collect()
}
