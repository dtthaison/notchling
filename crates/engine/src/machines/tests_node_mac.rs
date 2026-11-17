use super::fakes_b::*;
use super::registry::Machine;
use super::runner::Recorded;
use super::service_files::node_launchdaemon;
use super::step::{CheckResult, CheckState, FixContext, RemoteOutput, StepFix, StepId};
use super::steps_node::BINARY;
use super::steps_node_mac::{
    binary_path, install_script, load_script, source_for, Host, MAC_BINARY, NODE_PLIST,
    OLD_AGENT_PLIST,
};
use super::tests_node_steps::{fix_of, hex, node_linux, node_mac, step, texts};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const PASSWORD: &str = "pw-admin-test";
const REAL: &str = "/Users/tester/.local/share/notchling/Notchling.app/Contents/MacOS/notchling";
const BUILD: &[u8] = b"this mac's own notchling";

fn host_arch() -> &'static str {
    std::env::consts::ARCH
}

fn mac_arch(arch: &str) -> Machine {
    let mut m = node_mac();
    m.abilities.as_mut().unwrap().arch = Some(arch.into());
    m
}

fn mac_with_app() -> Machine {
    let mut m = node_mac();
    m.abilities.as_mut().unwrap().notchling_app = Some(true);
    m
}

fn parse(m: &Machine, id: &StepId, text: &str, code: i32) -> CheckResult {
    let rig = Rig::new(m.clone());
    let (_, parse) = remote(&step(m, id));
    parse(
        &RemoteOutput {
            text: text.into(),
            code,
        },
        &rig.step(),
    )
}

fn host(arch: &'static str, build: Option<&'static str>) -> Host<'static> {
    Host {
        arch,
        build,
        release: None,
    }
}

fn manifest(dir: &std::path::Path, build: &str) {
    let text = format!(
        "{{\"build\":\"{build}\",\"files\":{{\"notchling-linux-x86_64\":{{\"sha256\":\"{}\",\"size\":3}}}}}}",
        hex(b"abc")
    );
    std::fs::write(dir.join("manifest.json"), text).unwrap();
}

fn admin_calls(calls: &[Recorded]) -> Vec<&Recorded> {
    calls
        .iter()
        .filter(|c| {
            c.argv
                .last()
                .is_some_and(|s| s.contains("/usr/bin/sudo -S"))
        })
        .collect()
}

#[test]
fn mac_binary_path_is_bundle_shaped() {
    assert!(MAC_BINARY.ends_with(".app/Contents/MacOS/notchling"));
    assert_eq!(binary_path(&node_mac()), MAC_BINARY);
    assert_eq!(binary_path(&node_linux()), BINARY);
    let probe = remote(&step(&node_mac(), &StepId::NODE_BINARY)).0;
    assert!(probe.contains(".local/share/notchling/Notchling.app/Contents/MacOS/notchling"));
    let shown = plan(&step(&node_mac(), &StepId::NODE_BINARY)).display;
    assert!(shown.contains("ln -sfn"), "{shown}");
    assert!(shown.contains(".local/bin/notchling"), "{shown}");
}

#[test]
fn mac_same_arch_copies_this_macs_daemon() {
    let rig = Rig::new(mac_arch(host_arch()));
    let tmp = TempDir::new("mac-daemon");
    let daemon = tmp.path().join("notchling");
    std::fs::write(&daemon, BUILD).unwrap();
    let dist = TempDir::new("mac-daemon-dist");
    std::fs::write(
        dist.path().join(format!("notchling-macos-{}", host_arch())),
        b"a stale dist file",
    )
    .unwrap();
    rig.runner.on(
        |a| a.last().is_some_and(|s| s.contains("shasum -a 256")),
        ok(&format!("{}  new\n", hex(BUILD))),
    );
    let mut ctx = fix_of(&rig, Some(dist.path()), None);
    ctx.step.mac_daemon = Some(daemon.clone());
    let out = (plan(&step(&rig.machine, &StepId::NODE_BINARY)).run)(&ctx);
    assert!(out.ok, "{}", out.message);
    let calls = rig.calls();
    let scp = calls
        .iter()
        .find(|c| c.argv.first().is_some_and(|a| a == "/usr/bin/scp"))
        .expect("an scp call");
    assert!(scp.argv.contains(&daemon.display().to_string()));
    assert_eq!(
        scp.argv.last().unwrap(),
        "tester@m2.test:.local/share/notchling/Notchling.app/Contents/MacOS/notchling.new"
    );
    let seen = texts(&calls);
    let mkdir = seen.iter().find(|t| t.contains("mkdir -p")).unwrap();
    assert!(mkdir.contains("Notchling.app/Contents/MacOS"), "{mkdir}");
    let moved = seen.iter().find(|t| t.contains("mv -f")).unwrap();
    assert!(moved.contains("ln -sfn"), "{moved}");
    assert!(moved.contains(".local/bin/notchling"), "{moved}");
}

