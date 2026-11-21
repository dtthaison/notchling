use super::{build, DispatchCalls, DispatchOps, NodeReporter};
use crate::acting_ops_tests::app;
use crate::acting_ops_tests::FakeRoom;
use crate::cluster_dispatch_delegate::DelegateRunner;
use crate::cluster_node::LocalCalls;
use crate::cluster_switches::LateHandler;
use crate::cluster_testkit::{eventually, member, online, pair, Dir, FakeCalls, Opts, TestNode};
use crate::cluster_wiring::DaemonCalls;
use crate::op_group::OpGroup;
use crate::shell_jobs::{Reporter, ShellJobs};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::LocalCmd;
use notchling_engine::versions::Tool as VTool;
use notchling_local_api::{ApiError, Caller, ClientKind, ErrorCode, Handler, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

fn agent() -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid: None,
    }
}

fn home_opts(tools: &[(VTool, &'static str)]) -> Opts {
    let mut opts = Opts::named("home");
    opts.client = true;
    opts.os = "macos";
    opts.tools = tools.to_vec();
    opts
}

fn worker_opts(os: &'static str, tools: &[(VTool, &'static str)]) -> Opts {
    let mut opts = Opts::named("worker");
    opts.os = os;
    opts.tools = tools.to_vec();
    opts
}

fn ops_of(node: &TestNode) -> (Arc<DispatchOps>, Dir) {
    let dir = Dir::new("dispatch-shells");
    let shells = ShellJobs::new(dir.path().to_path_buf(), Arc::new(NodeReporter::default()));
    (DispatchOps::new(Ok(node.node.clone()), shells), dir)
}

fn dispatch(ops: &DispatchOps, who: &Caller, args: Value) -> Result<Value, ApiError> {
    ops.call(who, "cluster.dispatch", &args)
}

fn claude_args(node: &TestNode, key: &str) -> Value {
    json!({"node": node.id(), "tool": "claude", "cwd": "/srv/repo", "key": key})
}

const BOTH: [(VTool, &str); 2] = [(VTool::Claude, "2.1.324"), (VTool::Agy, "1.5.1")];

#[test]
fn dispatch_on_home_starts_session_on_worker() {
    let net = MemNet::new();
    let (a, b) = pair(&net, home_opts(&BOTH), worker_opts("macos", &BOTH));
    let (ops, _dir) = ops_of(&a);
    let mut args = claude_args(&b, "k1");
    args["model"] = json!("opus");
    let reply = dispatch(&ops, &app(), args).unwrap();
    assert!(reply["job"].as_str().unwrap().starts_with("j-"), "{reply}");
    assert_eq!(reply["session"], "s-session.new");
    assert_eq!(reply["state"], "running");
    let calls = b.calls.calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].0, "session.new");
    assert_eq!(calls[0].1["folder"], "/srv/repo");
    assert_eq!(calls[0].1["flags"], json!(["--model", "opus"]));
    assert_eq!(calls[0].1["job"], reply["job"]);
    assert_eq!(
        calls[0].2,
        Origin {
            node: a.id(),
            tier: OriginTier::App
        }
    );
}

#[test]
fn dispatch_from_client_goes_through_home() {
    let net = MemNet::new();
    let (a, b) = pair(&net, home_opts(&BOTH), worker_opts("macos", &BOTH));
    let mut opts = home_opts(&BOTH);
    opts.name = "laptop";
    let c = crate::cluster_testkit::node(&net, opts);
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
    let (ops, _dir) = ops_of(&c);
    let reply = dispatch(&ops, &app(), claude_args(&b, "k2")).unwrap();
    assert_eq!(reply["session"], "s-session.new");
    let calls = b.calls.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].2.node, a.id(), "the home sent it, not the client");
    drop(calls);
    let qualified = json!(format!("{}/s-session.new", b.id()));
    eventually("the home lists the running job", || {
        let job = a.node.status()["jobs"][0].clone();
        job["state"] == "running" && job["qualified"] == qualified
    });
}

#[test]
fn dispatch_to_offline_node_is_an_error() {
    let net = MemNet::new();
    let (a, b) = pair(&net, home_opts(&BOTH), worker_opts("macos", &BOTH));
    let id = b.id();
    b.node.close();
    eventually("the worker is offline", || {
        !online(&a.node.status(), &id)
            && member(&a.node.status(), &id).is_some_and(|m| m["state"] == "active")
    });
    let (ops, _dir) = ops_of(&a);
    let err = dispatch(&ops, &app(), claude_args(&b, "k3")).unwrap_err();
    assert_eq!(err.message, "worker is offline");
    assert!(a.node.status()["jobs"].as_array().unwrap().is_empty());
}

