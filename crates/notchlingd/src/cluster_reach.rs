use crate::cluster_cli::{Sub, DONE, USAGE};
use crate::cluster_wiring::{loopback_ok, LOOPBACK_ENV};
use crate::contract::Failure;
use notchling_cluster_net::allowed;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

pub const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Reached,
    NoAnswer,
    Refused,
    Failed(String),
}

pub fn parse(addr: &str, port: &str) -> Result<Sub, String> {
    let addr: Ipv4Addr = addr
        .parse()
        .map_err(|_| format!("{addr} is not an IPv4 address.\n{USAGE}"))?;
    let port: u16 = match port.parse() {
        Ok(port) if port != 0 => port,
        _ => return Err(format!("{port} is not a port number.\n{USAGE}")),
    };
    Ok(Sub::Reach { addr, port })
}

pub fn check_addr(addr: Ipv4Addr, loopback: bool) -> Result<(), String> {
    if allowed(&IpAddr::V4(addr), loopback) {
        Ok(())
    } else {
        Err(format!(
            "{addr} is not a Tailscale address (100.64.0.0/10)."
        ))
    }
}

pub fn reach_with<C>(addr: Ipv4Addr, port: u16, timeout: Duration, connect: C) -> Outcome
where
    C: FnOnce(&SocketAddr, Duration) -> std::io::Result<()>,
{
    match connect(&SocketAddr::new(IpAddr::V4(addr), port), timeout) {
        Ok(()) => Outcome::Reached,
        Err(e) => match e.kind() {
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => Outcome::NoAnswer,
            std::io::ErrorKind::ConnectionRefused => Outcome::Refused,
            _ => Outcome::Failed(e.to_string()),
        },
    }
}

pub fn tcp_connect(target: &SocketAddr, timeout: Duration) -> std::io::Result<()> {
    TcpStream::connect_timeout(target, timeout).map(drop)
}

pub fn reach(addr: Ipv4Addr, port: u16, timeout: Duration) -> Outcome {
    reach_with(addr, port, timeout, tcp_connect)
}

pub fn report(
    addr: Ipv4Addr,
    port: u16,
    outcome: &Outcome,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match outcome {
        Outcome::Reached => {
            let _ = writeln!(out, "reached {addr}:{port}");
            DONE
        }
        Outcome::NoAnswer => Failure::failed("")
            .with_human(format!("no answer in {} s", TIMEOUT.as_secs()))
            .emit(false, out, err),
        Outcome::Refused => Failure::failed("")
            .with_human("refused")
            .emit(false, out, err),
        Outcome::Failed(why) => {
            let why = why.lines().next().unwrap_or("");
            Failure::failed("")
                .with_human(format!("no connection: {why}"))
                .emit(false, out, err)
        }
    }
}

pub fn run_in(
    addr: Ipv4Addr,
    port: u16,
    loopback: bool,
    timeout: Duration,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    if let Err(message) = check_addr(addr, loopback) {
        return Failure::usage(format!("{message}\n{USAGE}")).emit(false, out, err);
    }
    report(addr, port, &reach(addr, port, timeout), out, err)
}

pub fn run(addr: Ipv4Addr, port: u16, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let exe = std::env::current_exe().ok();
    let loopback = loopback_ok(std::env::var_os(LOOPBACK_ENV).as_deref(), exe.as_deref());
    run_in(addr, port, loopback, TIMEOUT, out, err)
}