#[test]
fn mac_other_arch_needs_dist_file() {
    let m = mac_arch("x86_64");
    let dist = TempDir::new("mac-other");
    let daemon = PathBuf::from("/nonexistent/notchling");
    let none = source_for(&m, Some(dist.path()), Some(&daemon), host("aarch64", None));
    assert_eq!(
        none.unwrap_err(),
        "Build Notchling for Intel Macs first: tools/build-node.sh --mac-intel"
    );
    let file = dist.path().join("notchling-macos-x86_64");
    std::fs::write(&file, b"intel").unwrap();
    let found = source_for(&m, Some(dist.path()), Some(&daemon), host("aarch64", None));
    assert_eq!(found.unwrap().path, file);
    let same = source_for(&m, Some(dist.path()), None, host("x86_64", None));
    assert_eq!(same.unwrap().path, file);
}

#[test]
fn mac_same_arch_prefers_the_running_apps_daemon() {
    let m = mac_arch("aarch64");
    let tmp = TempDir::new("mac-prefer");
    let daemon = tmp.path().join("notchling");
    std::fs::write(&daemon, BUILD).unwrap();
    let got = source_for(&m, None, Some(&daemon), host("aarch64", Some("abc1234"))).unwrap();
    assert_eq!(got.path, daemon);
    assert!(got.label.contains("this Mac's own"), "{}", got.label);
}

#[test]
fn linux_needs_manifest_build_equal_to_this_mac() {
    let m = node_linux();
    let dist = TempDir::new("linux-manifest");
    let file = dist.path().join("notchling-linux-x86_64");
    std::fs::write(&file, b"abc").unwrap();
    let want = "Build the Linux nodes first: tools/build-node.sh";

    let none = source_for(
        &m,
        Some(dist.path()),
        None,
        host("aarch64", Some("abc1234")),
    );
    assert_eq!(none.unwrap_err(), want);
    manifest(dist.path(), "def5678");
    let other = source_for(
        &m,
        Some(dist.path()),
        None,
        host("aarch64", Some("abc1234")),
    );
    assert_eq!(other.unwrap_err(), want);
    manifest(dist.path(), "abc1234");
    let same = source_for(
        &m,
        Some(dist.path()),
        None,
        host("aarch64", Some("abc1234")),
    )
    .unwrap();
    assert_eq!(same.path, file);
    assert!(!same.what.contains("no build id"));

    std::fs::remove_file(dist.path().join("manifest.json")).unwrap();
    let dev = source_for(&m, Some(dist.path()), None, host("aarch64", None)).unwrap();
    assert_eq!(dev.path, file);
    assert!(dev.what.contains("no build id"), "{}", dev.what);
}

fn manifest_with(dir: &std::path::Path, build: &str, files: &str) {
    let text = format!("{{\"build\":\"{build}\",\"files\":{files}}}");
    std::fs::write(dir.join("manifest.json"), text).unwrap();
}

#[test]
fn linux_refuses_a_file_the_manifest_does_not_list() {
    let m = node_linux();
    let dist = TempDir::new("linux-unlisted");
    std::fs::write(dist.path().join("notchling-linux-x86_64"), b"abc").unwrap();
    manifest_with(dist.path(), "abc1234", "{}");
    let got = source_for(
        &m,
        Some(dist.path()),
        None,
        host("aarch64", Some("abc1234")),
    );
    let why = got.unwrap_err();
    assert!(why.contains("notchling-linux-x86_64"), "{why}");
    assert!(why.contains("tools/build-node.sh"), "{why}");
}

