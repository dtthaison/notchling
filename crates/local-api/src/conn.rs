use crate::framing::{encode_line, FrameError, LineReader};
use crate::peer::{same_user, PeerInfo};
use crate::server::{Caller, OpHome, Shared, Slot};
use crate::transport::{self, Stream};
use crate::wire::{
    err_reply, ok_reply, ops, ApiError, ClientKind, ErrorCode, Hello, HelloReply, Request,
    Subscribe, Tier, API_VERSION,
};
use notchling_events::kinds::security::{PeerWhy, RefusedWhy};
use notchling_events::scope::{self, Scope};
use notchling_events::spec::Outcome;
use notchling_events::{Actor, ActorTier, Exe, Ref, Via};
use serde_json::{Map, Value};
use std::io::Write;
use std::net::Shutdown;
use std::path::Path;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::time::Instant;

pub(crate) fn serve(stream: Stream, shared: Arc<Shared>, slot: Slot) {
    let peer = match transport::peer(&stream) {
        Ok(peer) => peer,
        Err(_) => {
            shared.unknown_peer(PeerWhy::PeerUnknown, None);
            return;
        }
    };
    if !same_user(peer.uid, shared.my_uid) {
        shared.unknown_peer(PeerWhy::OtherUser, peer.exe.as_deref());
        return;
    }
    if slot.reserved && shared.allowlist.tier_of(peer.exe.as_deref()) != Tier::App {
        return;
    }
    let limits = shared.limits;
    if stream.set_read_timeout(Some(limits.tick)).is_err()
        || stream.set_write_timeout(Some(limits.write_within)).is_err()
    {
        return;
    }
    let (Ok(write_half), Ok(hub_half), Ok(probe)) =
        (stream.try_clone(), stream.try_clone(), stream.try_clone())
    else {
        return;
    };
    let (tx, rx) = sync_channel::<Vec<u8>>(limits.queue);
    let writer = std::thread::Builder::new()
        .name("notchling-api-write".into())
        .spawn(move || write_loop(write_half, rx));
    let Ok(writer) = writer else {
        return;
    };
    let id = shared.hub.add(Tier::Agent, tx.clone(), hub_half);
    let mut session = Session {
        id,
        tier: Tier::Agent,
        at_accept: peer,
        at_hello: None,
        probe,
        kind: None,
        tx,
        shared: shared.clone(),
        accepted: Instant::now(),
        reserved: slot.reserved,
        subscribed: false,
        last_request: Instant::now(),
        agent_session: None,
    };
    let reader = match stream.try_clone() {
        Ok(read_half) => Some(LineReader::new(read_half)),
        Err(_) => None,
    };
    if let Some(reader) = reader {
        session.read_loop(reader);
    }
    drop(session);
    let _ = writer.join();
    let _ = stream.shutdown(Shutdown::Both);
    drop(slot);
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.hub.forget(self.id);
        if self.kind.is_some() && self.tier == Tier::App {
            self.shared
                .presence
                .on_close(self.at_accept.pid, Instant::now());
        }
    }
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
    at_accept: PeerInfo,
    at_hello: Option<PeerInfo>,
    probe: Stream,
    kind: Option<ClientKind>,
    tx: SyncSender<Vec<u8>>,
    shared: Arc<Shared>,
    accepted: Instant,
    reserved: bool,
    subscribed: bool,
    last_request: Instant,
    agent_session: Option<String>,
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
                Err(FrameError::TooLong | FrameError::PartialAtEof | FrameError::Io(_)) => return,
            }
            if self.kind.is_none() && self.accepted.elapsed() > limits.hello_within {
                return;
            }
            if self.kind.is_some()
                && !self.subscribed
                && self.last_request.elapsed() > limits.idle_within
            {
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
        self.reply_with(id, result, Map::new())
    }

    fn reply_with(
        &self,
        id: u64,
        result: Result<Value, ApiError>,
        extras: Map<String, Value>,
    ) -> bool {
        match result {
            Ok(value) => {
                let mut line = ok_reply(id, value);
                if let Value::Object(fields) = &mut line {
                    for (key, value) in extras {
                        if !matches!(key.as_str(), "id" | "ok" | "err") {
                            fields.entry(key).or_insert(value);
                        }
                    }
                }
                self.send(&line)
            }
            Err(e) => self.send(&err_reply(id, &e)),
        }
    }

    fn on_line(&mut self, line: &[u8]) -> bool {
        self.last_request = Instant::now();
        let request = match Request::parse(line) {
            Ok(request) => request,
            Err((id, e)) => {
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
        let _scope = scope::enter(self.scope());
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
                let (result, extras) = self.dispatch(op, &request.args);
                self.reply_with(id, result, extras)
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
        if hello.client == ClientKind::Node {
            self.reply(
                id,
                Err(ApiError::bad_request("cluster nodes never use this socket")),
            );
            return false;
        }
        if hello.api != API_VERSION {
            let message = format!(
                "api {} is not served; this daemon speaks {API_VERSION}",
                hello.api
            );
            self.reply(id, Err(ApiError::bad_request(message)));
            return false;
        }
        let now = transport::peer(&self.probe).ok();
        self.tier = self
            .shared
            .allowlist
            .tier_for(&self.at_accept, now.as_ref());
        let listed = |p: Option<&PeerInfo>| {
            p.is_some_and(|p| self.shared.allowlist.tier_of(p.exe.as_deref()) == Tier::App)
        };
        let refused =
            self.tier != Tier::App && (listed(Some(&self.at_accept)) || listed(now.as_ref()));
        self.at_hello = now;
        if refused {
            self.shared.refused(
                ops::HELLO,
                Some(self.tier),
                self.peer(),
                RefusedWhy::PeerChanged,
                None,
            );
        }
        if self.reserved && self.tier != Tier::App {
            return false;
        }
        if self.tier == Tier::Agent {
            let pid = self.at_hello.as_ref().map_or(self.at_accept.pid, |p| p.pid);
            self.agent_session = self.shared.handler.session_of(pid);
        }
        if self.tier == Tier::App {
            let attached = self.shared.presence.on_hello(
                self.at_accept.pid,
                &hello.version,
                self.peer(),
                Instant::now(),
            );
            if let Some(attached) = attached {
                self.shared.push_event(&attached, Outcome::Info);
            }
        }
        self.kind = Some(hello.client);
        self.shared.hub.hello(self.id, hello.client, self.tier);
        let reply = HelloReply {
            api: API_VERSION,
            daemon: self.shared.version.clone(),
            tier: self.tier,
        };
        let value = serde_json::to_value(reply).map_err(|e| ApiError::internal(e.to_string()));
        self.reply(id, value)
    }

    fn subscribe(&mut self, args: &Value) -> Result<Value, ApiError> {
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
        let app_only = subscribe
            .topics
            .iter()
            .any(|t| self.shared.handler.topic_tier(t) == Tier::App);
        if app_only && (self.tier != Tier::App || !self.still_the_hello_process()) {
            let result = if self.tier == Tier::App {
                "peer_changed"
            } else {
                "forbidden"
            };
            let why = if result == "peer_changed" {
                RefusedWhy::PeerChanged
            } else {
                RefusedWhy::Forbidden
            };
            self.shared.refused(
                ops::EVENTS_SUBSCRIBE,
                Some(self.tier),
                self.peer(),
                why,
                None,
            );
            return Err(ApiError::new(
                ErrorCode::Forbidden,
                "only the Notchling app may follow this topic",
            ));
        }
        self.shared.hub.subscribe(self.id, &subscribe.topics);
        self.subscribed |= !subscribe.topics.is_empty();
        Ok(serde_json::json!({ "topics": subscribe.topics }))
    }

    fn dispatch(&self, op: &str, args: &Value) -> (Result<Value, ApiError>, Map<String, Value>) {
        let mut extras = Map::new();
        let result = self.dispatch_into(op, args, &mut extras);
        (result, extras)
    }

    fn dispatch_into(
        &self,
        op: &str,
        args: &Value,
        extras: &mut Map<String, Value>,
    ) -> Result<Value, ApiError> {
        let handler = &self.shared.handler;
        let Some(spec) = handler.spec(op) else {
            return Err(ApiError::new(
                ErrorCode::UnknownOp,
                format!("no op named {op}"),
            ));
        };
        if spec.tier == Tier::App && self.tier != Tier::App {
            self.shared.refused(
                op,
                Some(self.tier),
                self.peer(),
                RefusedWhy::Forbidden,
                None,
            );
            return Err(ApiError::new(
                ErrorCode::Forbidden,
                "only the Notchling app may do this",
            ));
        }
        if spec.tier == Tier::App && !self.still_the_hello_process() {
            self.shared.refused(
                op,
                Some(self.tier),
                self.peer(),
                RefusedWhy::PeerChanged,
                None,
            );
            return Err(ApiError::new(
                ErrorCode::Forbidden,
                "this request didn't come from the process that said hello",
            ));
        }
        if spec.approval && !self.shared.hub.app_attached() {
            self.shared.refused(
                op,
                Some(self.tier),
                self.peer(),
                RefusedWhy::NeedsApproval,
                None,
            );
            return Err(ApiError::new(
                ErrorCode::NeedsApproval,
                "this needs approval in the Notchling app, and the app isn't attached",
            ));
        }
        let caller = Caller {
            tier: self.tier,
            client: self.kind.unwrap_or(ClientKind::Cli),
            peer: self.peer().map(Path::to_path_buf),
            pid: transport::peer(&self.probe).ok().map(|p| p.pid),
        };
        let started = Instant::now();
        let (result, more) = handler.call_with_extras(&caller, op, args);
        *extras = more;
        if !scope::take_recorded() {
            self.record_generic(op, args, &spec, &result, started);
        }
        result
    }

    fn record_generic(
        &self,
        op: &str,
        args: &Value,
        spec: &crate::server::OpSpec,
        result: &Result<Value, ApiError>,
        started: Instant,
    ) {
        let handler = &self.shared.handler;
        match result {
            Err(e) if e.code == ErrorCode::Forbidden => self.shared.refused(
                op,
                Some(self.tier),
                self.peer(),
                RefusedWhy::Forbidden,
                None,
            ),
            Err(e) if e.code == ErrorCode::NeedsApproval => self.shared.refused(
                op,
                Some(self.tier),
                self.peer(),
                RefusedWhy::NeedsApproval,
                None,
            ),
            Err(e) => self.shared.failed(op, e.code, started),
            Ok(_) if spec.tier == Tier::App => {
                let called = match handler.home(op) {
                    OpHome::Called => true,
                    OpHome::Quiet => handler.loud(op, args),
                    OpHome::Kind => false,
                };
                if called {
                    self.shared.called(op, self.tier, started);
                }
            }
            Ok(_) => {}
        }
    }

    fn scope(&self) -> Scope {
        let via = match self.kind {
            Some(ClientKind::App) => Some(Via::App),
            Some(ClientKind::Cli) => Some(Via::Cli),
            Some(ClientKind::Hook) => Some(Via::Hook),
            Some(ClientKind::Node) | None => None,
        };
        let tier = match self.tier {
            Tier::App => ActorTier::App,
            Tier::Agent => ActorTier::Agent,
        };
        let session = match self.tier {
            Tier::Agent => self.agent_session.as_deref().map(Ref::new),
            Tier::App => None,
        };
        Scope {
            op_id: Some(notchling_events::ids::new_op_id()),
            actor: Some(Actor {
                tier,
                via,
                exe: self.peer().map(Exe::from_path),
                device: None,
                session,
            }),
        }
    }

    fn peer(&self) -> Option<&Path> {
        self.at_hello
            .as_ref()
            .unwrap_or(&self.at_accept)
            .exe
            .as_deref()
    }

    fn still_the_hello_process(&self) -> bool {
        match (&self.at_hello, transport::peer(&self.probe)) {
            (Some(at_hello), Ok(now)) => at_hello.same_process(&now),
            _ => false,
        }
    }
}
