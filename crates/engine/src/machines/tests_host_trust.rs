use super::addresses::Route;
use super::fakes::TempDir;
use super::host_trust::*;
use super::runner::{CommandResult, ScriptedRunner};

const ED25519: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";
const RSA: &str = "AAAAB3NzaC1yc2EAAAADAQABAAAAgQDfake";

fn route() -> Route {
    Route::direct("app-1.tail1234.ts.net", "ubuntu")
}

fn scan_out(host: &str) -> String {
    format!(
        "# {host}:22 SSH-2.0-OpenSSH_9.6\n{host} ssh-ed25519 {ED25519}\n# {host}:22 SSH-2.0-OpenSSH_9.6\n"
    )
}

#[test]
fn the_scan_asks_ssh_keyscan_for_the_host_and_port_only() {
    let argv = scan_argv(&route());
    assert_eq!(
        argv,
        [
            "/usr/bin/ssh-keyscan",
            "-T",
            "5",
            "--",
            "app-1.tail1234.ts.net"
        ]
    );
    let r = Route {
        port: Some(2222),
        ..route()
    };
    assert_eq!(&scan_argv(&r)[3..5], ["-p", "2222"]);
}

#[test]
fn a_scan_reads_the_key_and_works_out_its_fingerprint() {
    let keys = parse_scan(&scan_out("app-1.tail1234.ts.net"), &route());
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].kind, "ssh-ed25519");
    assert_eq!(keys[0].label(), "ED25519");
    assert!(keys[0].fingerprint.starts_with("SHA256:"));
    assert_eq!(keys[0].fingerprint.len(), "SHA256:".len() + 43);
    assert_eq!(
        keys[0].line(),
        format!("app-1.tail1234.ts.net ssh-ed25519 {ED25519}")
    );
}

#[test]
fn a_port_makes_the_known_hosts_field_bracketed() {
    let r = Route {
        port: Some(2222),
        ..route()
    };
    let out = format!("[app-1.tail1234.ts.net]:2222 ssh-ed25519 {ED25519}\n");
    let keys = parse_scan(&out, &r);
    assert_eq!(keys[0].field, "[app-1.tail1234.ts.net]:2222");
}

#[test]
fn lines_for_another_host_or_with_a_mismatched_key_are_dropped() {
    let other = format!("other.example ssh-ed25519 {ED25519}\n");
    assert!(parse_scan(&other, &route()).is_empty());
    let lying = format!("app-1.tail1234.ts.net ssh-rsa {ED25519}\n");
    assert!(parse_scan(&lying, &route()).is_empty());
    let junk = "app-1.tail1234.ts.net ssh-ed25519 !!!not-base64!!!\n";
    assert!(parse_scan(junk, &route()).is_empty());
    let extra = format!("app-1.tail1234.ts.net ssh-ed25519 {ED25519} comment\n");
    assert!(parse_scan(&extra, &route()).is_empty());
    let _ = RSA;
}

#[test]
fn a_scan_goes_through_the_runner_and_never_through_a_jump() {
    let runner = ScriptedRunner::new();
    runner.on(
        |a| a[0] == "/usr/bin/ssh-keyscan",
        CommandResult::ok(&scan_out("app-1.tail1234.ts.net")),
    );
    assert_eq!(scan(&runner, &route()).unwrap().len(), 1);
    let jumped = Route {
        jump: Some(Box::new(Route::direct("gw.example", "hop"))),
        ..route()
    };
    assert_eq!(scan(&runner, &jumped), Err(ScanFailure::Jump));
    assert_eq!(runner.calls().len(), 1);
    let silent = ScriptedRunner::new();
    silent.on(|_| true, CommandResult::new(1, "", "timed out"));
    assert_eq!(scan(&silent, &route()), Err(ScanFailure::Nothing));
}

#[test]
fn trusting_appends_the_shown_key_to_known_hosts_once() {
    let home = TempDir::new("trust");
    let keys = parse_scan(&scan_out("app-1.tail1234.ts.net"), &route());
    assert_eq!(trust(home.path(), &keys), Ok(1));
    let file = known_hosts_path(home.path());
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        text,
        format!("app-1.tail1234.ts.net ssh-ed25519 {ED25519}\n")
    );
    assert_eq!(trust(home.path(), &keys), Ok(0));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
}

#[test]
fn trusting_keeps_what_is_already_in_known_hosts() {
    let home = TempDir::new("trust");
    let ssh = home.path().join(".ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    std::fs::write(ssh.join("known_hosts"), "old.example ssh-rsa AAAA").unwrap();
    let keys = parse_scan(&scan_out("app-1.tail1234.ts.net"), &route());
    assert_eq!(trust(home.path(), &keys), Ok(1));
    let text = std::fs::read_to_string(ssh.join("known_hosts")).unwrap();
    assert!(
        text.starts_with("old.example ssh-rsa AAAA\napp-1"),
        "{text}"
    );
}

#[cfg(unix)]
#[test]
fn a_new_known_hosts_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let home = TempDir::new("trust");
    let keys = parse_scan(&scan_out("app-1.tail1234.ts.net"), &route());
    trust(home.path(), &keys).unwrap();
    let mode = |p: std::path::PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(home.path().join(".ssh")), 0o700);
    assert_eq!(mode(known_hosts_path(home.path())), 0o600);
}

#[test]
fn nothing_to_trust_writes_nothing() {
    let home = TempDir::new("trust");
    assert_eq!(trust(home.path(), &[]), Ok(0));
    assert!(!known_hosts_path(home.path()).exists());
}