#[test]
fn linux_refuses_a_file_whose_hash_is_not_the_manifests() {
    let m = node_linux();
    let dist = TempDir::new("linux-other-hash");
    std::fs::write(
        dist.path().join("notchling-linux-x86_64"),
        b"changed after the build",
    )
    .unwrap();
    manifest(dist.path(), "abc1234");
    let got = source_for(
        &m,
        Some(dist.path()),
        None,
        host("aarch64", Some("abc1234")),
    );
    let why = got.unwrap_err();
    assert!(why.contains("notchling-linux-x86_64"), "{why}");
    assert!(why.contains("tools/build-node.sh"), "{why}");
}

#[test]
fn linux_uses_a_file_whose_name_and_hash_the_manifest_lists() {
    let m = node_linux();
    let dist = TempDir::new("linux-listed");
    let file = dist.path().join("notchling-linux-x86_64");
    std::fs::write(&file, b"abc").unwrap();
    manifest(dist.path(), "abc1234");
    let got = source_for(
        &m,
        Some(dist.path()),
        None,
        host("aarch64", Some("abc1234")),
    )
    .unwrap();
    assert_eq!(got.path, file);
    assert_eq!(got.digest.as_deref(), Some(hex(b"abc").as_str()));
}

#[test]
fn app_present_no_longer_skips_binary_and_service() {
    let m = mac_with_app();
    let binary = parse(
        &m,
        &StepId::NODE_BINARY,
        "sh: notchling: not found\nsha256=\napp=yes\n",
        0,
    );
    assert!(binary.state.is_fail(), "{:?}", binary.state);
    assert_eq!(binary.detected.notchling_app, Some(true));
    let service = parse(&m, &StepId::NODE_SERVICE, "plist=no\n", 0);
    assert!(service.state.is_fail(), "{:?}", service.state);
    assert!(!service.state.text().contains("Not needed"));
}

fn golden() -> String {
    [
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">"#,
        r#"<plist version="1.0">"#,
        "<dict>",
        "\t<key>Label</key>",
        "\t<string>dev.notchling.node</string>",
        "\t<key>ProgramArguments</key>",
        "\t<array>",
        &format!("\t\t<string>{REAL}</string>"),
        "\t\t<string>daemon</string>",
        "\t</array>",
        "\t<key>UserName</key>",
        "\t<string>tester</string>",
        "\t<key>EnvironmentVariables</key>",
        "\t<dict>",
        "\t\t<key>HOME</key>",
        "\t\t<string>/Users/tester</string>",
        "\t\t<key>PATH</key>",
        "\t\t<string>/Users/tester/.local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>",
        "\t</dict>",
        "\t<key>RunAtLoad</key>",
        "\t<true/>",
        "\t<key>KeepAlive</key>",
        "\t<dict>",
        "\t\t<key>SuccessfulExit</key>",
        "\t\t<false/>",
        "\t</dict>",
        "\t<key>ThrottleInterval</key>",
        "\t<integer>30</integer>",
        "\t<key>AbandonProcessGroup</key>",
        "\t<true/>",
        "\t<key>ProcessType</key>",
        "\t<string>Background</string>",
        "\t<key>StandardOutPath</key>",
        "\t<string>/Users/tester/Library/Application Support/Notchling/notchlingd.log</string>",
        "\t<key>StandardErrorPath</key>",
        "\t<string>/Users/tester/Library/Application Support/Notchling/notchlingd.log</string>",
        "</dict>",
        "</plist>",
        "",
    ]
    .join("\n")
}

#[test]
fn plist_golden() {
    let text = node_launchdaemon("tester", "/Users/tester", REAL).unwrap();
    assert_eq!(text, golden());
    assert!(!text.contains("<!--"));
}

