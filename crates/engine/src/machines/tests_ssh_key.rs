use super::addresses::{AddressKind, MachineAddress, Route};
use super::fakes::TempDir;
use super::reach::Router;
use super::registry::{Machine, RegistryFiles};
use super::runner::{CommandResult, ScriptedRunner};
use super::shell::{git_ssh_command, ssh_admin, ssh_script};
use super::ssh_key::*;
use super::tailscale::{import, parse_status, Picked, Tailscale};
use std::path::Path;
use std::sync::{Arc, Mutex};

fn write_key(dir: &TempDir, name: &str, mode: u32) -> String {
    let path = dir.join(name);
    std::fs::write(&path, "not a real key, a fixture").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    let _ = mode;
    path.to_string_lossy().into_owned()
}

#[test]
fn a_key_path_is_absolute_or_under_home() {
    assert!(shape_ok("/Users/x/.ssh/id_ed25519"));
    assert!(shape_ok("~/.ssh/id_work"));
    assert!(shape_ok("  ~/.ssh/id_work  "));
    for bad in [
        "",
        "   ",
        "id_rsa",
        "./id_rsa",
        "~",
        "~other/.ssh/id",
        "-oProxyCommand=x",
        "/a/b/",
        "/a/b\nc",
    ] {
        assert!(!shape_ok(bad), "{bad:?}");
    }
    assert!(!shape_ok(&format!("/{}", "a".repeat(MAX_LEN))));
}

#[test]
fn the_tilde_stays_in_what_is_stored() {
    let home = Path::new("/Users/sondoan");
    assert_eq!(tidy("/Users/sondoan/.ssh/id_work", home), "~/.ssh/id_work");
    assert_eq!(tidy("  ~/.ssh/id_work ", home), "~/.ssh/id_work");
    assert_eq!(tidy("/Users/sondoan", home), "/Users/sondoan");
    assert_eq!(tidy("/Users/sondoan2/k", home), "/Users/sondoan2/k");
    assert_eq!(tidy("/elsewhere/k", home), "/elsewhere/k");
    assert_eq!(
        expand("~/.ssh/id_work", home),
        Path::new("/Users/sondoan/.ssh/id_work")
    );
    assert_eq!(expand("/k", home), Path::new("/k"));
    assert_eq!(file_name("~/.ssh/id_work"), "id_work");
}

#[test]
fn a_key_file_is_checked_without_reading_it() {
    let dir = TempDir::new("key");
    let ok = write_key(&dir, "id_ok", 0o600);
    assert_eq!(
        inspect(&ok, dir.path()),
        KeyStatus {
            issue: None,
            public: false
        }
    );
    let under_home = inspect("~/id_ok", dir.path());
    assert!(under_home.is_usable());
    assert_eq!(
        inspect("~/missing", dir.path()).issue,
        Some(KeyIssue::Missing)
    );
    assert_eq!(
        inspect(&dir.path().to_string_lossy(), dir.path()).issue,
        Some(KeyIssue::NotAFile)
    );
    assert_eq!(
        inspect("relative", dir.path()).issue,
        Some(KeyIssue::BadShape)
    );
}

#[cfg(unix)]
#[test]
fn a_key_others_can_read_is_refused_and_a_public_key_only_warns() {
    let dir = TempDir::new("key");
    let open = write_key(&dir, "id_open", 0o644);
    assert_eq!(inspect(&open, dir.path()).issue, Some(KeyIssue::TooOpen));
    let group = write_key(&dir, "id_group", 0o640);
    assert_eq!(inspect(&group, dir.path()).issue, Some(KeyIssue::TooOpen));
    let public = write_key(&dir, "id_ok.pub", 0o644);
    let status = inspect(&public, dir.path());
    assert!(status.public);
    assert_eq!(status.issue, None);
}

fn machine_with_key() -> Machine {
    let mut m = Machine::new("m1", "m1.tail0a1b2.ts.net", "agent");
    m.set_addresses(vec![
        MachineAddress::new(AddressKind::TailscaleName, "m1.tail0a1b2.ts.net"),
        MachineAddress::new(AddressKind::Lan, "192.168.1.20").with_port(2222),
    ]);
    m.set_ssh_key(Some(" ~/.ssh/id_work "));
    m
}

