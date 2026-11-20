use super::ssh_command::*;

fn parsed(text: &str) -> SshTarget {
    parse_ssh_command(text).unwrap_or_else(|why| panic!("{text}: {why}"))
}

fn target(user: Option<&str>, host: &str, port: Option<u16>) -> SshTarget {
    SshTarget {
        user: user.map(str::to_owned),
        host: host.into(),
        port,
        key: None,
        jump: None,
    }
}

#[test]
fn port_before_the_destination() {
    assert_eq!(
        parsed("ssh -p 234 stackops@203.0.113.7"),
        target(Some("stackops"), "203.0.113.7", Some(234))
    );
}

#[test]
fn port_after_the_destination() {
    assert_eq!(
        parsed("ssh stackops@203.0.113.7 -p 234"),
        target(Some("stackops"), "203.0.113.7", Some(234))
    );
}

#[test]
fn port_attached_to_its_flag() {
    assert_eq!(
        parsed("ssh -p234 agent@box.example.test"),
        target(Some("agent"), "box.example.test", Some(234))
    );
}

#[test]
fn key_and_port() {
    let got = parsed("ssh -i ~/.ssh/id_x -p 2222 user@host.example.test");
    assert_eq!(got.key.as_deref(), Some("~/.ssh/id_x"));
    assert_eq!(got.port, Some(2222));
    assert_eq!(got.user.as_deref(), Some("user"));
    assert_eq!(got.host, "host.example.test");
}

#[test]
fn key_with_an_absolute_path() {
    assert_eq!(
        parsed("ssh -i /keys/id_a user@h.example.test")
            .key
            .as_deref(),
        Some("/keys/id_a")
    );
}

#[test]
fn jump_host() {
    let got = parsed("ssh -J bastion user@inner.example.test");
    assert_eq!(got.jump.as_deref(), Some("bastion"));
    assert_eq!(got.host, "inner.example.test");
}

#[test]
fn jump_host_with_its_own_user_and_port() {
    let got = parsed("ssh -J me@bastion:2200 user@inner.example.test");
    assert_eq!(got.jump.as_deref(), Some("bastion"));
}

#[test]
fn port_as_an_option() {
    assert_eq!(
        parsed("ssh -o Port=234 user@host.example.test").port,
        Some(234)
    );
    assert_eq!(
        parsed("ssh -oPort=234 user@host.example.test").port,
        Some(234)
    );
}

#[test]
fn other_named_options() {
    let got = parsed("ssh -o IdentityFile=~/.ssh/k -o ProxyJump=hop -o User=ann host.example.test");
    assert_eq!(got.key.as_deref(), Some("~/.ssh/k"));
    assert_eq!(got.jump.as_deref(), Some("hop"));
    assert_eq!(got.user.as_deref(), Some("ann"));
}

#[test]
fn login_flag() {
    assert_eq!(
        parsed("ssh -l user host.example.test"),
        target(Some("user"), "host.example.test", None)
    );
}

#[test]
fn bare_forms() {
    assert_eq!(
        parsed("user@host.example.test:2222"),
        target(Some("user"), "host.example.test", Some(2222))
    );
    assert_eq!(
        parsed("host.example.test"),
        target(None, "host.example.test", None)
    );
    assert_eq!(
        parsed("[2001:db8::7]:2200"),
        target(None, "2001:db8::7", Some(2200))
    );
    assert_eq!(parsed("[2001:db8::7]"), target(None, "2001:db8::7", None));
    assert_eq!(
        parsed("a@2001:db8::7"),
        target(Some("a"), "2001:db8::7", None)
    );
}

#[test]
fn ssh_uri() {
    assert_eq!(
        parsed("ssh://user@host.example.test:2222"),
        target(Some("user"), "host.example.test", Some(2222))
    );
    assert_eq!(
        parsed("ssh://host.example.test/"),
        target(None, "host.example.test", None)
    );
    assert_eq!(
        parsed("ssh ssh://u@[2001:db8::1]:22"),
        target(Some("u"), "2001:db8::1", Some(22))
    );
}

#[test]
fn harmless_flags_are_ignored() {
    assert_eq!(
        parsed("ssh -4 -C -v user@host.example.test"),
        target(Some("user"), "host.example.test", None)
    );
}

#[test]
fn double_dash_ends_the_options() {
    assert_eq!(
        parsed("ssh -p 22 -- user@host.example.test"),
        target(Some("user"), "host.example.test", Some(22))
    );
}

#[test]
fn extra_whitespace_is_fine() {
    assert_eq!(
        parsed("  ssh   -p   234   user@host.example.test  "),
        target(Some("user"), "host.example.test", Some(234))
    );
}

#[test]
fn the_same_port_twice_is_fine() {
    assert_eq!(parsed("ssh -p 22 user@host.example.test:22").port, Some(22));
}