#[test]
fn dispatch_refused_by_versions_says_why() {
    let net = MemNet::new();
    let (a, b) = pair(
        &net,
        home_opts(&BOTH),
        worker_opts("linux", &[(VTool::Claude, "2.1.324")]),
    );
    let (ops, _dir) = ops_of(&a);
    let args = json!({"node": b.id(), "tool": "agy", "cwd": "/srv", "text": "hi", "key": "k4"});
    let err = dispatch(&ops, &app(), args).unwrap_err();
    assert_eq!(err.message, "Antigravity isn't installed on worker");
    assert!(b.calls.calls.lock().unwrap().is_empty());
    let linux = dispatch(&ops, &app(), claude_args(&b, "k4b")).unwrap_err();
    assert_eq!(linux.message, "Claude Code on Linux isn't proven yet");
}

#[test]
fn tool_mismatch_dispatches_with_a_warning() {
    let net = MemNet::new();
    let (a, b) = pair(
        &net,
        home_opts(&BOTH),
        worker_opts("linux", &[(VTool::Agy, "1.5.0")]),
    );
    let (ops, _dir) = ops_of(&a);
    let args = json!({"node": b.id(), "tool": "agy", "cwd": "/srv", "text": "hello", "key": "k5"});
    let reply = dispatch(&ops, &app(), args).unwrap();
    let warning = reply["warning"].as_str().unwrap();
    assert!(
        warning.contains("1.5.0") && warning.contains("1.5.1"),
        "{warning}"
    );
    let calls = b.calls.calls.lock().unwrap();
    assert_eq!(calls[0].0, "agy.send");
    assert_eq!(calls[0].1["draft_folder"], "/srv");
    assert_eq!(calls[0].1["text"], "hello");
}

#[test]
fn retry_with_same_key_returns_same_job() {
    let net = MemNet::new();
    let (a, b) = pair(&net, home_opts(&BOTH), worker_opts("macos", &BOTH));
    let (ops, _dir) = ops_of(&a);
    let first = dispatch(&ops, &app(), claude_args(&b, "same")).unwrap();
    let again = dispatch(&ops, &app(), claude_args(&b, "same")).unwrap();
    assert_eq!(first["job"], again["job"]);
    assert_eq!(b.calls.calls.lock().unwrap().len(), 1);
    let other = dispatch(&ops, &app(), claude_args(&b, "other")).unwrap();
    assert_ne!(first["job"], other["job"]);
}

#[test]
fn agent_dispatch_needs_accepts_delegation() {
    let net = MemNet::new();
    let (a, b) = pair(&net, home_opts(&BOTH), worker_opts("macos", &BOTH));
    let (ops, _dir) = ops_of(&a);
    let delegate = json!({
        "node": b.id(),
        "delegate": ["agy", "--folder", "/srv", "--json", "--", "review"],
        "key": "d1",
    });
    let err = dispatch(&ops, &agent(), delegate.clone()).unwrap_err();
    assert_eq!(err.message, "worker doesn't take delegated jobs");
    assert!(b.calls.calls.lock().unwrap().is_empty());
    a.node
        .local(LocalCmd::SetMember {
            node: b.id(),
            can_be_master: None,
            client: None,
            reference: None,
            accepts_delegation: Some(true),
            headless: None,
            desired: None,
        })
        .unwrap();
    let reply = dispatch(&ops, &agent(), delegate).unwrap();
    assert!(reply["job"].as_str().is_some(), "{reply}");
    let calls = b.calls.calls.lock().unwrap();
    assert_eq!(calls[0].0, "delegate.run");
    assert_eq!(
        calls[0].1["args"],
        json!(["agy", "--folder", "/srv", "--json", "--", "review"])
    );
    assert_eq!(calls[0].2.tier, OriginTier::Agent);
}

#[test]
fn an_agent_cannot_start_a_session_or_a_shell_job() {
    let net = MemNet::new();
    let (a, b) = pair(&net, home_opts(&BOTH), worker_opts("macos", &BOTH));
    let (ops, _dir) = ops_of(&a);
    let shell =
        json!({"node": b.id(), "tool": "shell", "cwd": "/srv", "command": "id", "key": "x"});
    let err = dispatch(&ops, &agent(), shell).unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    let err = dispatch(&ops, &agent(), claude_args(&b, "y")).unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert!(b.calls.calls.lock().unwrap().is_empty());
}

