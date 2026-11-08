//! `BridgeConnection`: one phone on the wire, what the router sees of it.
//!
//! The live [`ConnectionIo`] queues frames for the connection's own writer thread, so a phone that
//! stops reading holds up nobody else (its queue fills and it is closed); tests use a recorder.

use super::devices::PairedDevice;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Runs once the frame has left (a binary frame counts as busy until then).
pub type Sent = Box<dyn FnOnce() + Send>;

/// The wire side of one client.
pub trait ConnectionIo: Send + Sync {
    fn send_text(&self, text: &str);
    fn send_binary(&self, data: Vec<u8>, sent: Sent);
    /// A close frame behind anything already queued, then the socket goes.
    fn close(&self);
}

/// The binary frames the bridge sends.
pub mod phone_frame {
    /// A screen frame: magic `PHN1`, the sequence (u32 big-endian), then the JPEG.
    pub fn encode(sequence: u32, jpeg: &[u8]) -> Vec<u8> {
        tagged(b"PHN1", sequence, jpeg)
    }

    /// The reply to `chat.image`: magic `IMG1`, the request id, then the JPEG.
    pub fn encode_image(request_id: u32, jpeg: &[u8]) -> Vec<u8> {
        tagged(b"IMG1", request_id, jpeg)
    }

    /// The reply to `web.snapshot`: magic `WEB1`, the request id (0 when not a number), the JPEG.
    pub fn encode_snapshot(request_id: u32, jpeg: &[u8]) -> Vec<u8> {
        tagged(b"WEB1", request_id, jpeg)
    }

    fn tagged(magic: &[u8; 4], number: u32, jpeg: &[u8]) -> Vec<u8> {
        let mut data = Vec::with_capacity(8 + jpeg.len());
        data.extend_from_slice(magic);
        data.extend_from_slice(&number.to_be_bytes());
        data.extend_from_slice(jpeg);
        data
    }
}

/// At most one screen frame in flight per client: a frame offered while the last one is still
/// being sent is dropped, never queued.
#[derive(Default)]
pub struct FrameGate {
    state: Arc<Mutex<GateState>>,
}

#[derive(Default)]
struct GateState {
    in_flight: bool,
    sequence: u32,
    dropped: u64,
    sent: u64,
}

impl FrameGate {
    pub fn offer(&self, jpeg: &[u8], io: &dyn ConnectionIo) -> bool {
        let frame = {
            let mut s = lock(&self.state);
            if s.in_flight {
                s.dropped += 1;
                return false;
            }
            s.in_flight = true;
            s.sequence = s.sequence.wrapping_add(1);
            s.sent += 1;
            phone_frame::encode(s.sequence, jpeg)
        };
        let state = self.state.clone();
        io.send_binary(frame, Box::new(move || lock(&state).in_flight = false));
        true
    }

    pub fn dropped(&self) -> u64 {
        lock(&self.state).dropped
    }

