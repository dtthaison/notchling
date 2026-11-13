use super::registry::{Machine, MachineOs};
use super::shell::*;
use crate::lanes::quote;

fn machine() -> Machine {
    Machine::new("vps", "vps.tail", "agent")
}

const PATH: &str = "$HOME/.local/bin:$HOME/.cargo/bin:$HOME/.local/share/mise/shims:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

#[test]
fn script_argv_runs_posix_sh_with_the_service_path() {
    assert_eq!(
        ssh_script(&machine(), "echo hi"),
        vec![
            "/usr/bin/ssh",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=8",
            "--",
            "agent@vps.tail",
            &format!("/bin/sh -c 'export PATH=\"{PATH}\"; echo hi'"),
        ]
    );
}

#[test]
fn argv_quotes_each_remote_word() {
    let argv = ssh_argv(&machine(), &["ls", "/tmp/a b", "it's"]);
    assert_eq!(argv.last().unwrap(), "'ls' '/tmp/a b' 'it'\\''s'");
}

fn assert_nothing_of_the_workers_own(argv: &[String]) {
    let text = argv.join("\n");
    let prelude = prelude();
    for needle in [
        prelude.as_str(),
        "export PATH",
        "$HOME",
        ".local/bin",
        ".cargo",
        "mise",
        "/opt/homebrew",
        "/usr/local",
    ] {
        assert!(
            !text.contains(needle),
            "an admin command carries {needle}: {text}"
        );
    }
}

#[test]
fn admin_through_sudo_calls_the_absolute_sudo_with_a_fixed_environment() {
    let mut mac = machine();
    mac.os = Some(MachineOs::MacOs);
    let argv = ssh_admin(&mac, "pmset -a sleep 0", false);
    assert_eq!(argv[argv.len() - 2], "agent@vps.tail");
    let inner = format!(
        "/usr/bin/sudo -S -k -p '' /usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin HOME=/var/root /bin/sh -c {}",
        quote("exec </dev/null; pmset -a sleep 0")
    );
    assert_eq!(
        argv.last().unwrap(),
        &format!("/bin/sh -c {}", quote(&inner))
    );
    assert_nothing_of_the_workers_own(&argv);
}

#[test]
fn admin_through_sudo_on_linux_uses_roots_home() {
    let mut linux = machine();
    linux.os = Some(MachineOs::Linux);
    let argv = ssh_admin(&linux, "apt-get install -y git", false);
    let last = argv.last().unwrap();
    assert!(
        last.contains("/usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin HOME=/root /bin/sh -c"),
        "{last}"
    );
    assert!(last.contains("/usr/bin/sudo -S -k -p "), "{last}");
    assert_nothing_of_the_workers_own(&argv);
}

#[test]
fn admin_as_root_over_ssh_has_no_sudo_and_a_fixed_environment() {
    let mut linux = machine();
    linux.os = Some(MachineOs::Linux);
    let argv = ssh_admin(&linux, "apt-get update", true);
    assert_eq!(argv[argv.len() - 2], "root@vps.tail");
    assert!(!argv.last().unwrap().contains("sudo"));
    let inner = format!(
        "/usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin HOME=/root /bin/sh -c {}",
        quote("exec </dev/null; apt-get update")
    );
    assert_eq!(
        argv.last().unwrap(),
        &format!("/bin/sh -c {}", quote(&inner))
    );
    assert_nothing_of_the_workers_own(&argv);
}

#[test]
fn a_hostile_script_stays_inside_one_quoted_word() {
    let script = "echo 'a'; $(id) `id`\nrm -rf ~";
    let argv = ssh_script(&machine(), script);
    assert_eq!(argv.len(), 8);
    let expected = format!("/bin/sh -c {}", quote(&format!("{}{script}", prelude())));
    assert_eq!(argv[7], expected);
}

#[test]
fn remote_path_quoting() {
    assert_eq!(path("~/src/my app"), "\"$HOME\"/'src/my app'");
    assert_eq!(path("~"), "\"$HOME\"");
    assert_eq!(path("/opt/x y"), "'/opt/x y'");
    assert_eq!(path("~other/x"), "'~other/x'");
    assert_eq!(path("/a/b'c"), "'/a/b'\\''c'");
}

