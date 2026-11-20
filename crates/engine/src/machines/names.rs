use super::addresses::{AddressKind, MachineAddress};

const SUFFIX: &str = ".ts.net";

pub fn short_host(host: &str) -> &str {
    if !host.to_ascii_lowercase().ends_with(SUFFIX) {
        return host;
    }
    match host.split('.').next() {
        Some(first) if !first.is_empty() && first.len() < host.len() => first,
        _ => host,
    }
}

pub fn connect_hosts(address: &MachineAddress) -> Vec<String> {
    let short = short_host(&address.host);
    if address.kind == AddressKind::TailscaleName && short != address.host {
        vec![short.to_owned(), address.host.clone()]
    } else {
        vec![address.host.clone()]
    }
}
