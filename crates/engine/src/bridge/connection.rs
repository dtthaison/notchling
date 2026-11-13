//! One client: where its frames go ([`ConnectionIo`], the transport's side), who it logged in as,
//! and what it subscribed to (Swift `BridgeConnection`, `FrameGate`).

use super::backend::PairedDevice;
use super::wire::frame;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

/// The transport's side of one client. Implementations queue the frame and return at once.
pub trait ConnectionIo: Send + Sync {
    fn send_text(&self, text: String);
    /// `sent` runs once the frame has left; until then a phone frame counts as in flight.
    fn send_binary(&self, data: Vec<u8>, sent: Box<dyn FnOnce() + Send>);
    /// Closes the WebSocket. Called at most once per connection.
    fn close(&self);
}

/// A mutex that survives a panicking holder: the state is plain data, still usable.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What a client subscribed to, and what it has already been sent (the hub's memory).
#[derive(Debug, Default)]
pub(crate) struct Subscription {
    pub wants_sessions: bool,
    pub wants_inbox: bool,
    pub known_inbox: HashMap<String, String>,
    pub chat_session_id: Option<String>,
    pub chat_subagent_id: Option<String>,
    pub last_sessions_json: Option<String>,
    pub known_prompt_ids: HashSet<String>,
    pub known_chat_ids: HashSet<String>,
    pub phone_active: bool,
}

#[derive(Default)]
struct State {
    device: Option<PairedDevice>,
    closing: bool,
    inflight_sends: HashSet<String>,
}

pub struct Connection {
    pub id: u64,
    /// The peer's address; for a tunnel client the real client (`Cf-Connecting-Ip`).
    pub remote_address: String,
    pub is_tunnel: bool,
    io: Arc<dyn ConnectionIo>,
    frames: FrameGate,
    state: Mutex<State>,
    subscription: Mutex<Subscription>,
}

impl Connection {
    pub(crate) fn new(
        id: u64,
        remote_address: &str,
        is_tunnel: bool,
        io: Arc<dyn ConnectionIo>,
    ) -> Connection {
        Connection {
            id,
            remote_address: remote_address.to_string(),
            is_tunnel,
            io,
            frames: FrameGate::default(),
            state: Mutex::new(State::default()),
            subscription: Mutex::new(Subscription::default()),
        }
    }

    pub fn device(&self) -> Option<PairedDevice> {
        lock(&self.state).device.clone()
    }

    pub fn is_authenticated(&self) -> bool {
        lock(&self.state).device.is_some()
    }

    pub(crate) fn authenticate(&self, device: PairedDevice) {
        lock(&self.state).device = Some(device);
    }

    pub(crate) fn wants_inbox(&self) -> bool {
        lock(&self.subscription).wants_inbox
    }

    /// Whether the client asked for `sessions` pushes.
    pub fn wants_sessions(&self) -> bool {
        lock(&self.subscription).wants_sessions
    }

    pub(crate) fn subscription(&self) -> MutexGuard<'_, Subscription> {
        lock(&self.subscription)
    }

    /// The audit log names the address of tunnel clients only.
    pub(crate) fn audit_address(&self) -> Option<String> {
        self.is_tunnel.then(|| self.remote_address.clone())
    }

    pub(crate) fn device_name(&self) -> String {
        self.device().map(|d| d.name).unwrap_or_else(|| "?".into())
    }

    /// Marks a send to `session_id` as running; false when one already is (answered `busy`).
    pub(crate) fn begin_send(&self, session_id: &str) -> bool {
        lock(&self.state)
            .inflight_sends
            .insert(session_id.to_string())
    }

    pub(crate) fn end_send(&self, session_id: &str) {
        lock(&self.state).inflight_sends.remove(session_id);
    }

    pub fn send_text(&self, text: String) {
        self.io.send_text(text);
    }

    pub fn send_binary(&self, data: Vec<u8>) {
        self.io.send_binary(data, Box::new(|| {}));
    }

    /// Offers a device-window frame; dropped (false) while the last one is still going out.
    pub fn offer_phone_frame(&self, jpeg: &[u8]) -> bool {
        self.frames.offer(jpeg, self.io.as_ref())
    }

    /// Phone frames dropped and sent so far.
    pub fn frame_counts(&self) -> (u64, u64) {
        self.frames.counts()
    }

    pub fn close(&self) {
        {
            let mut state = lock(&self.state);
            if state.closing {
                return;
            }
            state.closing = true;
        }
        self.io.close();
    }

    pub fn is_closing(&self) -> bool {
        lock(&self.state).closing
    }
}

/// At most one phone frame in flight: a frame offered while the last is still being sent is
/// dropped, never queued.
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
        let data = {
            let mut state = lock(&self.state);
            if state.in_flight {
                state.dropped += 1;
                return false;
            }
            state.in_flight = true;
            state.sequence = state.sequence.wrapping_add(1);
            state.sent += 1;
            frame::phone(state.sequence, jpeg)
        };
        let shared = Arc::clone(&self.state);
        io.send_binary(data, Box::new(move || lock(&shared).in_flight = false));
        true
    }

    /// (dropped, sent).
    pub fn counts(&self) -> (u64, u64) {
        let state = lock(&self.state);
        (state.dropped, state.sent)
    }
}