#[test]
fn plist_refuses_unsafe_home_and_user() {
    for user in ["root", "", "a b", "x;y", "Tester", "a<b", "-x"] {
        assert!(
            node_launchdaemon(user, "/Users/tester", REAL).is_err(),
            "user {user:?}"
        );
    }
    for home in [
        "relative",
        "/Users/a b",
        "/Users/../x",
        "/Users/a<b",
        "/Users/a&b",
        "/Users/a\"b",
        "",
    ] {
        assert!(
            node_launchdaemon("tester", home, REAL).is_err(),
            "home {home:?}"
        );
    }
    assert!(node_launchdaemon("tester", "/Users/tester", "/Users/t/a b/notchling").is_err());
    assert!(node_launchdaemon("tester", "/Users/tester", "notchling").is_err());
    assert!(node_launchdaemon("tester", "/Users/tester", "/Users/tester/<x>/notchling").is_err());
}

#[test]
fn plist_abandons_process_group() {
    let text = node_launchdaemon("tester", "/Users/tester", REAL).unwrap();
    assert!(text.contains("<key>AbandonProcessGroup</key>\n\t<true/>"));
    assert!(text.contains("<key>ThrottleInterval</key>\n\t<integer>30</integer>"));
    assert!(text.contains("<key>SuccessfulExit</key>\n\t\t<false/>"));
}

#[test]
fn plist_carries_no_switch() {
    let text = node_launchdaemon("tester", "/Users/tester", REAL).unwrap();
    let args: Vec<&str> = text
        .split("<key>ProgramArguments</key>")
        .nth(1)
        .unwrap()
        .split("</array>")
        .next()
        .unwrap()
        .lines()
        .filter_map(|l| l.trim().strip_prefix("<string>")?.strip_suffix("</string>"))
        .collect();
    assert_eq!(args, [REAL, "daemon"]);
    let env: Vec<&str> = text
        .split("<key>EnvironmentVariables</key>")
        .nth(1)
        .unwrap()
        .split("</dict>")
        .next()
        .unwrap()
        .lines()
        .filter_map(|l| l.trim().strip_prefix("<key>")?.strip_suffix("</key>"))
        .collect();
    assert_eq!(env, ["HOME", "PATH"]);
    assert!(!text.contains("NOTCHLING_"));
}

fn mac_fix(rig: &Rig, log: &Arc<Mutex<String>>) -> FixContext {
    let mut ctx = fix_of(rig, None, None);
    let sink = log.clone();
    ctx.log = Arc::new(move |_, t: &str| sink.lock().unwrap().push_str(t));
    ctx
}

#[test]
fn launchdaemon_fix_password_only_on_stdin() {
    let rig = Rig::new(node_mac());
    let log = Arc::new(Mutex::new(String::new()));
    rig.runner.on_with(
        |a| a.last().is_some_and(|s| s.contains("sudo")),
        ok("done\n"),
        vec![(super::runner::Channel::Stdout, "loaded\n".into())],
    );
    let ctx = mac_fix(&rig, &log);
    let service = step(&rig.machine, &StepId::NODE_SERVICE);
    assert!(plan(&service).needs_admin);
    let out = (plan(&service).run)(&ctx);
    assert!(out.ok, "{}", out.message);
    let calls = rig.calls();
    let admin = admin_calls(&calls);
    assert_eq!(admin.len(), 1, "{calls:?}");
    assert_eq!(admin[0].stdin_text(), format!("{PASSWORD}\n"));
    for call in &calls {
        assert!(!call.argv.join("\n").contains(PASSWORD), "{call:?}");
        assert!(
            call.env
                .iter()
                .all(|(k, v)| !k.contains(PASSWORD) && !v.contains(PASSWORD)),
            "{call:?}"
        );
        if !std::ptr::eq(call, admin[0]) {
            assert!(!call.stdin_text().contains(PASSWORD), "{call:?}");
        }
    }
    assert!(!log.lock().unwrap().contains(PASSWORD));
    assert!(!out.message.contains(PASSWORD));
    let script = unquoted(admin[0].argv.last().unwrap());
    assert!(script.contains("/usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin"));
    assert!(
        script.contains("install -m 644 -o root -g wheel"),
        "{script}"
    );
    assert!(script.contains("dev.notchling.node"), "{script}");
    assert!(script.contains(REAL), "{script}");
}

