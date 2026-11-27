use crate::cluster_relay::{HeldRelay, Limits, Parts, Served, SetView, Wire, SEND_WAIT};
use crate::limits_ops_tests::TempDir;
use notchling_engine::bridge::model::{
    Control, PendingPrompt, PromptKind, PromptQuestion, Session, SessionState,
};
use notchling_engine::bridge::relay::{Call, HeldAct, Relay, Routed};
use notchling_engine::cluster::wire::WireError;
use notchling_engine::cluster::NodeId;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) const M5: &str = "0123456789abcdef";
pub(crate) const M1: &str = "fedcba9876543210";
pub(crate) const W: &str = "1111222233334444";
pub(crate) const M2: &str = "aaaabbbbccccdddd";
pub(crate) const T0: i64 = 1_800_000_000_000;
pub(crate) const HOUR: i64 = 3_600_000;
pub(crate) const DEV: &str = "phone-1";
pub(crate) const OTHER: &str = "phone-2";

pub(crate) type Hang = (Sender<()>, Receiver<()>);

pub(crate) struct Net {
    pub(crate) status: Mutex<Value>,
    pub(crate) sent: Mutex<Vec<(String, String, Value)>>,
    pub(crate) reply: Mutex<Result<Value, WireError>>,
    pub(crate) hang: Mutex<Option<Hang>>,
}

impl Wire for Net {
    fn status(&self) -> Value {
        self.status.lock().unwrap().clone()
    }

    fn call(
        &self,
        to: &NodeId,
        op: &str,
        args: Value,
        within: Duration,
    ) -> Result<Value, WireError> {
        assert_eq!(within, SEND_WAIT);
        self.sent
            .lock()
            .unwrap()
            .push((to.as_str().to_string(), op.to_string(), args));
        let hang = self.hang.lock().unwrap().take();
        if let Some((entered, release)) = hang {
            let _ = entered.send(());
            let _ = release.recv_timeout(Duration::from_secs(60));
        }
        self.reply.lock().unwrap().clone()
    }
}

pub(crate) struct Set {
    pub(crate) newest: Mutex<Option<i64>>,
    pub(crate) kept: PathBuf,
}

impl SetView for Set {
    fn newest_ms(&self) -> Option<i64> {
        *self.newest.lock().unwrap()
    }

    fn transcript(&self, original: &Path) -> Option<PathBuf> {
        (original == Path::new("/Users/example/.claude/projects/app/job-7.jsonl"))
            .then(|| self.kept.clone())
    }
}

pub(crate) fn member(id: &str, name: &str, role: &str, can: bool, online: bool) -> Value {
    json!({
        "id": id, "name": name, "role": role, "canBeMaster": can, "online": online,
        "client": true, "headless": false, "reference": false, "state": "active",
        "addrs": [format!("10.0.0.{}", name.len())], "fingerprint": format!("{id}{id}{id}{id}"),
        "versions": {"bridge": {"port": 47900, "fingerprint": format!("b{id}b{id}b{id}b{id}")}},
    })
}

pub(crate) fn status(me: &str, home: &str, online: bool) -> Value {
    let role = |id: &str, standby: &'static str| -> &'static str {
        if id == home {
            "home"
        } else {
            standby
        }
    };
    json!({
        "cluster": {"id": "c", "name": "c", "term": {"n": 3, "node": home}, "home": home},
        "me": me,
        "members": [
            member(M5, "m5", role(M5, "standby"), true, online || me == M5),
            member(M1, "m1", role(M1, "standby"), true, true),
            member(M2, "m2", role(M2, "standby"), true, true),
            member(W, "worker", "worker", false, true),
        ],
    })
}

pub(crate) struct Rig {
    pub(crate) _dir: TempDir,
    pub(crate) dir: PathBuf,
    pub(crate) net: Arc<Net>,
    pub(crate) set: Arc<Set>,
    pub(crate) clock: Arc<AtomicI64>,
    pub(crate) rows: Arc<Mutex<Vec<String>>>,
    pub(crate) limits: Limits,
    pub(crate) relay: Arc<HeldRelay>,
}

pub(crate) struct Env<'a> {
    pub(crate) dir: &'a Path,
    pub(crate) net: &'a Arc<Net>,
    pub(crate) set: &'a Arc<Set>,
    pub(crate) clock: &'a Arc<AtomicI64>,
    pub(crate) rows: &'a Arc<Mutex<Vec<String>>>,
    pub(crate) limits: Limits,
}

