use super::wire::BridgeFailure;
use crate::viz::url::Url;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

pub const LOAD_LIMIT: Duration = Duration::from_secs(10);
pub const SETTLE: Duration = Duration::from_millis(700);
pub const REACHABLE_LIMIT: Duration = Duration::from_secs(1);
pub const JPEG_QUALITY: u8 = 60;
const HEIGHT_RATIO: f64 = 1.8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRequest {
    pub url: String,
    pub width: i64,
}

impl SnapshotRequest {
    pub fn view_size(&self) -> (u32, u32) {
        let width = self.width.max(1) as f64;
        (width as u32, (width * HEIGHT_RATIO).round() as u32)
    }
}

pub trait Snapshotter: Send + Sync {
    fn capture(&self, request: &SnapshotRequest) -> Result<Vec<u8>, BridgeFailure>;
}

pub struct Unsupported;

impl Snapshotter for Unsupported {
    fn capture(&self, request: &SnapshotRequest) -> Result<Vec<u8>, BridgeFailure> {
        Err(BridgeFailure::not_reachable(format!(
            "Máy này chưa chụp được {}.",
            request.url
        )))
    }
}

pub fn allows_navigation(scheme: &str) -> bool {
    matches!(scheme, "http" | "https" | "about")
}

pub fn not_running_message(url: &Url) -> String {
    format!(
        "{}:{} không chạy trên Mac (không trả lời trong 1 giây).",
        url.host,
        url.port.map_or_else(|| "80".to_string(), |p| p.to_string())
    )
}

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

pub struct WebSnapshots {
    snapshotter: Box<dyn Snapshotter>,
    reachable: Reachable,
}

impl WebSnapshots {
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

    pub fn snapshot(&self, url: &str, width: i64) -> Result<Vec<u8>, BridgeFailure> {
        let parsed = Url::parse_local(url).ok_or_else(|| {
            BridgeFailure::invalid("Chỉ chụp được URL http(s) trên localhost hoặc 127.0.0.1.")
        })?;
        if !(self.reachable)(&parsed) {
            return Err(BridgeFailure::not_reachable(not_running_message(&parsed)));
        }
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