#[test]
fn launchdaemon_fix_without_the_password_fails_before_writing() {
    let rig = Rig::new(node_mac()).without_password();
    let ctx = fix_of(&rig, None, None);
    let out = (plan(&step(&rig.machine, &StepId::NODE_SERVICE)).run)(&ctx);
    assert!(!out.ok);
    assert!(out.message.contains("admin password"), "{}", out.message);
    assert!(rig.calls().is_empty(), "nothing runs without the password");
}

#[test]
fn launchdaemon_display_shows_the_heredoc_and_stops_work() {
    let m = node_mac();
    let shown = plan(&step(&m, &StepId::NODE_SERVICE)).display;
    assert!(
        shown.starts_with("Work running on this machine stops"),
        "{shown}"
    );
    assert!(shown.contains("<<'NOTCHLING_PLIST_END'"), "{shown}");
    assert!(shown.contains("<key>AbandonProcessGroup</key>"), "{shown}");
    assert!(shown.contains("plutil -lint"), "{shown}");
    assert!(shown.contains("install -m 644 -o root -g wheel"), "{shown}");
    let mut unknown = node_mac();
    unknown.abilities.as_mut().unwrap().home = None;
    let shown = plan(&step(&unknown, &StepId::NODE_SERVICE)).display;
    assert!(shown.contains("home folder isn't known yet"), "{shown}");
}

#[test]
fn a_heredoc_delimiter_in_the_content_is_refused() {
    let plain = install_script("<plist>\n</plist>\n").unwrap();
    assert!(plain.contains("<<'NOTCHLING_PLIST_END'\n<plist>\n</plist>\nNOTCHLING_PLIST_END\n"));
    let forged = "<plist>\nNOTCHLING_PLIST_END\nrm -rf /\n</plist>\n";
    assert!(install_script(forged).is_err());
    assert!(install_script("<string>xNOTCHLING_PLIST_ENDx</string>\n").is_err());
}

#[test]
fn launchdaemon_fix_refuses_an_unknown_home() {
    let mut m = node_mac();
    m.abilities.as_mut().unwrap().home = None;
    let rig = Rig::new(m);
    let ctx = fix_of(&rig, None, None);
    let out = (plan(&step(&rig.machine, &StepId::NODE_SERVICE)).run)(&ctx);
    assert!(!out.ok);
    assert!(out.message.contains("home folder"), "{}", out.message);
    assert!(admin_calls(&rig.calls()).is_empty());
}

#[test]
fn binary_fix_still_kickstarts_without_rereading_the_plist() {
    let rig = Rig::new(mac_arch(host_arch()));
    let tmp = TempDir::new("mac-kick");
    let daemon = tmp.path().join("notchling");
    std::fs::write(&daemon, BUILD).unwrap();
    rig.runner.on(
        |a| a.last().is_some_and(|s| s.contains("shasum -a 256")),
        ok(&format!("{}  new\n", hex(BUILD))),
    );
    let mut ctx = fix_of(&rig, None, None);
    ctx.step.mac_daemon = Some(daemon);
    let out = (plan(&step(&rig.machine, &StepId::NODE_BINARY)).run)(&ctx);
    assert!(out.ok, "{}", out.message);
    let calls = rig.calls();
    let admin = admin_calls(&calls);
    let last = admin.last().expect("an admin call").argv.last().unwrap();
    assert!(
        last.contains("launchctl kickstart -k system/dev.notchling.node"),
        "{last}"
    );
    assert!(
        !last.contains("bootout") && !last.contains("bootstrap"),
        "{last}"
    );
    let shown = plan(&step(&rig.machine, &StepId::NODE_BINARY)).display;
    assert!(shown.contains("kickstart -k"), "{shown}");
}

