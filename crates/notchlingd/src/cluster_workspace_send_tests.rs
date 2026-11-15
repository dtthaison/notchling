use super::{code_mode, Code, NodePeers, ReturnOps, Sender};
use crate::acting_ops_tests::app;
use crate::cluster_blobs::Blobs;
use crate::cluster_dispatch::{DispatchOps, NodeReporter};
use crate::cluster_node::LocalCalls;
use crate::cluster_testkit::{eventually, node, online, pair, Dir, FakeCalls, Opts, TestNode};
use crate::cluster_workspace::{Workspace, WorkspaceCalls};
use crate::cluster_workspace_testkit::{git_in, Fixture, Isolated};
use crate::op_group::OpGroup;
use crate::shell_jobs::ShellJobs;
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::jobs::job_id;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::LocalCmd;
use notchling_engine::control::Launcher;
use notchling_engine::versions::Tool as VTool;
use notchling_local_api::{Caller, ClientKind, Tier};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

const BOTH: [(VTool, &str); 2] = [(VTool::Claude, "2.1.324"), (VTool::Agy, "1.5.1")];

fn agent() -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid: None,
    }
}

struct Rig {
    fx: Fixture,
    fake: Arc<FakeCalls>,
}

fn opts(name: &'static str, os: &'static str, client: bool) -> Opts {
    let mut opts = Opts::named(name);
    opts.client = client;
    opts.os = os;
    opts.tools = BOTH.to_vec();
    opts
}

fn worker_opts(rig: &Rig) -> Opts {
    let cluster = rig.fx.worker_cluster.clone();
    let launcher: Arc<dyn Launcher> = Arc::new(Isolated::new(rig.fx.home.clone()));
    let workspace = Workspace::new(cluster.clone(), launcher, Blobs::new(cluster.clone()));
    let inner: Arc<dyn LocalCalls> = rig.fake.clone();
    let mut opts = opts("worker", "macos", false);
    opts.cluster_dir = Some(cluster);
    opts.local = Some(Arc::new(WorkspaceCalls::new(inner, workspace)));
    opts
}

fn rig(tag: &str) -> Rig {
    Rig {
        fx: Fixture::new(tag),
        fake: Arc::new(FakeCalls::default()),
    }
}

fn dispatch_ops(rig: &Rig, from: &TestNode) -> (Arc<DispatchOps>, Arc<ReturnOps>, Dir) {
    let dir = Dir::new("send-shells");
    let shells = ShellJobs::new(dir.path().to_path_buf(), Arc::new(NodeReporter::default()));
    let holder = Arc::new(OnceLock::new());
    let _ = holder.set(from.node.clone());
    let launcher: Arc<dyn Launcher> = Arc::new(Isolated::new(rig.fx.home.clone()));
    let sender = Sender::new(
        rig.fx.dir.path().join("dispatcher/cluster"),
        launcher,
        Arc::new(NodePeers::new(holder)),
    );
    let ops = DispatchOps::with_code(Ok(from.node.clone()), shells, Some(sender.clone()));
    (ops, ReturnOps::new(sender), dir)
}

fn send_args(worker: &TestNode, rig: &Rig, key: &str) -> Value {
    json!({
        "node": worker.id(),
        "tool": "claude",
        "cwd": rig.fx.repo_str(),
        "key": key,
        "code": "send",
    })
}

fn folder_of(rig: &Rig) -> String {
    let calls = rig.fake.calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].0, "session.new");
    calls[0].1["folder"].as_str().unwrap().to_string()
}

#[test]
fn code_modes() {
    assert_eq!(code_mode(&json!({})).unwrap(), Code::Folder);
    assert_eq!(code_mode(&json!({"code": null})).unwrap(), Code::Folder);
    assert_eq!(code_mode(&json!({"code": "folder"})).unwrap(), Code::Folder);
    assert_eq!(code_mode(&json!({"code": "send"})).unwrap(), Code::Send);
    assert!(code_mode(&json!({"code": "zip"})).is_err());
    assert!(code_mode(&json!({"code": 1})).is_err());
}

#[test]
fn dispatch_with_send_starts_the_session_in_a_worktree() {
    let rig = rig("send-home");
    rig.fx.commit("a.txt", "two\n", "two");
    let net = MemNet::new();
    let (a, b) = pair(&net, opts("home", "macos", true), worker_opts(&rig));
    let (ops, _returns, _dir) = dispatch_ops(&rig, &a);
    let reply = ops
        .call(&app(), "cluster.dispatch", &send_args(&b, &rig, "s1"))
        .unwrap();
    assert_eq!(reply["state"], "running", "{reply}");
    let folder = folder_of(&rig);
    assert!(Path::new(&folder).starts_with(rig.fx.worker_cluster.join("work/jobs")));
    assert_eq!(
        std::fs::read_to_string(Path::new(&folder).join("a.txt")).unwrap(),
        "two\n"
    );
    assert!(folder.ends_with(&reply["job"].as_str().unwrap().to_string()));
    let again = ops
        .call(&app(), "cluster.dispatch", &send_args(&b, &rig, "s1"))
        .unwrap();
    assert_eq!(again["job"], reply["job"]);
}

