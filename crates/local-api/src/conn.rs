//! One connection: the peer check, then lines until the client goes, a deadline passes or the
//! server stops. Replies and events leave through a writer thread with a bounded queue.

use crate::framing::{encode_line, FrameError, LineReader};
use crate::peer::same_user;
use crate::server::{Caller, Shared};
use crate::transport::{self, Stream};
use crate::wire::{
    err_reply, ok_reply, ops, ApiError, ClientKind, ErrorCode, Hello, HelloReply, Request,
    Subscribe, Tier, API_VERSION,
};
use serde_json::Value;
use std::io::Write;
use std::net::Shutdown;
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::time::Instant;

pub(crate) fn serve(stream: Stream, shared: Arc<Shared>) {
    let peer = match transport::peer(&stream) {
        Ok(peer) => peer,
        Err(_) => {
            shared.audit("connect", None, None, "peer_unknown");
            return;
        }
    };
    if !same_user(peer.uid, shared.my_uid) {
        shared.audit("connect", None, peer.exe.as_deref(), "other_user");
        return;
    }
    let tier = shared.allowlist.tier_of(peer.exe.as_deref());
    let limits = shared.limits;
    if stream.set_read_timeout(Some(limits.tick)).is_err()
        || stream.set_write_timeout(Some(limits.write_within)).is_err()
    {
        return;
    }
    let (Ok(write_half), Ok(hub_half)) = (stream.try_clone(), stream.try_clone()) else {
        return;
    };
    let (tx, rx) = sync_channel::<Vec<u8>>(limits.queue);
    let writer = std::thread::Builder::new()
        .name("notchling-api-write".into())
        .spawn(move || write_loop(write_half, rx));
    let Ok(writer) = writer else {
        return;
    };
    let id = shared.hub.add(tier, tx.clone(), hub_half);
    let mut session = Session {
        id,
        tier,
        peer: peer.exe,
        kind: None,
        tx,
        shared: shared.clone(),
        accepted: Instant::now(),
    };
    let reader = match stream.try_clone() {
        Ok(read_half) => Some(LineReader::new(read_half)),
        Err(_) => None,
    };
    if let Some(reader) = reader {
        session.read_loop(reader);
    }
    // Off the list, then let the writer send what is queued (a refusal's reply, say) before closing.
    // A client that doesn't read holds this up only until the write timeout.
    shared.hub.forget(id);
    drop(session);
    let _ = writer.join();
    let _ = stream.shutdown(Shutdown::Both);
}

fn write_loop(mut stream: Stream, rx: Receiver<Vec<u8>>) {
    for line in rx {
        if stream.write_all(&line).is_err() {
            let _ = stream.shutdown(Shutdown::Both);
            return;
        }
    }
}

struct Session {
    id: u64,
    tier: Tier,
    peer: Option<PathBuf>,
    kind: Option<ClientKind>,
    tx: SyncSender<Vec<u8>>,
    shared: Arc<Shared>,
    accepted: Instant,
}

impl Session {
    fn read_loop(&mut self, mut reader: LineReader<Stream>) {
        let limits = self.shared.limits;
        loop {
            if self.shared.stopping() {
                return;
            }
            match reader.next_line() {
                Ok(Some(line)) => {
                    if !self.on_line(&line) {
                        return;
                    }
                }
                Ok(None) => return,
                Err(e) if e.is_timeout() => {}
                // An oversize line, a line cut off or a read error: close.
                Err(FrameError::TooLong | FrameError::PartialAtEof | FrameError::Io(_)) => return,
            }
            if self.kind.is_none() && self.accepted.elapsed() > limits.hello_within {
                return;
            }
            if reader
                .partial_since()
                .is_some_and(|t| t.elapsed() > limits.line_within)
            {
                return;
            }
        }
    }

    /// Queues a reply; false when the client is gone. Waiting here holds up only this client.
    fn send(&self, value: &Value) -> bool {
        let line = encode_line(value).or_else(|| {
            let id = value.get("id").and_then(Value::as_u64).unwrap_or(0);
            encode_line(&err_reply(
                id,
                &ApiError::internal("the reply is over the line size cap"),
            ))
        });
        match line {
            Some(line) => self.tx.send(line).is_ok(),
            None => false,
        }
    }