#[test]
fn old_launch_agent_is_booted_out_after_the_install() {
    let rig = Rig::new(node_mac());
    let ctx = fix_of(&rig, None, None);
    let out = (plan(&step(&rig.machine, &StepId::NODE_SERVICE)).run)(&ctx);
    assert!(out.ok, "{}", out.message);
    let calls = rig.calls();
    let seen = texts(&calls);
    let at = seen
        .iter()
        .position(|t| t.contains("launchctl bootout \"gui/"))
        .expect("a bootout");
    assert!(!seen[at].contains("sudo"), "{}", seen[at]);
    assert!(
        seen[at].contains("gui/$(id -u)/dev.notchling.daemon"),
        "{}",
        seen[at]
    );
    assert!(seen[at].contains("rm -f"), "{}", seen[at]);
    assert!(seen[at].contains(OLD_AGENT_PLIST.rsplit('/').next().unwrap()));
    let admin = seen.iter().position(|t| t.contains("sudo")).unwrap();
    assert!(admin < at, "{seen:?}");
    let shown = plan(&step(&rig.machine, &StepId::NODE_SERVICE)).display;
    assert!(shown.contains("launchctl bootout \"gui/"), "{shown}");
}

#[test]
fn a_failed_install_leaves_the_old_launch_agent_alone() {
    let rig = Rig::new(node_mac());
    rig.runner.on(
        |a| a.last().is_some_and(|s| s.contains("sudo")),
        result(1, "", "Sorry, try again."),
    );
    let ctx = fix_of(&rig, None, None);
    let out = (plan(&step(&rig.machine, &StepId::NODE_SERVICE)).run)(&ctx);
    assert!(!out.ok);
    let seen = texts(&rig.calls());
    assert!(
        seen.iter().any(|t| t.contains("sudo")),
        "the admin step ran: {seen:?}"
    );
    assert!(
        !seen.iter().any(|t| t.contains("dev.notchling.daemon")),
        "{seen:?}"
    );
}

