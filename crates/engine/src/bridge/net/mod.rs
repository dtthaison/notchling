pub mod access;
pub mod address;
pub mod app_url;
pub mod audit;
pub mod bridge;
pub mod core;
pub mod crypto;
pub mod devices;
pub mod fetch;
pub mod files;
pub mod frame;
pub mod http;
pub mod identity;
pub mod jwt;
pub mod limiter;
pub mod pairing;
pub mod parts;
pub mod server;
mod wire;

#[cfg(test)]
mod e2e_tests;

pub use bridge::{Bridge, BridgeConfig};

pub mod ports {
    pub const LAN: u16 = 47821;
    pub const TUNNEL: u16 = 47822;
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_ports_are_fixed_and_the_tunnel_one_is_not_the_lan_one() {
        assert_eq!(super::ports::LAN, 47821);
        assert_eq!(super::ports::TUNNEL, 47822);
    }
}
