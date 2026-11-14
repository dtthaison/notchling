mod addr;
mod frame;
mod identity;
mod link;
mod listen;
mod mem;
mod pin;
pub mod tls;

pub use addr::allowed;
pub use frame::{LineReader, Outbox, MAX_LINE, OUTBOX};
pub use identity::{load_or_make, Identity, CERT_FILE, KEY_FILE};
pub use link::{Conn, LinkEvents, Listener, Transport};
pub use listen::TcpTls;
pub use mem::{MemNet, MemTransport};
pub use pin::{Admit, Pins};
pub use tls::{client_config, server_config};

use sha2::{Digest, Sha256};
use std::fmt;

pub const PORT: u16 = 47823;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Tls(String),
    Identity(String),
    NotAllowed(String),
    Closed,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Tls(why) => write!(f, "TLS failed: {why}"),
            Error::Identity(why) => write!(f, "the node identity: {why}"),
            Error::NotAllowed(why) => write!(f, "{why}"),
            Error::Closed => write!(f, "the link is closed"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e)
    }
}

impl From<rustls::Error> for Error {
    fn from(e: rustls::Error) -> Error {
        Error::Tls(e.to_string())
    }
}

pub fn fingerprint(der: &[u8]) -> String {
    let digest = Sha256::digest(der);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[cfg(test)]
mod frame_tests;
#[cfg(test)]
mod identity_tests;
#[cfg(test)]
mod link_tests;
#[cfg(test)]
mod pin_tests;
#[cfg(test)]
mod testutil;
