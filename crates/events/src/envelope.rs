use crate::event::Event;
use crate::field::{Exe, Hex, LogField};
use crate::field_ref::{Device, Ref, RefMode, Session};
use crate::spec::Proc;
use crate::value::{write_str, write_value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorTier {
    App,
    Agent,
    Daemon,
    Phone,
}

impl ActorTier {
    pub fn name(self) -> &'static str {
        match self {
            ActorTier::App => "app",
            ActorTier::Agent => "agent",
            ActorTier::Daemon => "daemon",
            ActorTier::Phone => "phone",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    App,
    Cli,
    Hook,
    Bridge,
}

impl Via {
    pub fn name(self) -> &'static str {
        match self {
            Via::App => "app",
            Via::Cli => "cli",
            Via::Hook => "hook",
            Via::Bridge => "bridge",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Actor {
    pub tier: ActorTier,
    pub via: Option<Via>,
    pub exe: Option<Exe>,
    pub device: Option<Ref<Device>>,
    pub session: Option<Ref<Session>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
    pub ts_rx: Option<i64>,
    pub seq: u64,
    pub host: Hex,
    pub proc: Proc,
    pub pid: u32,
    pub boot: Hex,
    pub op_id: Option<Hex>,
    pub actor: Option<Actor>,
    pub repeat: Option<u32>,
    pub spooled: bool,
}

fn key(out: &mut String, name: &str) {
    write_str(out, name);
    out.push(':');
}

fn write_actor(actor: &Actor, mode: &RefMode, out: &mut String) {
    out.push('{');
    key(out, "tier");
    write_str(out, actor.tier.name());
    if let Some(via) = actor.via {
        out.push(',');
        key(out, "via");
        write_str(out, via.name());
    }
    if let Some(exe) = &actor.exe {
        out.push(',');
        key(out, "exe");
        write_str(out, exe.as_str());
    }
    if let Some(device) = &actor.device {
        out.push(',');
        key(out, "device");
        write_value(out, &device.value(), mode);
    }
    if let Some(session) = &actor.session {
        out.push(',');
        key(out, "session");
        write_value(out, &session.value(), mode);
    }
    out.push('}');
}

pub fn write_body(env: &Envelope, event: &Event, mode: &RefMode, out: &mut String) {
    out.push_str("\"v\":1,");
    key(out, "kind");
    write_str(out, event.spec.name);
    out.push(',');
    key(out, "kv");
    out.push_str(&event.spec.kv.to_string());
    out.push(',');
    key(out, "ts");
    out.push_str(&event.ts.to_string());
    if let Some(ts_rx) = env.ts_rx {
        out.push(',');
        key(out, "ts_rx");
        out.push_str(&ts_rx.to_string());
    }
    out.push(',');
    key(out, "seq");
    out.push_str(&env.seq.to_string());
    out.push(',');
    key(out, "host");
    write_str(out, env.host.as_str());
    out.push(',');
    key(out, "proc");
    write_str(out, env.proc.name());
    out.push(',');
    key(out, "pid");
    out.push_str(&env.pid.to_string());
    out.push(',');
    key(out, "boot");
    write_str(out, env.boot.as_str());
    if let Some(op_id) = &env.op_id {
        out.push(',');
        key(out, "op_id");
        write_str(out, op_id.as_str());
    }
    if let Some(actor) = &env.actor {
        out.push(',');
        key(out, "actor");
        write_actor(actor, mode, out);
    }
    out.push(',');
    key(out, "outcome");
    write_str(out, event.outcome.name());
    if let Some(repeat) = env.repeat {
        out.push(',');
        key(out, "repeat");
        out.push_str(&repeat.to_string());
    }
    if env.spooled {
        out.push_str(",\"spooled\":true");
    }
    out.push(',');
    key(out, "f");
    event.fields.write_json(out, mode);
}