#[cfg(unix)]
mod launchctl {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    fn run(loaded: bool) -> Vec<String> {
        let dir = TempDir::new("launchctl-node-stub");
        let log = dir.path().join("log");
        let stub = dir.path().join("launchctl");
        std::fs::write(
            &stub,
            "#!/bin/sh\necho \"$@\" >> \"$LOG\"\nif [ \"$1\" = print ]; then exit \"$PRINT_RC\"; fi\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let status = Command::new("/bin/sh")
            .arg("-c")
            .arg(load_script())
            .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
            .env("LOG", &log)
            .env("PRINT_RC", if loaded { "0" } else { "113" })
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn loaded_daemon_is_booted_out_and_bootstrapped_again() {
        let log = run(true);
        let at = |want: &str| {
            log.iter()
                .position(|l| l == want)
                .unwrap_or_else(|| panic!("{want}: {log:?}"))
        };
        let out = at("bootout system/dev.notchling.node");
        let enable = at("enable system/dev.notchling.node");
        let load = at(&format!("bootstrap system {NODE_PLIST}"));
        assert!(out < enable && enable < load, "{log:?}");
        assert!(!log.iter().any(|l| l.starts_with("kickstart")), "{log:?}");
    }

    #[test]
    fn bootout_that_finds_nothing_loaded_does_not_stop_the_fix() {
        let dir = TempDir::new("launchctl-node-stub-fail");
        let log = dir.path().join("log");
        let stub = dir.path().join("launchctl");
        std::fs::write(
            &stub,
            "#!/bin/sh\necho \"$@\" >> \"$LOG\"\nif [ \"$1\" = bootout ]; then exit 3; fi\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let status = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("set -e\n{}", load_script()))
            .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
            .env("LOG", &log)
            .status()
            .unwrap();
        assert!(status.success());
        let seen = std::fs::read_to_string(&log).unwrap();
        assert!(
            seen.contains(&format!("bootstrap system {NODE_PLIST}")),
            "{seen}"
        );
    }

    #[test]
    fn unloaded_daemon_is_bootstrapped_from_the_plist() {
        let log = run(false);
        assert!(
            log.iter()
                .any(|l| *l == format!("bootstrap system {NODE_PLIST}")),
            "{log:?}"
        );
        assert!(!log.iter().any(|l| l.starts_with("kickstart")), "{log:?}");
    }
}

fn service_text(
    user: &str,
    binary: &str,
    plist: &str,
    loaded: i32,
    old: i32,
    status: &str,
) -> String {
    format!(
        "plist={plist}\nuser={user}\nbinary={binary}\nloaded={loaded}\nold_agent={old}\nstatus_code={}\nstatus={status}\n",
        if status.is_empty() { 1 } else { 0 }
    )
}

#[test]
fn service_probe_passes_when_status_answers() {
    let m = node_mac();
    let script = remote(&step(&m, &StepId::NODE_SERVICE)).0;
    for needle in [
        "/Library/LaunchDaemons/dev.notchling.node.plist",
        "plutil -extract UserName raw",
        "plutil -extract ProgramArguments.0 raw",
        "launchctl print system/dev.notchling.node",
        "launchctl print gui/$(id -u)/dev.notchling.daemon",
        " status",
    ] {
        assert!(script.contains(needle), "{needle}: {script}");
    }
    let good = service_text("tester", REAL, "yes", 0, 113, "running, no cluster");
    let state = parse(&m, &StepId::NODE_SERVICE, &good, 0).state;
    assert_eq!(state, CheckState::Pass("running, no cluster".into()));

    let cases = [
        (
            service_text("tester", REAL, "no", 113, 113, ""),
            "no service file",
        ),
        (
            service_text("other", REAL, "yes", 0, 113, "up"),
            "runs as other",
        ),
        (
            service_text("tester", "/tmp/notchling", "yes", 0, 113, "up"),
            "another notchling",
        ),
        (
            service_text("tester", REAL, "yes", 113, 113, "up"),
            "isn't loaded",
        ),
        (
            service_text("tester", REAL, "yes", 0, 0, "up"),
            "old per-login",
        ),
        (
            service_text("tester", REAL, "yes", 0, 113, ""),
            "doesn't answer",
        ),
    ];
    for (text, why) in cases {
        let state = parse(&m, &StepId::NODE_SERVICE, &text, 0).state;
        assert!(state.is_fail(), "{why}: {state:?}");
        assert!(state.text().contains(why), "{why}: {state:?}");
    }
}

#[test]
fn app_running_fails_app_closed() {
    let m = mac_with_app();
    let id = StepId::NODE_APP_CLOSED;
    let script = remote(&step(&m, &id)).0;
    assert!(
        script.contains("pgrep -x notchling-app >/dev/null && echo running || echo closed"),
        "{script}"
    );
    let running = parse(&m, &id, "running\n", 0).state;
    assert!(running.is_fail(), "{running:?}");
    let closed = parse(&m, &id, "closed\n", 0).state;
    assert!(closed.is_pass(), "{closed:?}");
    let odd = parse(&m, &id, "pgrep: not found\n", 127).state;
    assert!(matches!(odd, CheckState::CantTell(_)), "{odd:?}");
    match step(&m, &id).fix {
        StepFix::HowTo(text) => {
            assert_eq!(text, "Quit Notchling on m2; the service keeps running.")
        }
        _ => panic!("quitting the app is a how-to"),
    }
}

#[test]
fn app_closed_step_absent_without_app() {
    let ids = |m: &Machine| -> Vec<StepId> {
        super::catalog::steps(m, std::slice::from_ref(m))
            .into_iter()
            .map(|s| s.id)
            .collect()
    };
    let with = ids(&mac_with_app());
    let closed = with
        .iter()
        .position(|i| *i == StepId::NODE_APP_CLOSED)
        .unwrap();
    let joined = with.iter().position(|i| *i == StepId::NODE_JOINED).unwrap();
    assert!(closed < joined);
    assert!(!ids(&node_mac()).contains(&StepId::NODE_APP_CLOSED));
    assert!(!ids(&node_linux()).contains(&StepId::NODE_APP_CLOSED));
    let mut unknown = node_mac();
    unknown.abilities.as_mut().unwrap().notchling_app = Some(false);
    assert!(!ids(&unknown).contains(&StepId::NODE_APP_CLOSED));
}
