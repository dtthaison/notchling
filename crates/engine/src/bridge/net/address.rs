//! `AddressFilter` and `LocalAddresses`.
//!
//! Who may even open a connection to the LAN port: this computer, the private LAN, link-local and
//! Tailscale. Everything else is closed on sight. And the addresses the pairing QR offers.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Whether a peer at `address` (text, with an optional `%zone` and `[...]`) may connect.
pub fn is_allowed(address: &str) -> bool {
    let mut text = address;
    if let Some(zone) = text.find('%') {
        text = &text[..zone];
    }
    if let Some(inner) = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        text = inner;
    }
    if let Ok(v4) = text.parse::<Ipv4Addr>() {
        return allowed_v4(v4);
    }
    match text.parse::<Ipv6Addr>() {
        Ok(v6) => allowed_v6(v6),
        Err(_) => false,
    }
}

/// [`is_allowed`] for an address already parsed.
pub fn is_allowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => allowed_v4(v4),
        IpAddr::V6(v6) => allowed_v6(v6),
    }
}

fn allowed_v4(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    a == 127 // loopback
        || a == 10 // 10/8
        || (a == 172 && (16..=31).contains(&b)) // 172.16/12
        || (a == 192 && b == 168) // 192.168/16
        || (a == 169 && b == 254) // link-local
        || (a == 100 && (64..=127).contains(&b)) // Tailscale 100.64/10
}

fn allowed_v6(ip: Ipv6Addr) -> bool {
    let bytes = ip.octets();
    if ip == Ipv6Addr::LOCALHOST {
        return true;
    }
    // ::ffff:a.b.c.d is an IPv4 client seen through a dual-stack socket.
    if bytes[..10].iter().all(|b| *b == 0) && bytes[10] == 0xFF && bytes[11] == 0xFF {
        return allowed_v4(Ipv4Addr::new(bytes[12], bytes[13], bytes[14], bytes[15]));
    }
    bytes[0] & 0xFE == 0xFC // fc00::/7, which holds Tailscale's fd7a:115c:a1e0::/48
        || (bytes[0] == 0xFE && bytes[1] & 0xC0 == 0x80) // fe80::/10
}

pub fn is_tailscale_v4(address: &str) -> bool {
    match address.parse::<Ipv4Addr>() {
        Ok(ip) => {
            let [a, b, _, _] = ip.octets();
            a == 100 && (64..=127).contains(&b)
        }
        Err(_) => false,
    }
}

pub fn is_private_lan_v4(address: &str) -> bool {
    match address.parse::<Ipv4Addr>() {
        Ok(ip) => {
            let [a, b, _, _] = ip.octets();
            a == 10 || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168)
        }
        Err(_) => false,
    }
}

/// A peer's address as the limiter and the audit log key it: an IPv4 client seen through a
/// dual-stack socket is its IPv4 address.
pub fn canonical(ip: IpAddr) -> String {
    ip.to_canonical().to_string()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalAddresses {
    pub lan: Vec<String>,
    pub tailscale: Option<String>,
}

impl LocalAddresses {
    /// Sorts interface addresses: private IPv4 of up, non-loopback interfaces, and the Tailscale
    /// one (100.64/10 on a `utun`, or `tailscale*` on Linux).
    pub fn from_interfaces<'a>(entries: impl IntoIterator<Item = (&'a str, Ipv4Addr)>) -> Self {
        let mut result = LocalAddresses::default();
        for (name, ip) in entries {
            let text = ip.to_string();
            if is_tailscale_v4(&text) {
                let tunnel = name.starts_with("utun") || name.starts_with("tailscale");
                if tunnel && result.tailscale.is_none() {
                    result.tailscale = Some(text);
                }
            } else if is_private_lan_v4(&text) && !result.lan.contains(&text) {
                result.lan.push(text);
            }
        }
        result
    }

    /// This computer's addresses now (empty where the OS isn't read yet: Windows).
    pub fn current() -> LocalAddresses {
        let list = interfaces();
        LocalAddresses::from_interfaces(list.iter().map(|(n, ip)| (n.as_str(), *ip)))
    }
}

