use crate::cluster_node::LocalCalls;
use crate::cluster_testkit::{eventually, online, pair, Opts};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::core::WAITING;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::{Job, JobSpec, JobState, LocalCmd, Tool};
use notchling_local_api::ApiError;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const JOBS: usize = 6;

#[derive(Default)]
struct Held {
    hold_revoke: AtomicBool,
    hold_attach: AtomicBool,
    revoked: AtomicBool,
    attaches: AtomicUsize,
}

fn spin(flag: &AtomicBool) {
    while flag.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(10));
    }
}

impl LocalCalls for Held {
    fn call(&self, op: &str, _args: &Value, _origin: &Origin) -> Result<Value, ApiError> {
        match op {
            "delegate.run" => Ok(json!({"exit": 0, "out": "", "err": ""})),
            "grant.revoke_job" => {
                spin(&self.hold_revoke);
                self.revoked.store(true, Ordering::SeqCst);
                Ok(json!({}))
            }
            "grant.attach" => {
                self.attaches.fetch_add(1, Ordering::SeqCst);
                spin(&self.hold_attach);
                Ok(json!({"waiting": "the vault is elsewhere"}))
            }
            _ => Ok(json!({})),
        }
    }
}

fn block_done_rows(db: &std::path::Path) {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER no_done BEFORE INSERT ON journal
         WHEN NEW.command LIKE '%\"state\":\"done\"%'
         BEGIN SELECT RAISE(ABORT, 'blocked'); END;",
    )
    .unwrap();
}

fn secret_jobs(a: &crate::cluster_testkit::TestNode, want: &dyn Fn(&Job) -> bool) -> bool {
    let state = a.node.state().unwrap();
    state
        .jobs
        .values()
        .filter(|j| j.spec.secrets.len() == 1)
        .all(want)
}

#[test]
fn a_call_that_outlives_a_failed_save_cannot_complete_a_new_attach() {
    let net = MemNet::new();
    let held = Arc::new(Held::default());
    let mut home = Opts::named("home");
    home.client = true;
    home.os = "macos";
    home.tools = vec![(notchling_engine::versions::Tool::Agy, "1.5.1")];
    home.local = Some(held.clone());
    let (a, b) = pair(&net, home, Opts::named("worker"));
    let allow = LocalCmd::SetAllow {
        node: b.id(),
        names: Some(vec!["gh".to_string()]),
        any: None,
    };
    a.node.local(allow).unwrap();
    block_done_rows(&a.cluster_dir().join("cluster.db"));
    held.hold_revoke.store(true, Ordering::SeqCst);
    held.hold_attach.store(true, Ordering::SeqCst);
    let delegate = JobSpec {
        node: a.id(),
        tool: Tool::Agy,
        cwd: "/srv".to_string(),
        args: json!({"delegate": ["agy", "--json", "--", "hi"]}),
        secrets: Vec::new(),
    };
    let first = LocalCmd::Dispatch {
        key: "d1".to_string(),
        spec: delegate,
        tier: OriginTier::App,
    };
    let err = a.node.local(first).unwrap_err();
    assert!(err.message.contains("saving the cluster failed"), "{err:?}");
    eventually("the worker is online again", || {
        online(&a.node.status(), &b.id())
    });
    let mut threads = Vec::new();
    for n in 0..JOBS {
        let node = a.node.clone();
        let spec = JobSpec {
            node: b.id(),
            tool: Tool::Shell,
            cwd: "/srv".to_string(),
            args: json!({"command": "true"}),
            secrets: vec!["gh".to_string()],
        };
        let cmd = LocalCmd::Dispatch {
            key: format!("k{n}"),
            spec,
            tier: OriginTier::App,
        };
        threads.push(std::thread::spawn(move || node.local(cmd)));
    }
    eventually("every attach waits for its approval", || {
        held.attaches.load(Ordering::SeqCst) >= JOBS
    });
    held.hold_revoke.store(false, Ordering::SeqCst);
    eventually("the old revoke finishes", || {
        held.revoked.load(Ordering::SeqCst)
    });
    std::thread::sleep(Duration::from_millis(300));
    let fresh = |j: &Job| j.state == JobState::Dispatching && j.detail.is_none();
    assert!(secret_jobs(&a, &fresh), "a stale call moved a job on");
    held.hold_attach.store(false, Ordering::SeqCst);
    for thread in threads {
        thread.join().unwrap().unwrap();
    }
    let waiting = |j: &Job| {
        j.state == JobState::Dispatching
            && j.detail.as_deref().is_some_and(|d| d.starts_with(WAITING))
    };
    assert!(secret_jobs(&a, &waiting), "an attach result was lost");
}
