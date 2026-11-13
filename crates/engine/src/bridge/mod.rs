//! E5a: the phone bridge's router and hub, `agentconsole.v1` (`protocol/README.md`), ported from
//! ConsoleCore's `BridgeRouter`, `BridgeHub` and `BridgeProtocol`, with `LiveBridgeBackend`,
//! `PhoneModel`'s bridge side and `ForeignSessions` from the Swift app. Nothing here opens a socket:
//! the transport (E5b, `bridge::net` and the daemon) feeds frames in and carries frames out.
//!
//! # The API the transport uses
//!
//! ```text
//! let hub = Arc::new(Hub::new());
//! let router = Router::new(RouterParts { backend, devices, codes, limiter, audit,
//!                                        hub: hub.clone(), home, images_dir, now_ms });
//!
//! // A client connected (after TLS and the WebSocket upgrade, or the tunnel's Access check):
//! let connection: Arc<Connection> = hub.connect(&remote_address, is_tunnel, io);
//! // Each text frame it sends (blocking; run it off the reading thread):
//! router.handle(&connection, &text);
//! // It went away:
//! router.closed(&connection);
//! // Every 300 ms, on one timer thread:
//! hub.tick(backend.as_ref());
//! // After a revoke: hub.disconnect(&device_id). On `agentconsole://open?phones=1`: hub.notify_open().
//! ```
//!
//! - `io: Arc<dyn ConnectionIo>` is the transport's side of one client: `send_text(String)`,
//!   `send_binary(Vec<u8>, sent)` (call `sent` once the frame has left: the device-window stream
//!   keeps one frame in flight) and `close()`. They queue and return; the router never waits on
//!   the network. Binary frames are complete (`PHN1`/`IMG1`/`WEB1` + `u32` + JPEG).
//! - Every answer, event and binary frame goes out through `io`; [`Router::handle`] returns nothing.
//!   One request may write two answers and a binary frame (`web.snapshot`, `chat.image`).
//! - `handle` may run concurrently for frames of one connection (a `pair` waits minutes for the
//!   person; a second `session.message` to the same session meanwhile is answered `busy`).
//! - The transport enforces the 256 KB frame cap before handing the frame over too; the router
//!   refuses a bigger one with `invalid` and, before login, closes the connection.
//! - A refused login (`hello`, `pair`, garbage) calls `io.close()`; the transport then drops the
//!   socket and calls `closed`. A locked-out address gets `close()` with no answer.
//! - [`backend::Devices`], [`backend::PairingCodes`], [`backend::RateLimiter`] and
//!   [`backend::Audit`] are the transport's: the device store, codes, limiter and audit log in
//!   the Swift app's formats.
//!
//! # The backend
//!
//! [`backend::BridgeBackend`] is what the router asks of the app (the Swift protocol of that name).
//! [`live::LiveBackend`] implements it over [`live::Sources`] (what the daemon reads) and
//! [`live::Acting`] (what it does, through its acting ops, so the acting switch applies;
//! [`live::ActingOff`] refuses everything).
//!
//! # Extras (E15)
//!
//! [`extras::Extras`] holds what `LiveBackend` needs beyond the sessions, each behind a seam:
//!
//! - Visualize (`viz.get`): the pure engine in [`crate::viz`]; git goes through the `Launcher`.
//!   Entity specs need the schema readers, which are not ported: [`crate::viz::Schemas`] answers
//!   "unsupported" until they are.
//! - Chat pictures (`chat.image`): a phone names only a picture the chat listed; the bytes are read
//!   again on demand and turned into a JPEG by [`images::Imaging`] (the `image` crate; HEIC is
//!   unsupported). Pictures named by file are served only from home and `/tmp`, symlinks resolved
//!   at fetch time.
//! - Photo uploads (`session.image.*`): see [`uploads`]; the path is typed through
//!   [`live::Acting`], so it is refused while acting is off, and the refused file is deleted.
//! - Web snapshots (`web.snapshot`): [`snapshot::WebSnapshots`] checks the URL is local and
//!   answers, then asks a [`snapshot::Snapshotter`]. No renderer exists here yet
//!   ([`snapshot::Unsupported`], answered `not_reachable`).
//! - The device-window stream (`subscribe {phone}`): [`stream::WindowStreamer`] over a
//!   [`stream::ScreenCapture`]; no capture exists here yet ([`stream::UnsupportedStreamer`]).
//!
//! # Rulings
//!
//! - The generated `notchling_protocol` types are the wire for everything that matches it: chat
//!   messages, steps, pictures, commands, facts, limits, subagents, prompts, questions, sessions and
//!   viz specs. A session and a prompt also have engine types ([`model::Session`],
//!   [`model::PendingPrompt`]) because the router and backend need what the phone never sees (pid,
//!   job id, transcript, subagent transcripts).
//! - Requests are read from the JSON object field by field, not through the generated request
//!   types: the Swift router reads a field of the wrong type as absent (a string `limit` gives the
//!   default 50), and an `id` of any JSON type is echoed back.
//! - Answers are written with sorted keys and whole numbers without a fraction, as Swift's
//!   `JSONSerialization` wrote them, because the generated types keep times as `f64`.

pub mod agents;
pub mod backend;
pub mod connection;
pub mod extras;
pub mod foreign;
pub mod hub;
pub mod images;
pub mod inbox;
pub mod live;
pub mod model;
pub mod net;
pub mod router;
mod router_inbox;
mod router_ops;
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
mod hub_tests;
#[cfg(test)]
mod images_tests;
#[cfg(test)]
mod inbox_tests;
#[cfg(test)]
mod live_tests;
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
