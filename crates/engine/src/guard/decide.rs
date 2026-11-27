use super::event::{Event, Kind};
use super::rules::{stateless, Action, Limits, Rule, Would};
use super::state::with_session;
use std::path::Path;

pub struct Env<'a> {
    pub now: f64,
    pub dir: &'a Path,
    pub limits: Limits,
    pub size_of: &'a dyn Fn(&Path) -> Option<u64>,
}

pub fn file_size(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    meta.is_file().then_some(meta.len())
}

fn stateful(event: &Event, env: &Env) -> Vec<Would> {
    let spawn = event.spawns().then(|| event.tool_use_id.clone()).flatten();
    if spawn.is_none() && event.agent.is_none() {
        return Vec::new();
    }
    let limits = env.limits;
    with_session(env.dir, &event.session, |held| {
        let mut found = Vec::new();
        if let Some(id) = &spawn {
            if held.reserve(id, env.now, limits.cap) {
                found.push(Would {
                    rule: Rule::Cap,
                    action: Action::Block,
                    reason: "builders-at-cap",
                });
            }
        }
        if let Some(agent) = &event.agent {
            if held.count(agent, env.now) == limits.size_calls + 1 {
                found.push(Would {
                    rule: Rule::Size,
                    action: Action::Note,
                    reason: "helper-long-run",
                });
            }
        }
        found
    })
    .unwrap_or_default()
}

pub fn decide(event: &Event, env: &Env) -> Vec<Would> {
    match event.kind {
        Kind::Pre => {
            let mut out = stateless(event, &env.limits, env.size_of);
            out.extend(stateful(event, env));
            out
        }
        Kind::SubagentStart => {
            if let Some(agent) = &event.agent {
                let id = event.tool_use_id.as_deref();
                with_session(env.dir, &event.session, |held| {
                    held.claim(agent, env.now, id)
                });
            }
            Vec::new()
        }
        Kind::SubagentStop => {
            if let Some(agent) = &event.agent {
                with_session(env.dir, &event.session, |held| held.release(agent));
            }
            Vec::new()
        }
    }
}

pub fn record(input: &[u8], dir: &Path, now: f64) -> Vec<Would> {
    let Some(limits) = super::policy::read(dir) else {
        return Vec::new();
    };
    let Some(event) = super::event::parse(input) else {
        return Vec::new();
    };
    let env = Env {
        now,
        dir,
        limits,
        size_of: &file_size,
    };
    let woulds = decide(&event, &env);
    super::spool::append(dir, now, &event, &woulds);
    woulds
}