#[test]
fn dispatch_from_a_client_sends_code_straight_to_the_worker() {
    let rig = rig("send-client");
    rig.fx.commit("a.txt", "two\n", "two");
    let net = MemNet::new();
    let (a, b) = pair(&net, opts("home", "macos", true), worker_opts(&rig));
    let c = node(&net, opts("laptop", "macos", true));
    c.node.set_link(true);
    let token = a.node.join_token().unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(c.node.join(&token, false).unwrap()["state"], "pending");
    a.node
        .local(LocalCmd::Approve {
            node: c.id(),
            client: true,
            can_be_master: false,
        })
        .unwrap();
    eventually("the client is online", || {
        online(&a.node.status(), &c.id()) && online(&c.node.status(), &a.id())
    });
    let (ops, returns, _dir) = dispatch_ops(&rig, &c);
    let reply = ops
        .call(&app(), "cluster.dispatch", &send_args(&b, &rig, "s2"))
        .unwrap();
    let folder = folder_of(&rig);
    assert!(Path::new(&folder).join("a.txt").is_file());
    let tree = PathBuf::from(&folder);
    std::fs::write(tree.join("w.txt"), "w").unwrap();
    git_in(&rig.fx.home, Some(&tree), &["add", "w.txt"]);
    git_in(&rig.fx.home, Some(&tree), &["commit", "-q", "-m", "w"]);
    let job = reply["job"].as_str().unwrap().to_string();
    let back = returns
        .call(&app(), "cluster.return", &json!({"job": job}))
        .unwrap();
    assert_eq!(back["branch"], format!("notchling/worker/{job}"));
    assert_eq!(
        rig.fx.git(
            &rig.fx.repo,
            &[
                "rev-parse",
                &format!("refs/heads/{}", back["branch"].as_str().unwrap())
            ]
        ),
        git_in(&rig.fx.home, Some(&tree), &["rev-parse", "HEAD"])
    );
}

#[test]
fn a_worker_cannot_send_code_to_another_node() {
    let rig = rig("send-worker");
    let net = MemNet::new();
    let (a, b) = pair(&net, opts("home", "macos", true), worker_opts(&rig));
    for op in [
        "node.put",
        "node.put.end",
        "workspace.prepare",
        "workspace.return",
    ] {
        for (tier, why) in [
            (OriginTier::App, "TierTooHigh"),
            (OriginTier::Node, "NotAllowed"),
            (OriginTier::Agent, "NotAllowed"),
        ] {
            let err = b
                .node
                .local(LocalCmd::Forward {
                    node: a.id(),
                    op: op.to_string(),
                    args: json!({}).into(),
                    origin: Origin { node: b.id(), tier },
                })
                .unwrap_err();
            assert_eq!(err.message, why, "{op} {tier:?}");
        }
    }
    assert!(a.calls.calls.lock().unwrap().is_empty());
}

#[test]
fn folder_and_no_code_leave_the_folder_alone() {
    let rig = rig("send-folder");
    let net = MemNet::new();
    let (a, b) = pair(&net, opts("home", "macos", true), worker_opts(&rig));
    let (ops, _returns, _dir) = dispatch_ops(&rig, &a);
    let mut args = send_args(&b, &rig, "f1");
    args["code"] = json!("folder");
    ops.call(&app(), "cluster.dispatch", &args).unwrap();
    assert_eq!(folder_of(&rig), rig.fx.repo_str());
    assert!(!rig.fx.worker_cluster.join("work").exists());
    let mut bad = send_args(&b, &rig, "f2");
    bad["code"] = json!("zip");
    assert!(ops.call(&app(), "cluster.dispatch", &bad).is_err());
}

#[test]
fn only_the_app_sends_code() {
    let rig = rig("send-agent");
    let net = MemNet::new();
    let (a, b) = pair(&net, opts("home", "macos", true), worker_opts(&rig));
    let (ops, returns, _dir) = dispatch_ops(&rig, &a);
    let err = ops
        .call(&agent(), "cluster.dispatch", &send_args(&b, &rig, "g1"))
        .unwrap_err();
    assert!(
        err.message.contains("only the Notchling app"),
        "{}",
        err.message
    );
    let delegate = json!({
        "node": b.id(),
        "key": "g2",
        "code": "send",
        "delegate": ["agy", "--folder", rig.fx.repo_str(), "--json", "--", "x"],
    });
    assert!(ops.call(&app(), "cluster.dispatch", &delegate).is_err());
    assert!(rig.fake.calls.lock().unwrap().is_empty());
    assert!(!rig.fx.worker_cluster.join("work").exists());
    assert_eq!(
        returns.spec("cluster.return"),
        Some(notchling_local_api::OpSpec::APP)
    );
    assert!(returns
        .call(&app(), "cluster.return", &json!({"job": job_id("never")}))
        .is_err());
}

#[test]
fn a_dispatch_the_core_would_refuse_sends_nothing_when_the_node_is_unknown() {
    let rig = rig("send-unknown");
    let net = MemNet::new();
    let (a, _b) = pair(&net, opts("home", "macos", true), worker_opts(&rig));
    let (ops, _returns, _dir) = dispatch_ops(&rig, &a);
    let ghost = crate::cluster_workspace_testkit::node_id('e');
    let mut args = send_args(&_b, &rig, "u1");
    args["node"] = json!(ghost);
    let err = ops.call(&app(), "cluster.dispatch", &args).unwrap_err();
    assert!(!err.message.is_empty());
    assert!(!rig.fx.worker_cluster.join("work").exists());
}