#[test]
fn a_key_adds_identity_options_to_ssh_scp_and_the_admin_call() {
    let mut m = machine_with_key();
    m.route = Some(Route::direct("m1.tail0a1b2.ts.net", "agent"));
    let route = m.route();
    assert_eq!(route.key.as_deref(), Some("~/.ssh/id_work"));
    let want = ["-i", "~/.ssh/id_work", "-o", "IdentitiesOnly=yes"];
    assert_eq!(route.ssh_options(), want);
    assert_eq!(route.scp_options(), want);
    let at = |argv: &[String]| argv.iter().position(|a| a == "-i").expect("-i");
    let script = ssh_script(&m, "echo hi");
    assert_eq!(script[at(&script) + 1], "~/.ssh/id_work");
    assert!(script.iter().position(|a| a == "--").unwrap() > at(&script));
    let admin = ssh_admin(&m, "id", true);
    assert_eq!(admin[at(&admin) + 1], "~/.ssh/id_work");
    assert!(admin.contains(&"root@m1.tail0a1b2.ts.net".to_owned()));
}

#[test]
fn a_machine_without_a_key_has_no_identity_options() {
    let m = Machine::new("m1", "m1.tail", "agent");
    assert!(!ssh_script(&m, "x").iter().any(|a| a == "-i"));
    assert!(m.route().ssh_options().is_empty());
    let mut blank = m.clone();
    blank.set_ssh_key(Some("   "));
    assert_eq!(blank.ssh_key, None);
}

#[test]
fn a_key_goes_into_git_over_ssh_too() {
    let m = machine_with_key();
    assert_eq!(
        git_ssh_command(&m),
        "ssh -o BatchMode=yes -o ConnectTimeout=8 -i '~/.ssh/id_work' -o 'IdentitiesOnly=yes'"
    );
}

#[test]
fn a_keyless_jump_stays_a_dash_j_and_a_keyed_hop_becomes_a_proxy_command() {
    let plain = Route {
        jump: Some(Box::new(Route::direct("gw.example", "hop"))),
        ..Route::direct("10.0.0.5", "agent")
    };
    assert_eq!(plain.ssh_options(), ["-J", "hop@gw.example"]);
    let keyed = Route {
        jump: Some(Box::new(
            Route::direct("gw.example", "hop").with_key(Some("~/.ssh/id_gw")),
        )),
        ..Route::direct("10.0.0.5", "agent")
    };
    let options = keyed.ssh_options();
    assert_eq!(options[0], "-o");
    assert!(!options.contains(&"-J".to_owned()));
    let proxy = options[1].strip_prefix("ProxyCommand=").expect("proxy");
    assert_eq!(
        proxy,
        "'/usr/bin/ssh' '-o' 'BatchMode=yes' '-o' 'ConnectTimeout=8' '-i' '~/.ssh/id_gw' '-o' 'IdentitiesOnly=yes' '-W' '%h:%p' '--' 'hop@gw.example'"
    );
    let both = Route {
        key: Some("~/.ssh/id_dest".into()),
        ..keyed
    };
    let options = both.ssh_options();
    assert_eq!(
        &options[..4],
        ["-i", "~/.ssh/id_dest", "-o", "IdentitiesOnly=yes"]
    );
    assert!(options[5].starts_with("ProxyCommand="));
}

#[test]
fn a_chain_with_a_key_on_the_far_hop_nests_proxy_commands() {
    let far = Route::direct("edge.example", "edge").with_key(Some("~/.ssh/id_edge"));
    let near = Route {
        jump: Some(Box::new(far)),
        port: Some(2200),
        ..Route::direct("gw.example", "hop")
    };
    let route = Route {
        jump: Some(Box::new(near)),
        ..Route::direct("10.0.0.5", "agent")
    };
    let options = route.ssh_options();
    let proxy = options[1].strip_prefix("ProxyCommand=").unwrap();
    assert!(proxy.contains("'-p' '2200'"), "{proxy}");
    assert!(
        proxy.contains("ProxyCommand=") && proxy.contains("id_edge"),
        "{proxy}"
    );
    assert!(!proxy.contains("'-J'"), "{proxy}");
}

fn rig(keyed: bool) -> (Router, Arc<ScriptedRunner>, Machine) {
    let runner = Arc::new(ScriptedRunner::new());
    runner.on(
        |a| a.get(1).map(String::as_str) == Some("switch"),
        CommandResult::ok(
            "ID    Tailnet              Account\na1b2  person@example.com   person@example.com*\n",
        ),
    );
    runner.on(
        |a| a.last().map(String::as_str) == Some("true"),
        CommandResult::ok(""),
    );
    let tailscale = Tailscale::new(runner.clone(), "tailscale");
    let clock = Arc::new(Mutex::new(0.0_f64));
    let router = Router::new(
        runner.clone(),
        tailscale,
        Arc::new(move || *clock.lock().unwrap()),
    );
    let mut m = Machine::new("m1", "m1.tail", "agent");
    m.set_addresses(vec![
        MachineAddress::new(AddressKind::Lan, "10.0.0.8"),
        MachineAddress::new(AddressKind::Public, "app.example.org").with_port(2222),
    ]);
    if keyed {
        m.set_ssh_key(Some("~/.ssh/id_work"));
    }
    (router, runner, m)
}

