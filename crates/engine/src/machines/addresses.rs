use super::forms::{is_host, is_user};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AddressKind {
    #[serde(rename = "tailscale_name")]
    TailscaleName,
    #[serde(rename = "tailscale_ip")]
    TailscaleIp,
    #[serde(rename = "lan")]
    Lan,
    #[serde(rename = "public")]
    Public,
    #[serde(rename = "jump")]
    Jump,
}

impl AddressKind {
    pub const ALL: [AddressKind; 5] = [
        AddressKind::TailscaleName,
        AddressKind::TailscaleIp,
        AddressKind::Lan,
        AddressKind::Public,
        AddressKind::Jump,
    ];

    pub fn key(self) -> &'static str {
        match self {
            AddressKind::TailscaleName => "tailscale_name",
            AddressKind::TailscaleIp => "tailscale_ip",
            AddressKind::Lan => "lan",
            AddressKind::Public => "public",
            AddressKind::Jump => "jump",
        }
    }

    pub fn is_tailscale(self) -> bool {
        matches!(self, AddressKind::TailscaleName | AddressKind::TailscaleIp)
    }

    pub fn next(self) -> AddressKind {
        let at = Self::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Self::ALL[(at + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineAddress {
    pub kind: AddressKind,
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailnet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
}

impl MachineAddress {
    pub fn new(kind: AddressKind, host: &str) -> MachineAddress {
        MachineAddress {
            kind,
            host: host.into(),
            user: None,
            port: None,
            tailnet: None,
            via: None,
        }
    }

    pub fn on_tailnet(mut self, tailnet: &str) -> MachineAddress {
        self.tailnet = Some(tailnet.into());
        self
    }

    pub fn with_user(mut self, user: &str) -> MachineAddress {
        self.user = Some(user.into());
        self
    }

    pub fn with_port(mut self, port: u16) -> MachineAddress {
        self.port = Some(port);
        self
    }

    pub fn through(mut self, machine: &str) -> MachineAddress {
        self.via = Some(machine.into());
        self
    }

    pub fn legacy(host: &str) -> MachineAddress {
        MachineAddress::new(infer_kind(host), host)
    }

    pub fn tailnet_name(&self) -> Option<&str> {
        self.tailnet
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
    }

    pub fn normalized(mut self) -> MachineAddress {
        self.host = self.host.trim().to_owned();
        self.user = self
            .user
            .map(|u| u.trim().to_owned())
            .filter(|u| !u.is_empty());
        self.tailnet = self
            .tailnet
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty() && self.kind.is_tailscale());
        self.via = self
            .via
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty() && self.kind == AddressKind::Jump);
        self
    }
}

pub fn infer_kind(host: &str) -> AddressKind {
    if let Ok(v4) = host.parse::<Ipv4Addr>() {
        let o = v4.octets();
        if o[0] == 100 && (64..128).contains(&o[1]) {
            return AddressKind::TailscaleIp;
        }
        let private = o[0] == 10
            || (o[0] == 172 && (16..32).contains(&o[1]))
            || (o[0] == 192 && o[1] == 168)
            || (o[0] == 169 && o[1] == 254);
        return if private {
            AddressKind::Lan
        } else {
            AddressKind::Public
        };
    }
    let lower = host.to_ascii_lowercase();
    if lower.ends_with(".ts.net") || !lower.contains('.') {
        return AddressKind::TailscaleName;
    }
    if lower.ends_with(".local") || lower.ends_with(".lan") || lower.ends_with(".home.arpa") {
        return AddressKind::Lan;
    }
    AddressKind::Public
}

pub fn is_tailnet(s: &str) -> bool {
    let Some(first) = s.chars().next() else {
        return false;
    };
    s.len() <= 253
        && first.is_ascii_alphanumeric()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "@._+-".contains(c))
}

pub fn address_problems(address: &MachineAddress, machine: &str) -> Vec<String> {
    let mut out = Vec::new();
    if !is_host(&address.host) {
        out.push(HOST.to_owned());
    }
    if let Some(user) = &address.user {
        if !is_user(user) {
            out.push(USER.to_owned());
        }
    }
    if address.port == Some(0) {
        out.push(PORT.to_owned());
    }
    if let Some(tailnet) = &address.tailnet {
        if !address.kind.is_tailscale() {
            out.push(TAILNET_KIND.to_owned());
        } else if !is_tailnet(tailnet) {
            out.push(TAILNET.to_owned());
        }
    }
    match (&address.kind, address.via.as_deref()) {
        (AddressKind::Jump, None | Some("")) => out.push(VIA_MISSING.to_owned()),
        (AddressKind::Jump, Some(via)) if via == machine => out.push(VIA_SELF.to_owned()),
        (AddressKind::Jump, Some(_)) => {}
        (_, Some(_)) => out.push(VIA_KIND.to_owned()),
        _ => {}
    }
    out
}

