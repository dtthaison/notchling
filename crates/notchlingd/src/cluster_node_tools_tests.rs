use crate::acting_ops_tests::FakeRoom;
use crate::cluster_logins_notes::Notes;
use crate::cluster_logins_probe::{has_claude_profile, Files, LoginProbe};
use crate::cluster_versions::{with_own, with_probed, with_tools};
use crate::limits_ops_tests::TempDir;
use crate::versions_ops::{Clock, VersionsOps};
use notchling_engine::cluster::logins::{StatusRunner, AGY_LOGIN_FILE};
use notchling_engine::control::launcher::{FakeLauncher, LaunchOutput};
use notchling_engine::inbox::{Item, State};
use notchling_engine::versions::mac::Roots;
use notchling_engine::versions::record::MemoryRecorder;
use notchling_engine::versions::{Tool, Version};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

struct Silent;

impl Notes for Silent {
    fn raise(&self, _item: Item) {}

    fn withdraw(&self, _id: &str) {}

    fn state(&self, _id: &str) -> Option<State> {
        None
    }

    fn open_ids(&self) -> Vec<String> {
        Vec::new()
    }
}

struct NoCommands;

impl StatusRunner for NoCommands {
    fn run(&self, argv: &[String], _timeout: Duration) -> Option<(i32, String)> {
        if argv == notchling_engine::cluster::logins::keychain_argv() {
            return None;
        }
        panic!("a status command ran: {argv:?}");
    }
}

fn node_with_agy(answer: &str) -> (TempDir, Arc<VersionsOps>) {
    let tmp = TempDir::new("node-tools-agy");
    tmp.write("home/.local/bin/agy", "binary");
    tmp.write("own-binary", "the daemon");
    let launcher = Arc::new(FakeLauncher::new());
    launcher.push_output(Ok(LaunchOutput::ok(answer)));
    let clock: Clock = Arc::new(|| 1_000);
    let ops = VersionsOps::new(
        Roots::inside(&tmp.path("home")),
        tmp.path("data"),
        Arc::new(FakeRoom::default()),
        launcher,
        Arc::new(MemoryRecorder::new()),
        clock,
        Version::parse("0.1.0").unwrap(),
    );
    ops.set_exe(Some(tmp.path("own-binary")));
    ops.rescan();
    (tmp, ops)
}

fn probe_over(ops: &Arc<VersionsOps>, files: Files) -> Arc<LoginProbe> {
    LoginProbe::new(
        (ops.clone(), Arc::new(NoCommands), files),
        (Arc::new(Silent), Arc::new(|| 1_000)),
        ("node".to_string(), "macos".to_string()),
    )
}

fn token_file(ops: &VersionsOps) -> PathBuf {
    ops.home().join(AGY_LOGIN_FILE)
}

#[test]
fn a_bare_agy_version_makes_agy_a_tool_of_the_node() {
    let (_tmp, ops) = node_with_agy("1.5.9\n");
    assert_eq!(
        with_tools(ops.tool_versions()).tools.get(&Tool::Agy),
        Version::parse("1.5.9").as_ref()
    );
}

#[test]
fn a_prefixed_agy_version_still_does() {
    let (_tmp, ops) = node_with_agy("agy 1.4.2\n");
    assert_eq!(
        with_tools(ops.tool_versions()).tools.get(&Tool::Agy),
        Version::parse("1.4.2").as_ref()
    );
}

#[test]
fn an_agy_answer_that_is_no_version_leaves_agy_out_of_the_tools() {
    let (_tmp, ops) = node_with_agy("sk-ant-oat01-XXXX\n");
    assert!(!with_tools(ops.tool_versions())
        .tools
        .contains_key(&Tool::Agy));
    assert!(ops.tool_paths().contains_key(&Tool::Agy));
}

#[test]
fn agys_login_is_probed_from_its_token_file_once_agy_is_there() {
    let (_tmp, ops) = node_with_agy("1.5.9\n");
    let token = token_file(&ops);
    let probe = probe_over(&ops, Arc::new(move |path| path == token));
    assert!(probe.refresh());
    assert_eq!(probe.logins().get(&Tool::Agy), Some(&true));
}

#[test]
fn a_mac_without_the_token_file_says_nothing_about_agys_login() {
    let (_tmp, ops) = node_with_agy("1.5.9\n");
    let probe = probe_over(&ops, Arc::new(|_| false));
    assert!(probe.refresh());
    assert_eq!(probe.logins().get(&Tool::Agy), None);
}

#[test]
fn a_probe_with_no_own_check_reports_no_own_login() {
    let (_tmp, ops) = node_with_agy("1.5.9\n");
    let probe = probe_over(&ops, Arc::new(|_| false));
    assert!(probe.refresh());
    assert!(probe.own_logins().is_empty());
}

#[test]
fn the_own_claude_login_is_checked_in_the_nodes_home_and_kept_off_logins() {
    let (_tmp, ops) = node_with_agy("1.5.9\n");
    let probe = probe_over(&ops, Arc::new(|_| false));
    let home = ops.home();
    probe.set_own(Arc::new(move |asked| asked == home));
    assert!(probe.refresh());
    assert_eq!(probe.own_logins().get(&Tool::Claude), Some(&true));
    assert!(!probe.logins().contains_key(&Tool::Claude));
    let reported = with_own(
        with_probed(with_tools(ops.tool_versions()), probe.logins()),
        probe.own_logins(),
    );
    assert_eq!(reported.own_logins.get(&Tool::Claude), Some(&true));
    assert!(!reported.logins.contains_key(&Tool::Claude));
}

#[test]
fn no_own_login_leaves_the_claude_entry_out_instead_of_saying_false() {
    let (_tmp, ops) = node_with_agy("1.5.9\n");
    let probe = probe_over(&ops, Arc::new(|_| false));
    probe.set_own(Arc::new(|_| false));
    assert!(probe.refresh());
    assert!(probe.own_logins().is_empty());
}

#[test]
fn the_own_login_follows_the_profile_between_probes() {
    let (_tmp, ops) = node_with_agy("1.5.9\n");
    let probe = probe_over(&ops, Arc::new(|_| false));
    let signed_in = Arc::new(AtomicBool::new(true));
    let flag = signed_in.clone();
    probe.set_own(Arc::new(move |_| flag.load(Ordering::SeqCst)));
    assert!(probe.refresh());
    assert_eq!(probe.own_logins().len(), 1);
    signed_in.store(false, Ordering::SeqCst);
    assert!(probe.refresh());
    assert!(probe.own_logins().is_empty());
}

#[test]
fn the_real_check_reads_the_profile_in_the_home_it_is_given() {
    let tmp = TempDir::new("node-own-profile");
    tmp.write(
        "signed-in/.claude.json",
        r#"{"numStartups": 2, "oauthAccount": {"accountUuid": "u-1", "emailAddress": "a@example.com"}}"#,
    );
    tmp.write("signed-out/.claude.json", r#"{"numStartups": 2}"#);
    tmp.write("garbled/.claude.json", r#"{"oauthAccount": {"accountUuid""#);
    tmp.write(
        "elsewhere/.claude/.claude.json",
        r#"{"oauthAccount": {"a": 1}}"#,
    );
    assert!(has_claude_profile(&tmp.path("signed-in")));
    assert!(!has_claude_profile(&tmp.path("signed-out")));
    assert!(!has_claude_profile(&tmp.path("garbled")));
    assert!(!has_claude_profile(&tmp.path("elsewhere")));
    assert!(!has_claude_profile(&tmp.path("missing")));
}