/// Every up, non-loopback IPv4 interface address.
#[cfg(unix)]
fn interfaces() -> Vec<(String, Ipv4Addr)> {
    let mut out = Vec::new();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills `list` with a linked list we free below and only read meanwhile.
    if unsafe { libc::getifaddrs(&mut list) } != 0 {
        return out;
    }
    let mut cursor = list;
    while !cursor.is_null() {
        // SAFETY: `cursor` is a node of the list getifaddrs returned, not yet freed.
        let entry = unsafe { &*cursor };
        cursor = entry.ifa_next;
        let flags = entry.ifa_flags as libc::c_int;
        if flags & libc::IFF_UP == 0 || flags & libc::IFF_LOOPBACK != 0 || entry.ifa_addr.is_null()
        {
            continue;
        }
        // SAFETY: ifa_addr is non-null and points at a sockaddr of the family it names.
        let family = unsafe { (*entry.ifa_addr).sa_family };
        if libc::c_int::from(family) != libc::AF_INET {
            continue;
        }
        // SAFETY: an AF_INET address is a sockaddr_in.
        let raw = unsafe {
            (*(entry.ifa_addr as *const libc::sockaddr_in))
                .sin_addr
                .s_addr
        };
        // SAFETY: ifa_name is a NUL-terminated C string owned by the list.
        let name = unsafe { std::ffi::CStr::from_ptr(entry.ifa_name) }
            .to_string_lossy()
            .into_owned();
        out.push((name, Ipv4Addr::from(u32::from_be(raw))));
    }
    // SAFETY: `list` came from getifaddrs and is freed once.
    unsafe { libc::freeifaddrs(list) };
    out
}

#[cfg(not(unix))]
fn interfaces() -> Vec<(String, Ipv4Addr)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_lan_link_local_and_tailscale_are_allowed() {
        for address in [
            "127.0.0.1",
            "127.9.9.9",
            "10.0.0.5",
            "10.255.255.255",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.0.10",
            "169.254.12.1",
            "100.64.0.1",
            "100.100.1.1",
            "100.127.255.255",
            "::1",
            "fe80::1",
            "fe80::1%en0",
            "febf::1",
            "fc00::1",
            "fd12:3456::1",
            "fdff::1",
            "fd7a:115c:a1e0::5",
            "fd7a:115c:a1e0:ab12::1",
            "::ffff:192.168.1.5",
            "::ffff:127.0.0.1",
            "[::1]",
        ] {
            assert!(is_allowed(address), "{address}");
        }
    }

    #[test]
    fn everything_else_is_blocked() {
        for address in [
            "8.8.8.8",
            "1.1.1.1",
            "172.15.255.255",
            "172.32.0.1",
            "192.169.0.1",
            "192.167.255.255",
            "100.63.255.255",
            "100.128.0.1",
            "169.253.0.1",
            "11.0.0.1",
            "0.0.0.0",
            "2001:4860:4860::8888",
            "fec0::1",
            "fe00::1",
            "fb00::1",
            "fe::1",
            "::ffff:8.8.8.8",
            "::",
            "",
            "not an address",
            "300.1.1.1",
        ] {
            assert!(!is_allowed(address), "{address}");
        }
    }

    #[test]
    fn unique_local_ipv6_includes_the_tailscale_range() {
        assert!(is_allowed("fd7a:115c:a1e1::1"));
        assert!(is_allowed("fd7b:115c:a1e0::1"));
        assert!(
            !is_allowed("fe00::1"),
            "the next block after fc00::/7 is not"
        );
    }

    #[test]
    fn tailscale_and_lan_classifiers() {
        assert!(is_tailscale_v4("100.101.102.103"));
        assert!(!is_tailscale_v4("100.200.0.1"));
        assert!(is_private_lan_v4("192.168.1.9"));
        assert!(!is_private_lan_v4("100.64.0.1"));
    }

    #[test]
    fn interfaces_sort_into_lan_and_tailscale() {
        let ip = |s: &str| s.parse::<Ipv4Addr>().unwrap();
        let found = LocalAddresses::from_interfaces([
            ("en0", ip("192.168.1.5")),
            ("en1", ip("192.168.1.5")),
            ("en5", ip("8.8.8.8")),
            ("en6", ip("100.70.0.1")),
            ("utun3", ip("100.101.1.2")),
            ("utun4", ip("100.101.1.3")),
            ("bridge0", ip("10.0.0.2")),
        ]);
        assert_eq!(found.lan, vec!["192.168.1.5", "10.0.0.2"]);
        assert_eq!(found.tailscale.as_deref(), Some("100.101.1.2"));
        let _ = LocalAddresses::current();
    }

    #[test]
    fn a_mapped_address_is_keyed_as_ipv4() {
        let ip: IpAddr = "::ffff:192.168.1.5".parse().unwrap();
        assert_eq!(canonical(ip), "192.168.1.5");
        assert!(is_allowed_ip(ip));
    }
}