#[test]
fn unknown_flags_are_named() {
    let why = parse_ssh_command("ssh -X user@host.example.test").unwrap_err();
    assert!(why.contains("-X"), "{why}");
    let why = parse_ssh_command("ssh --verbose user@host.example.test").unwrap_err();
    assert!(why.contains("--verbose"), "{why}");
    let why = parse_ssh_command("ssh -L 80:x:80 user@host.example.test").unwrap_err();
    assert!(why.contains("-L"), "{why}");
    let why =
        parse_ssh_command("ssh -o StrictHostKeyChecking=no user@host.example.test").unwrap_err();
    assert!(why.contains("StrictHostKeyChecking"), "{why}");
}

#[test]
fn a_missing_host_is_an_error() {
    assert!(parse_ssh_command("ssh -p 22").is_err());
    assert!(parse_ssh_command("ssh").is_err());
    assert!(parse_ssh_command("").is_err());
    assert!(parse_ssh_command("   ").is_err());
    assert!(parse_ssh_command("user@").is_err());
    assert!(parse_ssh_command("@host.example.test").is_err());
}

#[test]
fn a_bad_port_is_an_error() {
    for text in [
        "ssh -p 0 user@h.example.test",
        "ssh -p 65536 user@h.example.test",
        "ssh -p 99999999 user@h.example.test",
        "ssh -p abc user@h.example.test",
        "ssh -o Port=0 user@h.example.test",
        "user@h.example.test:0",
        "user@h.example.test:65536",
        "user@h.example.test:",
        "[2001:db8::7]:0",
    ] {
        let why = parse_ssh_command(text).unwrap_err();
        assert!(why.contains("1 to 65535"), "{text}: {why}");
    }
    assert!(parse_ssh_command("ssh -p -1 user@h.example.test").is_err());
    assert_eq!(parsed("ssh -p 65535 h.example.test").port, Some(65535));
    assert_eq!(parsed("ssh -p 1 h.example.test").port, Some(1));
}

#[test]
fn a_command_after_the_host_is_an_error() {
    let why = parse_ssh_command("ssh host.example.test ls").unwrap_err();
    assert!(why.contains("ls"), "{why}");
    assert!(parse_ssh_command("ssh -p 22 user@host.example.test uptime now").is_err());
}

#[test]
fn text_that_is_not_ssh_is_an_error() {
    assert!(parse_ssh_command("scp file user@host.example.test").is_err());
    assert!(parse_ssh_command("user@host.example.test extra").is_err());
}

#[test]
fn shell_syntax_is_refused_not_run() {
    for text in [
        "ssh user@host.example.test; rm -rf /",
        "ssh user@$(whoami).example.test",
        "ssh -i $HOME/.ssh/k user@h.example.test",
        "ssh user@h.example.test | cat",
        "ssh 'user@h.example.test'",
        "ssh user@h.example.test > out",
        "ssh user@`id`",
    ] {
        assert!(parse_ssh_command(text).is_err(), "{text}");
    }
}

#[test]
fn control_characters_are_refused() {
    assert!(parse_ssh_command("ssh user@host.example.test\n-p 22").is_err());
    assert!(parse_ssh_command("ssh\u{0}user@h.example.test").is_err());
}

#[test]
fn bad_keys_are_refused() {
    assert!(parse_ssh_command("ssh -i id_x user@h.example.test").is_err());
    assert!(parse_ssh_command("ssh -i ~/a -i ~/b user@h.example.test").is_err());
    assert!(parse_ssh_command("ssh -i").is_err());
}

#[test]
fn bad_jumps_are_refused() {
    assert!(parse_ssh_command("ssh -J a,b user@h.example.test").is_err());
    assert!(parse_ssh_command("ssh -J a -J b user@h.example.test").is_err());
    assert!(parse_ssh_command("ssh -J bad/host user@h.example.test").is_err());
}

#[test]
fn conflicting_values_are_refused() {
    assert!(parse_ssh_command("ssh -p 22 user@h.example.test:23").is_err());
    assert!(parse_ssh_command("ssh -p 22 -p 23 h.example.test").is_err());
    assert!(parse_ssh_command("ssh -l a b@h.example.test").is_err());
    assert_eq!(
        parsed("ssh -l a a@h.example.test").user.as_deref(),
        Some("a")
    );
}

#[test]
fn bad_hosts_and_users_are_refused() {
    assert!(parse_ssh_command("ssh user@bad_host!").is_err());
    assert!(parse_ssh_command("ssh BadUser@h.example.test").is_err());
    assert!(parse_ssh_command("ssh a@b@h.example.test").is_err());
    assert!(parse_ssh_command("ssh [2001:db8::7 user").is_err());
    assert!(parse_ssh_command("[2001:db8::zz]").is_err());
    assert!(parse_ssh_command("ssh://user@host.example.test/path").is_err());
}

#[test]
fn canonical_text_round_trips() {
    for text in [
        "u@h.example.test",
        "u@h.example.test:2222",
        "h.example.test",
        "[2001:db8::7]:2200",
        "u@[2001:db8::7]",
    ] {
        let first = parsed(text);
        assert_eq!(parsed(&first.canonical()), first, "{text}");
    }
    assert_eq!(
        parsed("ssh -p 234 stackops@203.0.113.7").canonical(),
        "stackops@203.0.113.7:234"
    );
}
