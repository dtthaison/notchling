use notchling_engine::machines::addresses::{
    infer_kind, list_problems, AddressKind, MachineAddress,
};
use notchling_engine::machines::forms::{is_host, is_user};
use notchling_locale as t;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RowMeta {
    pub kind: Option<AddressKind>,
    pub tailnet: Option<String>,
    pub via: Option<String>,
}

impl RowMeta {
    pub fn is_plain(&self) -> bool {
        self.kind.is_none() && self.tailnet.is_none() && self.via.is_none()
    }

    pub fn kind_for(&self, text: &str) -> AddressKind {
        self.kind
            .unwrap_or_else(|| infer_kind(&split_row(text).map(|r| r.host).unwrap_or_default()))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RowDraft {
    pub text: String,
    pub meta: RowMeta,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowText {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
}

pub fn split_row(text: &str) -> Result<RowText, String> {
    if text.chars().any(char::is_control) {
        return Err(t::cluster::address_row_control().into());
    }
    let typed = text.trim();
    if typed.chars().any(char::is_whitespace) {
        return Err(t::cluster::address_row_spaces().into());
    }
    if typed.is_empty() {
        return Err(t::cluster::address_row_empty().into());
    }
    if typed.to_ascii_lowercase().starts_with("ssh://") || typed.contains('/') {
        return Err(t::cluster::address_row_shape().into());
    }
    let (user, rest) = match typed.split_once('@') {
        Some((user, rest)) => {
            if user.is_empty() || rest.is_empty() || rest.contains('@') {
                return Err(t::cluster::address_row_shape().into());
            }
            (Some(user.to_owned()), rest)
        }
        None => (None, typed),
    };
    let (host, port) = split_port(rest)?;
    if host.is_empty() {
        return Err(t::cluster::address_row_shape().into());
    }
    Ok(RowText { user, host, port })
}

fn split_port(rest: &str) -> Result<(String, Option<u16>), String> {
    if let Some(inner) = rest.strip_prefix('[') {
        let Some((host, after)) = inner.split_once(']') else {
            return Err(t::cluster::address_row_bracket().into());
        };
        return match after {
            "" => Ok((host.to_owned(), None)),
            _ => match after.strip_prefix(':') {
                Some(port) => Ok((host.to_owned(), Some(port_of(port)?))),
                None => Err(t::cluster::address_row_shape().into()),
            },
        };
    }
    match rest.matches(':').count() {
        0 => Ok((rest.to_owned(), None)),
        1 => {
            let (host, port) = rest.split_once(':').unwrap_or((rest, ""));
            Ok((host.to_owned(), Some(port_of(port)?)))
        }
        _ => Ok((rest.to_owned(), None)),
    }
}

fn port_of(text: &str) -> Result<u16, String> {
    text.parse::<u16>()
        .ok()
        .filter(|p| *p > 0)
        .ok_or_else(|| t::cluster::address_row_port().into())
}

pub fn address_of(row: &RowDraft, machine_user: &str) -> Result<MachineAddress, String> {
    let text = split_row(&row.text)?;
    if !is_host(&text.host) {
        return Err(t::cluster::address_row_host().into());
    }
    if let Some(user) = &text.user {
        if !is_user(user) {
            return Err(t::cluster::address_row_user().into());
        }
    }
    let kind = row.meta.kind.unwrap_or_else(|| infer_kind(&text.host));
    let user = text.user.filter(|u| u != machine_user);
    Ok(MachineAddress {
        kind,
        host: text.host,
        user,
        port: text.port,
        tailnet: row.meta.tailnet.clone(),
        via: row.meta.via.clone(),
    }
    .normalized())
}

pub fn addresses_of(
    rows: &[RowDraft],
    machine: &str,
    machine_user: &str,
) -> Result<Vec<MachineAddress>, String> {
    let mut out = Vec::new();
    for (at, row) in rows.iter().enumerate() {
        out.push(address_of(row, machine_user).map_err(|why| {
            if rows.len() > 1 {
                t::cluster::address_row_n(at + 1, &why)
            } else {
                why
            }
        })?);
    }
    if let Some(first) = list_problems(&out, machine).into_iter().next() {
        return Err(first);
    }
    Ok(out)
}

pub fn text_of(address: &MachineAddress, machine_user: &str) -> String {
    let body = match address.port {
        Some(port) if address.host.contains(':') => format!("[{}]:{port}", address.host),
        Some(port) => format!("{}:{port}", address.host),
        None => address.host.clone(),
    };
    match address.user.as_deref().filter(|u| *u != machine_user) {
        Some(user) => format!("{user}@{body}"),
        None => body,
    }
}

pub fn drafts_of(list: &[MachineAddress], machine_user: &str) -> Vec<RowDraft> {
    list.iter()
        .map(|a| RowDraft {
            text: text_of(a, machine_user),
            meta: RowMeta {
                kind: Some(a.kind),
                tailnet: a.tailnet.clone(),
                via: a.via.clone(),
            },
        })
        .collect()
}

pub fn kind_title(kind: AddressKind) -> &'static str {
    match kind {
        AddressKind::TailscaleName => t::cluster::address_kind_tailscale_name(),
        AddressKind::TailscaleIp => t::cluster::address_kind_tailscale_ip(),
        AddressKind::Lan => t::cluster::address_kind_lan(),
        AddressKind::Public => t::cluster::address_kind_public(),
        AddressKind::Jump => t::cluster::address_kind_jump(),
    }
}

pub fn cycle(options: &[Option<String>], current: &Option<String>) -> Option<String> {
    if options.is_empty() {
        return None;
    }
    let at = options.iter().position(|o| o == current);
    match at {
        Some(i) => options[(i + 1) % options.len()].clone(),
        None => options[0].clone(),
    }
}

pub fn swap_rows(rows: &mut [RowDraft], a: usize, b: usize) -> bool {
    if a >= rows.len() || b >= rows.len() || a == b {
        return false;
    }
    rows.swap(a, b);
    true
}