#[test]
fn unsafe_machine_paths_are_refused() {
    assert!(is_safe_machine_path("/home/agent"));
    assert!(is_safe_machine_path("/Users/sondoan/.local/bin/python3"));
    for bad in [
        "/Users/so doan",
        "/home/a;rm -rf ~",
        "relative",
        "/x/../y",
        "/x\ny",
        "",
        "/home/$USER",
        "/a/b'c",
        "/a/`id`",
        "/a/$(id)",
    ] {
        assert!(!is_safe_machine_path(bad), "{bad:?}");
    }
    assert!(!is_safe_machine_path(&format!("/{}", "a".repeat(600))));
}

#[test]
fn absolute_paths_only_from_a_safe_home() {
    assert_eq!(
        absolute("~/.config/agents/claude.env", "/home/agent").as_deref(),
        Some("/home/agent/.config/agents/claude.env")
    );
    assert_eq!(absolute("~", "/home/agent").as_deref(), Some("/home/agent"));
    assert_eq!(absolute("/etc/x", "/home/agent").as_deref(), Some("/etc/x"));
    assert_eq!(absolute("~/x", "/home/a b;x"), None);
    assert_eq!(absolute("~/x", "relative"), None);
}

#[test]
fn write_file_script() {
    assert_eq!(
        write_file("~/.config/agents/github-token", "600"),
        "umask 077; mkdir -p \"$HOME\"/'.config/agents' && cat > \"$HOME\"/'.config/agents/github-token.new' && chmod 600 \"$HOME\"/'.config/agents/github-token.new' && mv -f \"$HOME\"/'.config/agents/github-token.new' \"$HOME\"/'.config/agents/github-token'"
    );
    assert!(write_file("/etc/x/y", "644").contains("chmod 644 '/etc/x/y.new'"));
}

#[test]
fn stat_script_and_parse() {
    assert!(stat(&["~/a", "/b"], true).contains("stat -c '%a %s'"));
    assert!(stat(&["~/a"], false).contains("stat -f '%Lp %z'"));
    assert_eq!(
        stat(&["/b"], true),
        "if [ -f '/b' ]; then stat -c '%a %s' '/b'; else echo missing; fi"
    );
    assert_eq!(
        parse_stat("600 41\nmissing\n", 2),
        vec![
            Some(FileFacts {
                mode: "600".into(),
                size: 41
            }),
            None
        ]
    );
    assert_eq!(parse_stat("x", 1), vec![None]);
    assert_eq!(parse_stat("", 2), vec![None, None]);
}

#[test]
fn sha256_script_prints_only_the_hash() {
    let script = sha256("~/.config/agents/claude.env", true);
    assert!(script.contains("sha256sum"));
    assert!(script.contains("cut -d' ' -f1"));
    assert!(!script.contains("cat "));
    assert!(sha256("/a", false).contains("shasum -a 256"));
}

#[test]
fn service_path_expands_home() {
    let p = service_path("/home/agent");
    assert!(p.starts_with("/home/agent/.local/bin:/home/agent/.cargo/bin:/home/agent/.local/share/mise/shims:/opt/homebrew/bin"));
    assert!(!p.contains('$'));
}

#[test]
fn git_urls_need_an_absolute_path() {
    let m = machine();
    assert_eq!(
        git_url(&m, "/home/agent/src/x").as_deref(),
        Some("ssh://agent@vps.tail/home/agent/src/x")
    );
    assert_eq!(git_url(&m, ".evil.com/x"), None);
}

#[test]
fn admin_is_refused_for_the_worker_on_linux() {
    let mut linux = machine();
    linux.os = Some(MachineOs::Linux);
    assert!(admin_refusal(&linux).is_some());
    let mut mac = machine();
    mac.os = Some(MachineOs::MacOs);
    assert!(admin_refusal(&mac).is_none());
}