const WITH_CODEX: [(VTool, &str); 3] = [
    (VTool::Claude, "2.1.324"),
    (VTool::Codex, "0.166.0"),
    (VTool::Agy, "1.5.1"),
];

fn codex_args(node: &TestNode, key: &str) -> Value {
    json!({"node": node.id(), "tool": "codex", "cwd": "/srv/repo", "text": "fix it", "key": key})
}

#[test]
fn a_codex_job_starts_codex_send_on_the_worker_as_the_app() {
    let net = MemNet::new();
    let (a, b) = pair(
        &net,
        home_opts(&WITH_CODEX),
        worker_opts("linux", &WITH_CODEX),
    );
    let (ops, _dir) = ops_of(&a);
    let mut args = codex_args(&b, "c1");
    args["model"] = json!("gpt-6.1-sol");
    args["effort"] = json!("high");
    let reply = dispatch(&ops, &app(), args).unwrap();
    assert_eq!(reply["state"], "running");
    assert_eq!(reply["session"], "s-codex.send");
    let calls = b.calls.calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].0, "codex.send");
    assert_eq!(calls[0].1["draft_folder"], "/srv/repo");
    assert_eq!(calls[0].1["text"], "fix it");
    assert_eq!(calls[0].1["model"], "gpt-6.1-sol");
    assert_eq!(calls[0].1["effort"], "high");
    assert_eq!(calls[0].1["job"], reply["job"]);
    assert_eq!(
        calls[0].2,
        Origin {
            node: a.id(),
            tier: OriginTier::App
        }
    );
}

#[test]
fn an_agent_cannot_start_a_codex_job() {
    let net = MemNet::new();
    let (a, b) = pair(
        &net,
        home_opts(&WITH_CODEX),
        worker_opts("linux", &WITH_CODEX),
    );
    let (ops, _dir) = ops_of(&a);
    let err = dispatch(&ops, &agent(), codex_args(&b, "c2")).unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert!(b.calls.calls.lock().unwrap().is_empty());
}

#[test]
fn a_node_without_codex_refuses_a_codex_job_by_name() {
    let net = MemNet::new();
    let (a, b) = pair(&net, home_opts(&WITH_CODEX), worker_opts("linux", &BOTH));
    let (ops, _dir) = ops_of(&a);
    let err = dispatch(&ops, &app(), codex_args(&b, "c3")).unwrap_err();
    assert_eq!(err.message, "Codex isn't installed on worker");
    assert!(b.calls.calls.lock().unwrap().is_empty());
    assert!(a.node.status()["jobs"].as_array().unwrap().is_empty());
}

#[test]
fn a_codex_job_takes_no_secrets_and_the_others_need_the_allowlist() {
    let net = MemNet::new();
    let (a, b) = pair(&net, home_opts(&BOTH), worker_opts("macos", &BOTH));
    let (ops, _dir) = ops_of(&a);
    let mut codex = codex_args(&b, "c");
    codex["secrets"] = json!(["gh"]);
    let err = dispatch(&ops, &app(), codex).unwrap_err();
    assert_eq!(err.message, "Codex jobs on another machine take no secrets");
    assert!(b.calls.calls.lock().unwrap().is_empty());
    let mut secret = claude_args(&b, "s");
    secret["secrets"] = json!(["gh"]);
    let err = dispatch(&ops, &app(), secret).unwrap_err();
    assert!(err.message.contains("gh"), "{}", err.message);
    assert!(err.message.contains("allowed"), "{}", err.message);
    let mut empty = claude_args(&b, "e");
    empty["secrets"] = json!([]);
    assert!(dispatch(&ops, &app(), empty).is_ok());
    assert!(a.node.status()["jobs"].as_array().unwrap().len() == 1);
}

