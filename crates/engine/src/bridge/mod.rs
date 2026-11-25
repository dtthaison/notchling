pub mod agents;
pub mod backend;
pub mod connection;
pub mod events_audit;
pub mod extras;
mod fit;
pub mod foreign;
pub mod hub;
pub mod images;
pub mod inbox;
pub mod live;
pub mod model;
pub mod net;
pub mod relay;
pub mod router;
mod router_inbox;
mod router_machines;
mod router_ops;
mod router_relay;
pub mod snapshot;
pub mod stream;
pub mod uploads;
pub mod wire;

pub use backend::BridgeBackend;
pub use connection::{Connection, ConnectionIo};
pub use hub::Hub;
pub use router::{Router, RouterParts};
pub use router_ops::local_url;
pub use wire::BridgeFailure;

#[cfg(test)]
mod extras_tests;
#[cfg(test)]
mod fixtures_data;
#[cfg(test)]
mod fixtures_tests;
#[cfg(test)]
mod foreign_tests;
#[cfg(test)]
mod frame_tests;
#[cfg(test)]
mod hub_tests;
#[cfg(test)]
mod images_tests;
#[cfg(test)]
mod inbox_tests;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod router_cluster_tests;
#[cfg(test)]
mod router_machines_tests;
#[cfg(test)]
mod router_relay_tests;
#[cfg(test)]
mod router_tests;
#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod stream_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod uploads_tests;
#[cfg(test)]
mod wire_tests;