pub fn list_problems(list: &[MachineAddress], machine: &str) -> Vec<String> {
    let mut out = Vec::new();
    if list.is_empty() {
        out.push(NONE.to_owned());
    }
    for address in list {
        for why in address_problems(address, machine) {
            if !out.contains(&why) {
                out.push(why);
            }
        }
    }
    out
}

pub const NONE: &str = "Give the machine at least one address";
pub const HOST: &str = "Address host: a name or an IP address";
pub const USER: &str = "Address user: lowercase letters, digits, hyphens and underscores";
pub const PORT: &str = "Port: a number from 1 to 65535";
pub const TAILNET: &str = "Network: the name `tailscale switch --list` shows";
pub const TAILNET_KIND: &str = "Only a Tailscale address has a network";
pub const VIA_MISSING: &str = "A jump address names the machine to go through";
pub const VIA_SELF: &str = "A machine can't jump through itself";
pub const VIA_KIND: &str = "Only a jump address goes through a machine";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub host: String,
    pub user: String,
    pub port: Option<u16>,
    pub jump: Option<Box<Route>>,
    pub key: Option<String>,
}

impl Route {
    pub fn direct(host: &str, user: &str) -> Route {
        Route {
            host: host.into(),
            user: user.into(),
            port: None,
            jump: None,
            key: None,
        }
    }

    pub fn with_key(mut self, key: Option<&str>) -> Route {
        self.key = key
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(str::to_owned);
        self
    }

    pub fn as_user(&self, user: &str) -> Route {
        Route {
            user: user.into(),
            ..self.clone()
        }
    }

    pub fn login(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }

    fn hop(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        match self.port {
            Some(port) => format!("{}@{host}:{port}", self.user),
            None => format!("{}@{host}", self.user),
        }
    }

    pub fn jump_chain(&self) -> Option<String> {
        let jump = self.jump.as_deref()?;
        let mut hops = Vec::new();
        let mut at = Some(jump);
        while let Some(route) = at {
            hops.push(route.hop());
            at = route.jump.as_deref();
        }
        hops.reverse();
        Some(hops.join(","))
    }

    pub fn ssh_options(&self) -> Vec<String> {
        self.options("-p")
    }

    pub fn scp_options(&self) -> Vec<String> {
        self.options("-P")
    }

    fn options(&self, port_flag: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(port) = self.port {
            out.push(port_flag.to_owned());
            out.push(port.to_string());
        }
        out.extend(self.identity_options());
        out.extend(self.jump_options());
        out
    }

    pub fn identity_options(&self) -> Vec<String> {
        match &self.key {
            Some(key) => vec![
                "-i".to_owned(),
                key.clone(),
                "-o".to_owned(),
                "IdentitiesOnly=yes".to_owned(),
            ],
            None => Vec::new(),
        }
    }

    fn chain_has_key(&self) -> bool {
        self.key.is_some() || self.jump.as_deref().is_some_and(Route::chain_has_key)
    }

    pub fn jump_options(&self) -> Vec<String> {
        let Some(jump) = self.jump.as_deref() else {
            return Vec::new();
        };
        if jump.chain_has_key() {
            return vec![
                "-o".to_owned(),
                format!("ProxyCommand={}", jump.proxy_command()),
            ];
        }
        match self.jump_chain() {
            Some(chain) => vec!["-J".to_owned(), chain],
            None => Vec::new(),
        }
    }

    fn proxy_command(&self) -> String {
        let mut head: Vec<String> = [
            "/usr/bin/ssh",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=8",
        ]
        .iter()
        .map(|w| (*w).to_owned())
        .collect();
        head.extend(self.options("-p"));
        let literal = |w: &String| crate::lanes::quote(&w.replace('%', "%%"));
        let mut words: Vec<String> = head.iter().map(literal).collect();
        words.push(crate::lanes::quote("-W"));
        words.push(crate::lanes::quote("[%h]:%p"));
        words.push(literal(&"--".to_owned()));
        words.push(literal(&self.login()));
        words.join(" ")
    }

    pub fn scp_host(&self) -> String {
        if self.host.contains(':') {
            format!("{}@[{}]", self.user, self.host)
        } else {
            format!("{}@{}", self.user, self.host)
        }
    }

    pub fn git_url(&self, path: &str) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        match self.port {
            Some(port) => format!("ssh://{}@{host}:{port}{path}", self.user),
            None => format!("ssh://{}@{host}{path}", self.user),
        }
    }
}
