use notchling_platform::net_if::is_tailscale;
use std::net::IpAddr;

pub fn allowed(ip: &IpAddr, loopback_ok: bool) -> bool {
    let ip = ip.to_canonical();
    if ip.is_unspecified() {
        return false;
    }
    is_tailscale(&ip) || (loopback_ok && ip.is_loopback())
}