    pub fn sent(&self) -> u64 {
        lock(&self.state).sent
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub struct Connection {
    id: u64,
    /// The peer's address; for a tunnel client the real client (`Cf-Connecting-Ip`), not the
    /// loopback cloudflared connects from.
    remote_address: String,
    is_tunnel: bool,
    io: Arc<dyn ConnectionIo>,
    pub frame_gate: FrameGate,
    device: Mutex<Option<PairedDevice>>,
    closing: AtomicBool,
    inflight_sends: Mutex<HashSet<String>>,
}

impl Connection {
    pub fn new(remote_address: &str, io: Arc<dyn ConnectionIo>, is_tunnel: bool) -> Connection {
        Connection {
            id: NEXT_ID.fetch_add(1, Ordering::SeqCst),
            remote_address: remote_address.to_string(),
            is_tunnel,
            io,
            frame_gate: FrameGate::default(),
            device: Mutex::new(None),
            closing: AtomicBool::new(false),
            inflight_sends: Mutex::new(HashSet::new()),
        }
    }

    /// Unique for this run; what the hub keys its per-phone state by.
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn remote_address(&self) -> &str {
        &self.remote_address
    }

    pub fn is_tunnel(&self) -> bool {
        self.is_tunnel
    }

    pub fn io(&self) -> &dyn ConnectionIo {
        self.io.as_ref()
    }

    pub fn send_text(&self, text: &str) {
        self.io.send_text(text);
    }

    pub fn device(&self) -> Option<PairedDevice> {
        lock(&self.device).clone()
    }

    pub fn is_authenticated(&self) -> bool {
        lock(&self.device).is_some()
    }

    pub(crate) fn authenticate(&self, device: PairedDevice) {
        *lock(&self.device) = Some(device);
    }

    /// Recorded in the audit log for tunnel clients only; local clients are the person at the
    /// computer or their own network.
    pub fn audit_address(&self) -> Option<&str> {
        self.is_tunnel.then_some(self.remote_address.as_str())
    }

    /// Marks a message send to `session` as running; false when one already is (`busy`).
    pub fn begin_send(&self, session: &str) -> bool {
        lock(&self.inflight_sends).insert(session.to_string())
    }

    pub fn end_send(&self, session: &str) {
        lock(&self.inflight_sends).remove(session);
    }

    /// Closes once; later calls do nothing.
    pub fn close(&self) {
        if !self.closing.swap(true, Ordering::SeqCst) {
            self.io.close();
        }
    }

    pub fn is_closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }
}

/// Records what a connection sends (tests).
#[cfg(test)]
#[derive(Default)]
pub(crate) struct RecordingIo {
    pub texts: Mutex<Vec<String>>,
    pub binaries: Mutex<Vec<Vec<u8>>>,
    pub pending: Mutex<Vec<Sent>>,
    pub closed: AtomicBool,
}

#[cfg(test)]
impl RecordingIo {
    pub fn texts(&self) -> Vec<String> {
        lock(&self.texts).clone()
    }
    pub fn json(&self, index: usize) -> serde_json::Value {
        serde_json::from_str(&self.texts()[index]).unwrap_or_default()
    }
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    /// Lets every binary frame finish sending.
    pub fn finish_sends(&self) {
        for sent in std::mem::take(&mut *lock(&self.pending)) {
            sent();
        }
    }
}

#[cfg(test)]
impl ConnectionIo for RecordingIo {
    fn send_text(&self, text: &str) {
        lock(&self.texts).push(text.to_string());
    }
    fn send_binary(&self, data: Vec<u8>, sent: Sent) {
        lock(&self.binaries).push(data);
        lock(&self.pending).push(sent);
    }
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_frames_carry_their_magic_and_number() {
        assert_eq!(phone_frame::encode(7, b"J"), b"PHN1\0\0\0\x07J".to_vec());
        assert_eq!(
            phone_frame::encode_image(1, b""),
            b"IMG1\0\0\0\x01".to_vec()
        );
        assert_eq!(
            phone_frame::encode_snapshot(258, b"x"),
            b"WEB1\0\0\x01\x02x".to_vec()
        );
    }

    #[test]
    fn a_frame_offered_while_one_is_in_flight_is_dropped() {
        let io = RecordingIo::default();
        let gate = FrameGate::default();
        assert!(gate.offer(b"a", &io));
        assert!(!gate.offer(b"b", &io), "the first is still going out");
        assert_eq!(gate.dropped(), 1);
        io.finish_sends();
        assert!(gate.offer(b"c", &io));
        assert_eq!(gate.sent(), 2);
        let frames = lock(&io.binaries).clone();
        assert_eq!(
            frames[1][4..8],
            2u32.to_be_bytes(),
            "sequence counts sent frames"
        );
    }

    #[test]
    fn a_connection_closes_once_and_audits_only_tunnel_addresses() {
        let io = Arc::new(RecordingIo::default());
        let local = Connection::new("192.168.1.5", io.clone(), false);
        assert_eq!(local.audit_address(), None);
        local.close();
        local.close();
        assert!(io.is_closed() && local.is_closing());
        let tunnel = Connection::new("203.0.113.7", Arc::new(RecordingIo::default()), true);
        assert_eq!(tunnel.audit_address(), Some("203.0.113.7"));
        assert_ne!(local.id(), tunnel.id());
        assert!(tunnel.begin_send("s"));
        assert!(!tunnel.begin_send("s"), "busy");
        tunnel.end_send("s");
        assert!(tunnel.begin_send("s"));
    }
}