pub(crate) fn parts(env: Env<'_>) -> Parts {
    let clock = env.clock.clone();
    let rows = env.rows.clone();
    Parts {
        wire: env.net.clone(),
        dir: env.dir.to_path_buf(),
        clock: Arc::new(move || clock.load(Ordering::SeqCst)),
        prompts: Arc::new(|id| {
            (id == format!("{M5}/p1")).then(|| PendingPrompt {
                id: id.to_string(),
                session_id: format!("{M5}/job-7"),
                kind: PromptKind::Question(vec![PromptQuestion {
                    header: "Pick".into(),
                    question: "Which?".into(),
                    options: vec!["Red".into(), "Blue".into()],
                    multi_select: false,
                }]),
                tool_name: String::new(),
                summary: String::new(),
                created_ms: 0.0,
            })
        }),
        model: Arc::new(|short| (short == "opus").then(|| "claude-opus-5-5".to_string())),
        rows: Arc::new(move || rows.lock().unwrap().clone()),
        set: env.set.clone(),
        bridge: Arc::new(|| Served {
            port: 47821,
            fingerprint: Some("5".repeat(64)),
        }),
        limits: env.limits,
    }
}

pub(crate) fn rig(me: &str, home: &str, online: bool) -> Rig {
    rig_with(me, home, online, Limits::DEFAULT)
}

pub(crate) fn rig_with(me: &str, home: &str, online: bool, limits: Limits) -> Rig {
    let dir = TempDir::new("relay");
    let cluster = dir.path("cluster");
    std::fs::create_dir_all(&cluster).unwrap();
    let net = Arc::new(Net {
        status: Mutex::new(status(me, home, online)),
        sent: Mutex::new(Vec::new()),
        reply: Mutex::new(Ok(json!({"ok": true}))),
        hang: Mutex::new(None),
    });
    let set = Arc::new(Set {
        newest: Mutex::new(Some(T0 - 90_000)),
        kept: dir.path("kept.jsonl"),
    });
    let clock = Arc::new(AtomicI64::new(T0));
    let rows = Arc::new(Mutex::new(vec![
        format!("{M5}/job-7"),
        format!("{W}/job-3"),
    ]));
    let relay = Arc::new(HeldRelay::new(parts(Env {
        dir: &cluster,
        net: &net,
        set: &set,
        clock: &clock,
        rows: &rows,
        limits,
    })));
    Rig {
        _dir: dir,
        dir: cluster,
        net,
        set,
        clock,
        rows,
        limits,
        relay,
    }
}

pub(crate) fn reborn(rig: &Rig) -> HeldRelay {
    HeldRelay::new(parts(Env {
        dir: &rig.dir,
        net: &rig.net,
        set: &rig.set,
        clock: &rig.clock,
        rows: &rig.rows,
        limits: rig.limits,
    }))
}

pub(crate) fn session(id: &str) -> Session {
    Session::new(
        id,
        "job",
        "/Users/example/src/app",
        Control::Job(id.to_string()),
        SessionState::Done,
    )
}

pub(crate) fn route_as(
    rig: &Rig,
    device: &str,
    request: &str,
    key: Option<&str>,
    session_id: &str,
    text: &str,
) -> Routed {
    let act = HeldAct::Message {
        session_id: session_id.to_string(),
        text: text.to_string(),
    };
    let found = session(session_id);
    rig.relay.route(&Call {
        device,
        request,
        key,
        act: &act,
        session: Some(&found),
    })
}

pub(crate) fn message(rig: &Rig, request: &str, session_id: &str, text: &str) -> Routed {
    route_as(rig, DEV, request, None, session_id, text)
}

pub(crate) fn keyed(rig: &Rig, key: &str, session_id: &str, text: &str) -> Routed {
    route_as(rig, DEV, "7", Some(key), session_id, text)
}

pub(crate) fn sent(rig: &Rig) -> Vec<(String, String, Value)> {
    rig.net.sent.lock().unwrap().clone()
}

pub(crate) fn texts(rig: &Rig) -> Vec<Value> {
    sent(rig).iter().map(|s| s.2["text"].clone()).collect()
}

pub(crate) fn go(rig: &Rig, me: &str, home: &str, online: bool) {
    *rig.net.status.lock().unwrap() = status(me, home, online);
}

pub(crate) fn unreachable_error() -> WireError {
    WireError::new("offline", "m5 is offline")
}

pub(crate) fn held_id(routed: Routed) -> String {
    match routed {
        Routed::Held(held) => held.id,
        other => panic!("held was expected, got {other:?}"),
    }
}

pub(crate) fn summaries(rig: &Rig) -> Vec<String> {
    rig.relay
        .list(DEV)
        .entries
        .iter()
        .map(|e| e.summary.clone())
        .collect()
}
