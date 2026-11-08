//! Web snapshots for the phone: a picture of a page on this computer's localhost (Swift
//! `WebSnapshotter` and `LiveBridgeBackend.webSnapshot`).
//!
//! What is pure lives here: which URLs may be shot, the size of the page the picture is taken at,
//! the reachability check (the Swift app's one-second HEAD request) and the messages. The capture
//! itself is a [`Snapshotter`]: on macOS the Swift app renders in an offscreen `WKWebView`, which
//! needs WebKit bindings this workspace does not have, so every platform answers [`Unsupported`]
//! until one is written.

use super::wire::BridgeFailure;
use crate::viz::url::Url;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

/// How long the page may take to load before the snapshot is given up.
pub const LOAD_LIMIT: Duration = Duration::from_secs(10);
/// Time given to late scripts and images to paint after the load finished.
pub const SETTLE: Duration = Duration::from_millis(700);
/// How long the check that something answers on the port may take.
pub const REACHABLE_LIMIT: Duration = Duration::from_secs(1);
/// JPEG quality of the picture (0.6 in the Swift app).
pub const JPEG_QUALITY: u8 = 60;
/// The page is laid out at `width` by `width * 1.8`: a phone-shaped screen.
const HEIGHT_RATIO: f64 = 1.8;

/// What to shoot: a local http(s) URL at a width in points (100 to 1200 after the router's clamp).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRequest {
    pub url: String,
    pub width: i64,
}

impl SnapshotRequest {
    /// The size of the page the picture is taken at.
    pub fn view_size(&self) -> (u32, u32) {
        let width = self.width.max(1) as f64;
        (width as u32, (width * HEIGHT_RATIO).round() as u32)
    }
}

/// Renders a page to a JPEG. The implementation navigates only where [`allows_navigation`] says,
/// gives up after [`LOAD_LIMIT`], waits [`SETTLE`] and encodes at [`JPEG_QUALITY`].
pub trait Snapshotter: Send + Sync {
    fn capture(&self, request: &SnapshotRequest) -> Result<Vec<u8>, BridgeFailure>;
}

/// No renderer on this computer.
pub struct Unsupported;

impl Snapshotter for Unsupported {
    fn capture(&self, request: &SnapshotRequest) -> Result<Vec<u8>, BridgeFailure> {
        Err(BridgeFailure::not_reachable(format!(
            "Máy này chưa chụp được {}.",
            request.url
        )))
    }
}

/// A page may follow links to these schemes only: nothing like `file:` or `javascript:`.
pub fn allows_navigation(scheme: &str) -> bool {
    matches!(scheme, "http" | "https" | "about")
}

/// The message when nothing answers: the host and port as the phone should name them.
pub fn not_running_message(url: &Url) -> String {
    format!(
        "{}:{} không chạy trên Mac (không trả lời trong 1 giây).",
        url.host,
        url.port.map_or_else(|| "80".to_string(), |p| p.to_string())
    )
}

/// True when something answers HTTP on the URL's port within `limit`: a `HEAD` request that gets a
/// status line back. For `https` there is no TLS here, so a connection that opens counts.
pub fn http_reachable(url: &Url, limit: Duration) -> bool {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, url.effective_port()));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, limit) else {
        return false;
    };
    if url.scheme == "https" {
        return true;
    }
    let _ = stream.set_read_timeout(Some(limit));
    let _ = stream.set_write_timeout(Some(limit));
    let path = if url.path.is_empty() { "/" } else { &url.path };
    let request = format!(
        "HEAD {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: notchling\r\n\r\n",
        url.host
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut start = [0u8; 5];
    stream.read_exact(&mut start).is_ok() && &start == b"HTTP/"
}

type Reachable = Box<dyn Fn(&Url) -> bool + Send + Sync>;

/// The `web.snapshot` backend: checks the page answers, then asks the [`Snapshotter`].
pub struct WebSnapshots {
    snapshotter: Box<dyn Snapshotter>,
    reachable: Reachable,
}

impl WebSnapshots {
    /// With the real reachability check.
    pub fn new(snapshotter: Box<dyn Snapshotter>) -> WebSnapshots {
        WebSnapshots {
            snapshotter,
            reachable: Box::new(|url| http_reachable(url, REACHABLE_LIMIT)),
        }
    }

    pub fn with_reachable(mut self, reachable: Reachable) -> WebSnapshots {
        self.reachable = reachable;
        self
    }

    /// A JPEG of `url`, which must be http(s) on localhost or 127.0.0.1 (the router checks too).
    pub fn snapshot(&self, url: &str, width: i64) -> Result<Vec<u8>, BridgeFailure> {
        let parsed = Url::parse_local(url).ok_or_else(|| {
            BridgeFailure::invalid("Chỉ chụp được URL http(s) trên localhost hoặc 127.0.0.1.")
        })?;
        if !(self.reachable)(&parsed) {
            return Err(BridgeFailure::not_reachable(not_running_message(&parsed)));
        }
        // The renderer gets the parsed URL written out again: no user info, so the host it
        // navigates to is the one that was checked, whatever it makes of an `@`.
        let jpeg = self.snapshotter.capture(&SnapshotRequest {
            url: parsed.render(),
            width,
        })?;
        if jpeg.starts_with(&[0xFF, 0xD8]) {
            Ok(jpeg)
        } else {
            Err(BridgeFailure::internal("Không mã hoá được ảnh JPEG."))
        }
    }
}