#[test]
fn the_reach_probe_and_the_chosen_route_carry_the_key() {
    let (router, runner, m) = rig(true);
    let all = vec![m.clone()];
    let routed = router.reached(&m, &all);
    assert_eq!(
        routed.route().key.as_deref(),
        Some("~/.ssh/id_work"),
        "{:?}",
        routed.route()
    );
    let probe = runner
        .calls()
        .into_iter()
        .find(|c| c.argv.last().map(String::as_str) == Some("true"))
        .expect("a probe");
    let at = probe
        .argv
        .iter()
        .position(|a| a == "-i")
        .expect("-i in the probe");
    assert_eq!(probe.argv[at + 1], "~/.ssh/id_work");
    assert_eq!(probe.argv[at + 2], "-o");
    assert_eq!(probe.argv[at + 3], "IdentitiesOnly=yes");
}

#[test]
fn changing_the_key_drops_the_remembered_route() {
    let (router, _runner, m) = rig(true);
    let all = vec![m.clone()];
    router.reached(&m, &all);
    assert!(router.cached(&m).is_some());
    let mut other = m.clone();
    other.set_ssh_key(Some("~/.ssh/id_other"));
    assert!(router.cached(&other).is_none());
    let mut none = m.clone();
    none.set_ssh_key(None);
    assert!(router.cached(&none).is_none());
}

#[test]
fn every_address_of_a_machine_is_a_test_target_with_the_key() {
    use super::ssh_test::Target;
    let (router, _runner, m) = rig(true);
    let targets = router.targets(&m, std::slice::from_ref(&m));
    assert_eq!(targets.len(), 2);
    for (_, target) in &targets {
        let Target::Route(route) = target else {
            panic!("{target:?}")
        };
        assert_eq!(route.key.as_deref(), Some("~/.ssh/id_work"));
    }
    let Target::Route(second) = &targets[1].1 else {
        unreachable!()
    };
    assert_eq!(second.port, Some(2222));
}

#[test]
fn a_machine_file_without_a_key_loads_and_one_with_a_key_keeps_only_the_path() {
    let dir = TempDir::new("keyreg");
    let reg = RegistryFiles::single(dir.path());
    let old = r#"{"version":4,"machines":[{"name":"m1","host":"m1","user":"sondoan","jobs":[]}]}"#;
    std::fs::write(&reg.old_own, old).unwrap();
    let loaded = reg.all();
    assert_eq!(loaded.len(), 1, "{loaded:?}");
    assert_eq!(loaded[0].ssh_key, None);
    let mut keyed = loaded[0].clone();
    keyed.set_ssh_key(Some("~/.ssh/id_work"));
    assert!(reg.save(&[keyed.clone()]));
    assert_eq!(reg.all()[0].ssh_key.as_deref(), Some("~/.ssh/id_work"));
    let text = serde_json::to_string(&keyed).unwrap();
    assert!(text.contains("\"sshKey\":\"~/.ssh/id_work\""), "{text}");
    let plain = serde_json::to_string(&loaded[0]).unwrap();
    assert!(!plain.contains("sshKey"), "{plain}");
}

const STATUS: &str = r#"{
  "BackendState": "Running",
  "CurrentTailnet": {"Name": "person@example.com"},
  "Peer": {
    "nodekey:aa": {"HostName": "app-1", "DNSName": "app-1.tail1234.ts.net.", "OS": "linux", "UserID": 1, "TailscaleIPs": ["100.64.0.11"], "Online": true}
  },
  "User": {"1": {"LoginName": "person@example.com"}}
}"#;

#[test]
fn an_import_row_can_carry_a_key_and_a_bad_path_is_refused() {
    let status = parse_status(STATUS).unwrap();
    let pick = |key: Option<&str>| Picked {
        host: "app-1.tail1234.ts.net".into(),
        user: "ubuntu".into(),
        key: key.map(str::to_owned),
    };
    let made = import(&status, &[pick(Some(" ~/.ssh/id_work "))], &[], None).unwrap();
    assert_eq!(made[0].ssh_key.as_deref(), Some("~/.ssh/id_work"));
    let plain = import(&status, &[pick(None)], &[], None).unwrap();
    assert_eq!(plain[0].ssh_key, None);
    let bad = import(&status, &[pick(Some("-oProxyCommand=x"))], &[], None);
    assert!(bad.unwrap_err().contains("SSH key"));
}
