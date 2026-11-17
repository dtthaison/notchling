use super::fakes_b::*;
use super::runner::Call;
use super::shell;
use super::step::{CheckState, StepId};
use super::tests_node_steps::{node_linux, node_mac, probe, step};
use crate::cluster::logins::{argv_read_only, keychain_command};

const AGY: StepId = StepId::NODE_AGY_LOGIN;

#[test]
fn mac_keychain_item_passes() {
    let rig = Rig::new(node_mac());
    let state = probe(&rig, &AGY, "@@missing\n@@keychain\n", 0);
    assert_eq!(state, CheckState::Pass("signed in (Keychain)".to_string()));
    let file = probe(&rig, &AGY, "@@present\n", 0);
    assert_eq!(file, CheckState::Pass("signed in".to_string()));
    let both = probe(&rig, &AGY, "@@present\n@@keychain\n", 0);
    assert_eq!(both, CheckState::Pass("signed in".to_string()));
}

#[test]
fn mac_without_file_or_item_cant_tell() {
    let rig = Rig::new(node_mac());
    for text in ["@@missing\n", "@@missing\nsecurity: not found\n"] {
        let state = probe(&rig, &AGY, text, 0);
        assert!(
            matches!(state, CheckState::CantTell(_)),
            "{text}: {state:?}"
        );
    }
    for text in ["@@none\n", "", "@@keychain\n"] {
        let state = probe(&rig, &AGY, text, 0);
        assert!(
            matches!(state, CheckState::CantTell(_)),
            "{text}: {state:?}"
        );
    }
}

#[test]
fn linux_ignores_keychain() {
    let rig = Rig::new(node_linux());
    let state = probe(&rig, &AGY, "@@missing\n@@keychain\n", 0);
    assert!(state.is_fail(), "{state:?}");
    let (script, _) = remote(&step(&rig.machine, &AGY));
    assert!(!script.contains("security"), "{script}");
}

#[test]
fn keychain_probe_never_reads_the_secret() {
    let m = node_mac();
    let (script, _) = remote(&step(&m, &AGY));
    assert!(script.contains(&keychain_command()), "{script}");
    assert!(script.contains("[ -s "), "{script}");
    assert!(script.find("[ -s ").unwrap() < script.find("security").unwrap());
    for word in script.split(|c: char| c.is_whitespace() || ";&|()".contains(c)) {
        assert!(word != "-w" && word != "-g", "{word}: {script}");
    }
    assert_eq!(script.matches("security").count(), 1, "{script}");
    assert!(script.contains(">/dev/null 2>&1"));
    let call = Call::secs(shell::ssh_script(&m, &script), 8);
    assert!(argv_read_only(&call.argv), "{:?}", call.argv);
    let rig = Rig::new(m.clone());
    let calls = rig.argvs();
    assert!(calls.is_empty());
}
