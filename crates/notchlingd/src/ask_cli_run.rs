use crate::acting_ops::now_ms;
use crate::ask_cli::{
    answer, ask, asks, parse_answer, parse_ask, parse_asks, AskLink, Env, LinkError,
};
use crate::contract::{self, Failure};
use crate::layout::Layout;
use notchling_engine::ask::DEADLINE_VARIABLE;
use notchling_local_api::{Client, ClientError, ClientKind};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

struct SocketLink {
    socket: PathBuf,
    client: Option<Client>,
}

fn link_error(e: ClientError) -> LinkError {
    match e {
        ClientError::Api(e) => LinkError::Api(e),
        ClientError::Unsupported | ClientError::UntrustedServer(_) => {
            LinkError::Fatal(format!("notchling: {e}"))
        }
        _ => LinkError::Down,
    }
}

impl SocketLink {
    fn connected(&mut self) -> Result<&mut Client, LinkError> {
        if self.client.is_none() {
            let client = Client::connect(&self.socket, ClientKind::Cli, env!("CARGO_PKG_VERSION"))
                .map_err(|e| match e {
                    ClientError::Api(e) => LinkError::Fatal(e.message),
                    other => link_error(other),
                })?;
            self.client = Some(client);
        }
        self.client.as_mut().ok_or(LinkError::Down)
    }
}

impl AskLink for SocketLink {
    fn call(&mut self, op: &str, args: &Value, timeout: Duration) -> Result<Value, LinkError> {
        let client = self.connected()?;
        client.set_timeout(timeout);
        match client.call(op, args.clone()) {
            Ok(reply) => Ok(reply),
            Err(e) => {
                let mapped = link_error(e);
                if !matches!(mapped, LinkError::Api(_)) {
                    self.client = None;
                }
                Err(mapped)
            }
        }
    }

    fn reconnect(&mut self) -> Result<(), LinkError> {
        self.client = None;
        self.connected().map(|_| ())
    }
}

fn fresh_key() -> String {
    notchling_engine::vault::random_id("").unwrap_or_else(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        format!("{nanos:016x}{:016x}", u64::from(std::process::id()))
    })
}

fn delegate_deadline() -> Option<i64> {
    std::env::var(DEADLINE_VARIABLE)
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
}

fn drive(
    socket: Option<PathBuf>,
    usage_error: Option<String>,
    json: bool,
    run: impl FnOnce(&mut SocketLink, &mut Env) -> i32,
) -> i32 {
    if let Some(message) = usage_error {
        return Failure::usage(message).report(json);
    }
    let Some(layout) = Layout::from_env(socket.as_deref()) else {
        return Failure::not_running().report(json);
    };
    let mut link = SocketLink {
        socket: layout.socket,
        client: None,
    };
    let now = now_ms;
    let sleep = |ms: u64| std::thread::sleep(Duration::from_millis(ms));
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let mut env = Env {
        now: &now,
        sleep: &sleep,
        delegate_deadline: delegate_deadline(),
        key: fresh_key(),
        out: &mut out,
        err: &mut err,
    };
    run(&mut link, &mut env)
}

pub fn run_ask(args: &[String], socket: Option<PathBuf>) -> i32 {
    let json = contract::wants_json(args);
    let parsed = parse_ask(args);
    let (socket, error) = match &parsed {
        Ok(p) => (p.socket.clone().or(socket), None),
        Err(message) => (socket, Some(message.clone())),
    };
    drive(socket, error, json, |link, env| match &parsed {
        Ok(p) => ask(link, p, env),
        Err(_) => contract::BAD_REQUEST,
    })
}

pub fn run_asks(args: &[String], socket: Option<PathBuf>) -> i32 {
    let json = contract::wants_json(args);
    let parsed = parse_asks(args);
    let (socket, error) = match &parsed {
        Ok(p) => (p.socket.clone().or(socket), None),
        Err(message) => (socket, Some(message.clone())),
    };
    drive(socket, error, json, |link, env| match &parsed {
        Ok(p) => asks(link, p, env),
        Err(_) => contract::BAD_REQUEST,
    })
}

pub fn run_answer(args: &[String], socket: Option<PathBuf>) -> i32 {
    let json = contract::wants_json(args);
    let parsed = parse_answer(args);
    let (socket, error) = match &parsed {
        Ok(p) => (p.socket.clone().or(socket), None),
        Err(message) => (socket, Some(message.clone())),
    };
    drive(socket, error, json, |link, env| match &parsed {
        Ok(p) => answer(link, p, env),
        Err(_) => contract::BAD_REQUEST,
    })
}
