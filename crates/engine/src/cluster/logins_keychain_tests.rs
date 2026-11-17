use super::*;
use std::path::PathBuf;
use std::sync::Mutex;

struct Scripted {
    calls: Mutex<Vec<Vec<String>>>,
    keychain: Option<(i32, String)>,
}

impl StatusRunner for Scripted {
    fn run(&self, argv: &[String], timeout: Duration) -> Option<(i32, String)> {
        assert_eq!(timeout, TIMEOUT);
        assert!(argv_read_only(argv), "{argv:?}");
        self.calls.lock().unwrap().push(argv.to_vec());
        self.keychain.clone()
    }
}

fn scripted(keychain: Option<(i32, &str)>) -> Scripted {
    Scripted {
        calls: Mutex::new(Vec::new()),
        keychain: keychain.map(|(c, t)| (c, t.to_string())),
    }
}

fn agy(os: &'static str, installed: bool) -> (PathBuf, bool, &'static str) {
    (PathBuf::from("/h"), installed, os)
}

fn run_probe(
    run: &Scripted,
    (home, installed, os): (PathBuf, bool, &str),
    file: bool,
) -> BTreeMap<Tool, bool> {
    let src = Sources {
        codex: None,
        agy_installed: installed,
        os,
        home: &home,
    };
    probe(&src, run, &move |_| file)
}

#[test]
fn keychain_probe_never_reads_the_secret() {
    let argv = keychain_argv();
    assert_eq!(
        argv,
        [
            "/usr/bin/security",
            "find-generic-password",
            "-s",
            "gemini",
            "-a",
            "antigravity"
        ]
    );
    for word in &argv {
        assert!(word != "-w" && word != "-g", "{argv:?}");
    }
    assert!(argv_read_only(&argv));
    assert!(read_only(&keychain_command()));
    assert!(read_only(&format!(
        "if {} >/dev/null 2>&1; then echo @@keychain; fi",
        keychain_command()
    )));
    for bad in [
        "/usr/bin/security find-generic-password -s gemini -a antigravity -w",
        "/usr/bin/security find-generic-password -s gemini -a antigravity -g",
        "/usr/bin/security find-generic-password -w -s gemini -a antigravity",
        "/usr/bin/security find-generic-password -gw -s gemini -a antigravity",
        "/usr/bin/security find-generic-password -s gemini -a antigravity '-w'",
        "/usr/bin/security find-generic-password -s gemini -a antigravity -w>/dev/null",
        "/usr/bin/security find-generic-password -s gemini -a antigravity >/dev/null -w",
        "/usr/bin/security find-generic-password -s gemini -a antigravity-w",
        "/usr/bin/security find-generic-password -s gemini -a other",
        "/usr/bin/security find-generic-password -s other -a antigravity",
        "/usr/bin/security find-internet-password -s gemini -a antigravity",
        "/usr/bin/security add-generic-password -s gemini -a antigravity -w x",
        "/usr/bin/security delete-generic-password -s gemini -a antigravity",
        "/usr/bin/security dump-keychain",
        "/usr/bin/security unlock-keychain",
        "/usr/bin/security",
        "security find-generic-password -s gemini -a antigravity",
        "/tmp/security find-generic-password -s gemini -a antigravity",
        "echo security",
        "codex login status; /usr/bin/security find-generic-password -a antigravity -s gemini -w",
        "sudo /usr/bin/security find-generic-password -s gemini -a antigravity -g",
    ] {
        assert!(!read_only(bad), "{bad}");
        assert!(!argv_read_only(
            &bad.split(' ').map(String::from).collect::<Vec<_>>()
        ));
    }
}

#[test]
fn a_mac_keychain_item_is_a_login() {
    let run = scripted(Some((0, "keychain: \"login.keychain-db\"")));
    let got = run_probe(&run, agy("macos", true), false);
    assert_eq!(got.get(&Tool::Agy), Some(&true));
    assert_eq!(run.calls.lock().unwrap().as_slice(), [keychain_argv()]);
}

#[test]
fn a_mac_with_neither_file_nor_item_is_not_told() {
    for answer in [Some((44, "could not be found")), Some((1, "")), None] {
        let run = scripted(answer);
        let got = run_probe(&run, agy("macos", true), false);
        assert_eq!(got.get(&Tool::Agy), None, "{answer:?}");
        assert_eq!(run.calls.lock().unwrap().len(), 1);
    }
}

#[test]
fn a_login_file_wins_and_asks_no_keychain() {
    let run = scripted(Some((44, "")));
    let got = run_probe(&run, agy("macos", true), true);
    assert_eq!(got.get(&Tool::Agy), Some(&true));
    assert!(run.calls.lock().unwrap().is_empty());
}

#[test]
fn linux_and_uninstalled_tools_never_ask_the_keychain() {
    let run = scripted(Some((0, "")));
    let got = run_probe(&run, agy("linux", true), false);
    assert_eq!(got.get(&Tool::Agy), Some(&false));
    let got = run_probe(&run, agy("macos", false), false);
    assert!(got.is_empty());
    assert!(run.calls.lock().unwrap().is_empty());
}

#[test]
fn the_keychain_answer_is_only_the_exit_code() {
    assert_eq!(keychain_state(0), Some(true));
    for exit in [1, 44, 36, -1] {
        assert_eq!(keychain_state(exit), None);
    }
}