#[test]
fn bad_arguments_are_refused_before_anything_is_sent() {
    let node = "0123456789abcdef";
    let good = json!({"node": node, "tool": "claude", "cwd": "/srv", "key": "k"});
    let with = |field: &str, value: Value| {
        let mut args = good.clone();
        args[field] = value;
        args
    };
    let cases = [
        with("key", json!("")),
        with("node", json!("nope")),
        with("cwd", json!("relative")),
        with("tool", json!("emacs")),
        with("text", json!("hello")),
        with("command", json!("id")),
        json!({"node": node, "tool": "shell", "cwd": "/srv", "key": "k"}),
        json!({"node": node, "tool": "shell", "cwd": "/srv", "key": "k", "command": "id", "model": "x"}),
        json!({"node": node, "tool": "codex", "cwd": "/srv", "key": "k"}),
        json!({"node": node, "tool": "codex", "cwd": "/srv", "key": "k", "text": " "}),
        json!({"node": node, "tool": "codex", "cwd": "/srv", "key": "k", "text": "x", "command": "id"}),
        json!({"node": node, "tool": "codex", "cwd": "/srv", "key": "k", "text": "x", "secrets": ["gh"]}),
        json!({"node": node, "tool": "agy", "cwd": "/srv", "key": "k"}),
        json!({"node": node, "tool": "agy", "cwd": "/srv", "key": "k", "text": "x", "model": "m"}),
        json!({"node": node, "delegate": "agy", "key": "k"}),
        json!({"node": node, "delegate": ["codex", "--", "x"], "key": "k"}),
        json!({"node": node, "delegate": ["agy", "--write", "--", "x"], "key": "k"}),
        json!({"node": node, "delegate": ["agy", "--timeout", "99999", "--", "x"], "key": "k"}),
        json!({"node": node, "delegate": ["agy", "--prompt-file", "/etc/passwd"], "key": "k"}),
        json!({"node": node, "delegate": ["agy"], "key": "k"}),
    ];
    for args in cases {
        let got = build(&args, OriginTier::App);
        assert_eq!(got.unwrap_err().code, ErrorCode::BadRequest, "{args}");
    }
    let (key, spec) = build(&good, OriginTier::App).unwrap();
    assert_eq!(key, "k");
    assert_eq!(spec.cwd, "/srv");
}

#[test]
fn secret_names_are_checked_lowercased_and_deduplicated() {
    let node = "0123456789abcdef";
    let base = json!({"node": node, "tool": "claude", "cwd": "/srv", "key": "k"});
    let with = |secrets: Value| {
        let mut args = base.clone();
        args["secrets"] = secrets;
        build(&args, OriginTier::App)
    };
    let (_, spec) = with(json!(["GH", "gh", "Db.Pass"])).unwrap();
    assert_eq!(spec.secrets, ["gh", "db.pass"]);
    assert!(with(json!([])).unwrap().1.secrets.is_empty());
    assert!(build(&base, OriginTier::App).unwrap().1.secrets.is_empty());
    let many: Vec<String> = (0..17).map(|n| format!("s{n}")).collect();
    for bad in [
        json!("gh"),
        json!([1]),
        json!(["has space"]),
        json!([""]),
        json!(many),
    ] {
        let got = with(bad.clone());
        assert_eq!(got.unwrap_err().code, ErrorCode::BadRequest, "{bad}");
    }
    let delegate = json!({
        "node": node,
        "delegate": ["agy", "--", "x"],
        "key": "k",
        "secrets": ["gh"],
    });
    let err = build(&delegate, OriginTier::App).unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
}

#[test]
fn a_delegate_spec_carries_its_arguments_and_folder() {
    let args = json!({
        "node": "0123456789abcdef",
        "delegate": ["agy", "--folder", "/srv/x", "--json", "--", "look"],
        "key": "k",
    });
    let (_, spec) = build(&args, OriginTier::Agent).unwrap();
    assert!(spec.is_delegate());
    assert_eq!(spec.cwd, "/srv/x");
    assert_eq!(spec.tool, notchling_engine::cluster::Tool::Agy);
}

struct Runs(Mutex<Vec<Vec<String>>>);

impl DelegateRunner for Runs {
    fn run(&self, args: &[String]) -> (i32, String, String) {
        self.0.lock().unwrap().push(args.to_vec());
        (0, "done\n".to_string(), "careful\n".to_string())
    }
}

fn worker_calls(takes: bool) -> (DispatchCalls, Arc<Runs>, Arc<FakeCalls>, Dir) {
    let dir = Dir::new("calls");
    let shells = ShellJobs::new(dir.path().to_path_buf(), Arc::new(NodeReporter::default()));
    let runs = Arc::new(Runs(Mutex::new(Vec::new())));
    let inner = Arc::new(FakeCalls::default());
    let calls = DispatchCalls::new(inner.clone(), shells, runs.clone(), Arc::new(move || takes));
    (calls, runs, inner, dir)
}

