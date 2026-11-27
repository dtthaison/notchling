use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub fn is_tailscale(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4_is_tailscale(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4_is_tailscale(&v4),
            None => v6_is_tailscale(v6),
        },
    }
}

fn v4_is_tailscale(ip: &Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    a == 100 && (64..128).contains(&b)
}

fn v6_is_tailscale(ip: &Ipv6Addr) -> bool {
    let s = ip.segments();
    s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0
}

#[cfg(unix)]
pub fn tailscale_addrs() -> Vec<IpAddr> {
    let mut found: Vec<IpAddr> = interface_addrs().into_iter().filter(is_tailscale).collect();
    found.sort();
    found.dedup();
    found
}

#[cfg(not(unix))]
pub fn tailscale_addrs() -> Vec<IpAddr> {
    Vec::new()
}

#[cfg(unix)]
fn interface_addrs() -> Vec<IpAddr> {
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut at = head;
    while !at.is_null() {
        let entry = unsafe { &*at };
        if let Some(ip) = unsafe { sockaddr_ip(entry.ifa_addr) } {
            out.push(ip);
        }
        at = entry.ifa_next;
    }
    unsafe { libc::freeifaddrs(head) };
    out
}

#[cfg(unix)]
unsafe fn sockaddr_ip(addr: *const libc::sockaddr) -> Option<IpAddr> {
    if addr.is_null() {
        return None;
    }
    match i32::from(unsafe { (*addr).sa_family }) {
        libc::AF_INET => {
            let v4 = unsafe { &*(addr as *const libc::sockaddr_in) };
            Some(IpAddr::V4(Ipv4Addr::from(u32::from_be(v4.sin_addr.s_addr))))
        }
        libc::AF_INET6 => {
            let v6 = unsafe { &*(addr as *const libc::sockaddr_in6) };
            Some(IpAddr::V6(Ipv6Addr::from(v6.sin6_addr.s6_addr)))
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "net_if_tests.rs"]
mod tests;