    fn reply(&self, id: u64, result: Result<Value, ApiError>) -> bool {
        match result {
            Ok(value) => self.send(&ok_reply(id, value)),
            Err(e) => self.send(&err_reply(id, &e)),
        }
    }

    /// Handles one line; false closes the connection.
    fn on_line(&mut self, line: &[u8]) -> bool {
        let request = match Request::parse(line) {
            Ok(request) => request,
            Err((id, e)) => {
                // Garbage before hello closes; after it, the client may go on.
                return self.reply(id, Err(e)) && self.kind.is_some();
            }
        };
        let id = request.id;
        if self.kind.is_none() {
            if request.op != ops::HELLO {
                self.reply(id, Err(ApiError::bad_request("the first op must be hello")));
                return false;
            }
            return self.hello(id, &request.args);
        }
        match request.op.as_str() {
            ops::HELLO => self.reply(id, Err(ApiError::bad_request("hello was already said"))),
            ops::DAEMON_STATUS => {
                let status = serde_json::to_value(self.shared.status())
                    .map_err(|e| ApiError::internal(e.to_string()));
                self.reply(id, status)
            }
            ops::EVENTS_SUBSCRIBE => {
                let result = self.subscribe(&request.args);
                self.reply(id, result)
            }
            op => {
                let result = self.dispatch(op, &request.args);
                self.reply(id, result)
            }
        }
    }

    fn hello(&mut self, id: u64, args: &Value) -> bool {
        let hello: Hello = match serde_json::from_value(args.clone()) {
            Ok(hello) => hello,
            Err(e) => {
                self.reply(id, Err(ApiError::bad_request(format!("bad hello: {e}"))));
                return false;
            }
        };
        if hello.api != API_VERSION {
            let message = format!(
                "api {} is not served; this daemon speaks {API_VERSION}",
                hello.api
            );
            self.reply(id, Err(ApiError::bad_request(message)));
            return false;
        }
        self.kind = Some(hello.client);
        self.shared.hub.hello(self.id, hello.client);
        let reply = HelloReply {
            api: API_VERSION,
            daemon: self.shared.version.clone(),
            tier: self.tier,
        };
        let value = serde_json::to_value(reply).map_err(|e| ApiError::internal(e.to_string()));
        self.reply(id, value)
    }

    fn subscribe(&self, args: &Value) -> Result<Value, ApiError> {
        let subscribe: Subscribe = serde_json::from_value(args.clone())
            .map_err(|e| ApiError::bad_request(format!("bad subscribe: {e}")))?;
        let known = self.shared.handler.topics();
        if let Some(bad) = subscribe
            .topics
            .iter()
            .find(|t| !known.contains(&t.as_str()))
        {
            return Err(ApiError::bad_request(format!("no topic named {bad}")));
        }
        self.shared.hub.subscribe(self.id, &subscribe.topics);
        Ok(serde_json::json!({ "topics": subscribe.topics }))
    }

    fn dispatch(&self, op: &str, args: &Value) -> Result<Value, ApiError> {
        let handler = &self.shared.handler;
        let Some(spec) = handler.spec(op) else {
            return Err(ApiError::new(
                ErrorCode::UnknownOp,
                format!("no op named {op}"),
            ));
        };
        if spec.tier == Tier::App && self.tier != Tier::App {
            self.shared
                .audit(op, Some(self.tier), self.peer.as_deref(), "forbidden");
            return Err(ApiError::new(
                ErrorCode::Forbidden,
                "only the Notchling app may do this",
            ));
        }
        if spec.approval && !self.shared.hub.app_attached() {
            self.shared
                .audit(op, Some(self.tier), self.peer.as_deref(), "needs_approval");
            return Err(ApiError::new(
                ErrorCode::NeedsApproval,
                "this needs approval in the Notchling app, and the app isn't attached",
            ));
        }
        let caller = Caller {
            tier: self.tier,
            client: self.kind.unwrap_or(ClientKind::Cli),
            peer: self.peer.clone(),
        };
        handler.call(&caller, op, args)
    }
}