pub(super) fn from_home() -> Origin {
    Origin {
        node: notchling_engine::cluster::NodeId::parse("0123456789abcdef").unwrap(),
        tier: OriginTier::Agent,
    }
}

#[test]
fn delegate_run_reuses_the_engine_delegate_and_returns_its_outcome() {
    let (calls, runs, inner, _dir) = worker_calls(true);
    let args = json!({"args": ["agy", "--folder", "/srv", "--json", "--", "x"], "job": "j-1"});
    let out = calls.call("delegate.run", &args, &from_home()).unwrap();
    assert_eq!(out, json!({"exit": 0, "out": "done\n", "err": "careful\n"}));
    assert_eq!(runs.0.lock().unwrap().len(), 1);
    assert!(inner.calls.lock().unwrap().is_empty());
}

#[test]
fn delegate_run_refuses_when_this_node_does_not_take_delegation_or_args_are_unsafe() {
    let (calls, runs, _inner, _dir) = worker_calls(false);
    let args = json!({"args": ["agy", "--folder", "/srv", "--", "x"]});
    let err = calls.call("delegate.run", &args, &from_home()).unwrap_err();
    assert_eq!(err.message, "this machine doesn't take delegated jobs");
    let (calls, runs2, _inner, _dir) = worker_calls(true);
    for args in [
        json!({"args": ["agy", "--write", "--", "x"]}),
        json!({"args": ["codex", "--", "x"]}),
        json!({"args": ["agy", "--prompt-file", "/etc/hosts"]}),
        json!({"args": "agy"}),
        json!({}),
    ] {
        let got = calls.call("delegate.run", &args, &from_home());
        assert_eq!(got.unwrap_err().code, ErrorCode::BadRequest, "{args}");
    }
    assert!(runs.0.lock().unwrap().is_empty());
    assert!(runs2.0.lock().unwrap().is_empty());
}

#[test]
fn other_ops_go_to_the_inner_calls_untouched() {
    let (calls, _runs, inner, _dir) = worker_calls(true);
    let out = calls
        .call("session.new", &json!({"folder": "/x"}), &from_home())
        .unwrap();
    assert_eq!(out, json!({"session": "s-session.new"}));
    assert_eq!(inner.calls.lock().unwrap().len(), 1);
}

#[cfg(unix)]
#[test]
fn a_shell_job_runs_on_the_worker_and_the_home_hears_it_finish() {
    let net = MemNet::new();
    let work = Dir::new("shell-work");
    let data = Dir::new("shell-data");
    let tell = Arc::new(NodeReporter::default());
    let shells = ShellJobs::new(data.path().to_path_buf(), tell.clone());
    let runs = Arc::new(Runs(Mutex::new(Vec::new())));
    let inner = Arc::new(FakeCalls::default());
    let calls: Arc<dyn LocalCalls> = Arc::new(DispatchCalls::new(
        inner,
        shells.clone(),
        runs,
        Arc::new(|| false),
    ));
    let mut worker = worker_opts("linux", &[]);
    worker.local = Some(calls);
    let (a, b) = pair(&net, home_opts(&[]), worker);
    tell.attach(b.node.clone());
    let (ops, _dir) = ops_of(&a);
    let args = json!({
        "node": b.id(), "tool": "shell", "cwd": work.path().to_string_lossy(),
        "command": "printf ok", "key": "sh1",
    });
    let reply = dispatch(&ops, &app(), args).unwrap();
    let job = reply["job"].as_str().unwrap().to_string();
    assert!(
        reply["session"].as_str().unwrap().starts_with("shell:"),
        "{reply}"
    );
    eventually("the home sees the job done", || {
        a.node.status()["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|j| j["id"] == json!(job) && j["state"] == "done")
    });
    let log = data.path().join("cluster/jobs").join(format!("{job}.log"));
    assert_eq!(std::fs::read_to_string(log).unwrap(), "ok");
    assert_eq!(shells.rows()[0]["label"], "Done");
    shells.close();
}

#[test]
fn shell_stop_with_an_id_of_this_node_stops_the_local_job() {
    let net = MemNet::new();
    let (a, _b) = pair(&net, home_opts(&[]), worker_opts("linux", &[]));
    let dir = Dir::new("stop-own");
    let shells = ShellJobs::new(dir.path().to_path_buf(), Arc::new(NodeReporter::default()));
    let ops = DispatchOps::new(Ok(a.node.clone()), shells.clone());
    let own = format!("{}/shell:nope", a.id());
    let err = ops
        .call(&app(), "shell.stop", &json!({"id": own}))
        .unwrap_err();
    assert_eq!(err.message, "no such shell job");
    assert_eq!(
        ops.spec("shell.stop"),
        Some(notchling_local_api::OpSpec::APP)
    );
    assert_eq!(
        ops.spec("cluster.dispatch"),
        Some(notchling_local_api::OpSpec::AGENT_READ)
    );
    assert_eq!(ops.spec("shell.run"), None);
    assert_eq!(ops.spec("delegate.run"), None);
    shells.close();
}

#[test]
fn the_reporter_says_nothing_until_it_has_a_node() {
    let lonely = NodeReporter::default();
    lonely.report("j-1", notchling_engine::cluster::JobState::Done, None);
}

struct Starts(Mutex<Vec<(String, Tier, ClientKind)>>);

impl Handler for Starts {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        matches!(op, "session.new" | "agy.send").then_some(OpSpec::APP)
    }

    fn call(&self, caller: &Caller, op: &str, _args: &Value) -> Result<Value, ApiError> {
        self.0
            .lock()
            .unwrap()
            .push((op.to_string(), caller.tier, caller.client));
        Ok(match op {
            "session.new" => json!({"jobId": "c1"}),
            _ => json!({"id": "agy:x", "sent": true}),
        })
    }
}

fn real_calls() -> (Arc<dyn LocalCalls>, Arc<Starts>, Arc<dyn Handler>) {
    let starts = Arc::new(Starts(Mutex::new(Vec::new())));
    let handler: Arc<dyn Handler> = starts.clone();
    let late = Arc::new(LateHandler::default());
    late.serve(&handler);
    let calls: Arc<dyn LocalCalls> = DaemonCalls::new(late, Arc::new(FakeRoom::default()));
    (calls, starts, handler)
}

#[test]
fn a_home_dispatch_of_claude_and_agy_reaches_the_op_on_a_peer_and_on_the_home() {
    let net = MemNet::new();
    let (home_calls, home_starts, _keep_home) = real_calls();
    let (worker_calls, worker_starts, _keep_worker) = real_calls();
    let mut home = home_opts(&BOTH);
    home.local = Some(home_calls);
    let mut worker = worker_opts("macos", &BOTH);
    worker.local = Some(worker_calls);
    let (a, b) = pair(&net, home, worker);
    let (ops, _dir) = ops_of(&a);
    let agy = |node: &TestNode, key: &str| json!({"node": node.id(), "tool": "agy", "cwd": "/srv", "text": "hi", "key": key});
    for (target, starts, tag) in [(&b, &worker_starts, "peer"), (&a, &home_starts, "home")] {
        let claude = dispatch(&ops, &app(), claude_args(target, &format!("c-{tag}")));
        let claude = claude.unwrap_or_else(|e| panic!("{tag}: {}", e.message));
        assert_eq!(claude["session"], "c1", "{tag}");
        let sent = dispatch(&ops, &app(), agy(target, &format!("a-{tag}")));
        let sent = sent.unwrap_or_else(|e| panic!("{tag}: {}", e.message));
        assert_eq!(sent["session"], "agy:x", "{tag}");
        let seen = starts.0.lock().unwrap();
        let ops_seen: Vec<&str> = seen.iter().map(|(op, _, _)| op.as_str()).collect();
        assert_eq!(ops_seen, ["session.new", "agy.send"], "{tag}");
        assert!(
            seen.iter()
                .all(|(_, tier, client)| *tier == Tier::App && *client == ClientKind::Node),
            "{tag}"
        );
    }
}

#[test]
fn an_agent_still_cannot_send_a_session_start_through_the_real_path() {
    let net = MemNet::new();
    let (worker_calls, worker_starts, _keep) = real_calls();
    let mut worker = worker_opts("macos", &BOTH);
    worker.local = Some(worker_calls);
    let (a, b) = pair(&net, home_opts(&BOTH), worker);
    let (ops, _dir) = ops_of(&a);
    let err = dispatch(&ops, &agent(), claude_args(&b, "ag")).unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert!(worker_starts.0.lock().unwrap().is_empty());
}
