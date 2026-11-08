//! The worker.json step and the sd step order (Swift `MachinesSDWorkerStepsTests`).

use super::fakes_b::*;
use super::registry::Machine;
use super::runner::CommandResult;
use super::step::{CheckState, StepId};
use super::steps_sd::{is_at_least, MIN_SD};
use super::tests_sd::*;
use super::worker_json;
use serde_json::{json, Value};

// MARK: worker.json

fn owned_of(m: &Machine, all: &[Machine]) -> Result<serde_json::Map<String, Value>, String> {
    worker_json::owned(m, m.sd_worker().unwrap(), all)
}

#[test]
fn worker_json_owned_keys() {
    let (m1, vps) = (mac("m1"), linux("vps-sing-1"));
    let owned = owned_of(&vps, &[m1, vps.clone()]).unwrap();
    assert_eq!(owned["machine"], json!("vps-sing-1"));
    assert_eq!(owned["token_file"], json!("~/.config/agents/github-token"));
    assert_eq!(owned["claude_env"], json!("~/.config/agents/claude.env"));
    assert_eq!(owned["token_expires"], Value::Null);
    assert_eq!(owned["health_url"], Value::Null);
    let repos = owned["repos"].as_array().unwrap();
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0]["github"], json!(REPO));
    assert_eq!(
        repos[0]["kinds"],
        json!({"ios": "m1", "backend": "vps-sing-1"})
    );
    assert_eq!(repos[0]["path"], json!("~/src/lingeva-mono"));
    assert_eq!(repos[0]["check_cmd"], json!("true"));
    assert!(repos[0].get("verify_skill").is_none());
}

#[test]
fn worker_json_uses_the_repo_path_verify_skill_and_settings() {
    let mut vps = linux("vps-sing-1");
    let s = vps.sd_worker_mut().unwrap();
    s.repos[0].machine_path = Some("/srv/lingeva".into());
    s.repos[0].verify_skill = Some(".claude/skills/verify/SKILL.md".into());
    s.health_url = Some("https://x.example/health".into());
    s.token_expires = Some("2027-02-01".into());
    let owned = owned_of(&vps, std::slice::from_ref(&vps)).unwrap();
    assert_eq!(owned["health_url"], json!("https://x.example/health"));
    assert_eq!(owned["token_expires"], json!("2027-02-01"));
    let repos = owned["repos"].as_array().unwrap();
    assert_eq!(repos[0]["path"], json!("/srv/lingeva"));
    assert_eq!(
        repos[0]["verify_skill"],
        json!("/srv/lingeva/.claude/skills/verify/SKILL.md")
    );
}

const REMOTE_WITH_EXTRAS: &str = r#"
{"machine":"vps-sing-1","token_file":"~/.config/agents/github-token","claude_env":"~/.config/agents/claude.env",
 "token_expires":null,"health_url":"http://old",
 "budget":{"weekly_pct":50},
 "testflight":{"app_id":"1","key_id":"k","issuer_id":"i","p8":"~/k.p8","repo":"a/b"},
 "repos":[{"github":"dtthaison/lingeva-mono","path":"~/src/lingeva-mono","check_cmd":"true","writer_model":"opus",
           "kinds":{"ios":"m1","backend":"vps-sing-1"}}]}
"#;

#[test]
fn worker_json_keeps_keys_notchling_does_not_own() {
    let vps = linux("vps-sing-1");
    let owned = owned_of(&vps, &[mac("m1"), vps.clone()]).unwrap();
    assert_eq!(
        worker_json::differences(Some(REMOTE_WITH_EXTRAS), &owned),
        ["health_url"]
    );

    let data = worker_json::merged(Some(REMOTE_WITH_EXTRAS), &owned);
    let merged: Value = serde_json::from_slice(&data).unwrap();
    assert_eq!(merged["budget"]["weekly_pct"], json!(50));
    assert!(merged.get("testflight").is_some());
    assert_eq!(merged["health_url"], Value::Null);
    assert_eq!(merged["repos"][0]["writer_model"], json!("opus"));
    let text = String::from_utf8(data).unwrap();
    assert!(worker_json::differences(Some(&text), &owned).is_empty());
}

#[test]
fn worker_json_merged_is_sorted_and_pretty() {
    let vps = linux("vps-sing-1");
    let owned = owned_of(&vps, std::slice::from_ref(&vps)).unwrap();
    let text = String::from_utf8(worker_json::merged(None, &owned)).unwrap();
    assert!(text.ends_with('\n'));
    assert!(text.contains("\n  \"claude_env\""));
    assert!(text.find("claude_env").unwrap() < text.find("token_file").unwrap());
    assert!(
        text.contains("~/.config/agents/claude.env"),
        "slashes stay unescaped"
    );
    // a missing key equals null, so the two null keys are not differences
    assert_eq!(
        worker_json::differences(None, &owned),
        ["claude_env", "machine", "repos", "token_file"]
    );
}

#[test]
fn worker_json_differences_name_the_owned_keys_that_differ() {
    let mut vps = linux("vps-sing-1");
    vps.sd_worker_mut().unwrap().health_url = Some("https://x/h".into());
    let owned = owned_of(&vps, std::slice::from_ref(&vps)).unwrap();
    let remote = r#"
{"machine":"vps-sing-1","token_file":"~/.config/agents/github-token","claude_env":"~/.config/agents/claude.env",
 "repos":[{"github":"dtthaison/lingeva-mono","path":"~/src/lingeva-mono","check_cmd":"false","kinds":{"backend":"vps-sing-1"}}]}"#;
    assert_eq!(
        worker_json::differences(Some(remote), &owned),
        ["health_url", "repos"]
    );
}

#[test]
fn a_kind_claimed_by_two_machines_fails() {
    let mut m1 = mac("m1");
    m1.sd_worker_mut().unwrap().kinds = vec!["ios".into(), "backend".into()];
    let vps = linux("vps-sing-1");
    assert_eq!(
        owned_of(&vps, &[m1, vps.clone()]).unwrap_err(),
        format!("kind backend of {REPO} is claimed by m1 and vps-sing-1")
    );
}

#[test]
fn no_kinds_fails() {
    let mut vps = linux("vps-sing-1");
    vps.sd_worker_mut().unwrap().kinds.clear();
    assert_eq!(
        owned_of(&vps, std::slice::from_ref(&vps)).unwrap_err(),
        "no kinds chosen for vps-sing-1"
    );
}

#[test]
fn owned_refuses_what_sd_rejects() {
    let failure = |edit: &dyn Fn(&mut Machine)| {
        let mut vps = linux("vps-sing-1");
        edit(&mut vps);
        owned_of(&vps, std::slice::from_ref(&vps)).err()
    };
    assert_eq!(
        failure(&|m| m.sd_worker_mut().unwrap().repos.clear()).as_deref(),
        Some("no repos chosen for vps-sing-1")
    );
    assert_eq!(
        failure(&|m| m.sd_worker_mut().unwrap().repos[0].check_command = "  ".into()).as_deref(),
        Some(format!("no check command for {REPO}").as_str())
    );
    assert_eq!(
        failure(&|m| m.sd_worker_mut().unwrap().kinds = vec!["ios".into(), "web".into()])
            .as_deref(),
        Some("kind web must be ios or backend")
    );
    for name in ["VPS", "-vps", "vps_1", "vps sing", ""] {
        assert!(failure(&|m| m.name = name.into()).is_some(), "{name}");
    }
    assert!(failure(&|m| m.name = "vps-2".into()).is_none());
}

#[test]
fn worker_json_needs_absolute_python_and_sd_path() {
    let mut vps = linux("vps-sing-1");
    vps.sd_worker_mut().unwrap().python = "python3".into();
    assert_eq!(
        owned_of(&vps, std::slice::from_ref(&vps)).unwrap_err(),
        "python or sd.py is not an absolute path yet"
    );
}

#[test]
fn worker_json_probe_and_fix() {
    let vps = linux("vps-sing-1");
    assert_eq!(
        script(&StepId::WORKER_JSON, &vps),
        r#"head -c 262144 "$HOME/.config/sd/worker.json" 2>/dev/null"#
    );
    let owned = owned_of(&vps, std::slice::from_ref(&vps)).unwrap();
    let current = String::from_utf8(worker_json::merged(None, &owned)).unwrap();
    assert_eq!(
        check(&StepId::WORKER_JSON, &current).state,
        CheckState::Pass("current".into())
    );

    let stale = check(
        &StepId::WORKER_JSON,
        &current.replace("\"machine\": \"vps-sing-1\"", "\"machine\": \"other\""),
    );
    assert!(text(&stale).starts_with("differs: "), "{}", text(&stale));
    assert_eq!(
        check(&StepId::WORKER_JSON, "").state,
        CheckState::Fail("missing".into())
    );
    assert_eq!(
        check(&StepId::WORKER_JSON, "{not json").state,
        CheckState::Fail("not valid JSON".into())
    );

    // the Fix reads the file, merges, and writes the merged bytes on stdin (mode 600)
    let rig = Rig::new(vps.clone());
    rig.runner.on(
        script_has("head -c 262144"),
        CommandResult::ok(r#"{"budget":{"weekly_pct":50}}"#),
    );
    let outcome = (fix(&StepId::WORKER_JSON, &vps).run)(&rig.fix());
    assert!(outcome.ok, "{}", outcome.message);
    let calls = rig.calls();
    let write = calls.last().unwrap();
    assert!(last(&write.argv).contains("chmod 600"));
    let written: Value = serde_json::from_slice(write.stdin.as_ref().unwrap()).unwrap();
    assert_eq!(written["claude_env"], json!("~/.config/agents/claude.env"));
    assert!(written.get("budget").is_some());
}

#[test]
fn worker_json_fix_writes_a_new_file_when_the_machine_has_none() {
    let vps = linux("vps-sing-1");
    for code in [0, 1] {
        let rig = Rig::new(vps.clone());
        rig.runner.on(
            script_has("head -c 262144"),
            CommandResult::new(code, "", ""),
        );
        let outcome = (fix(&StepId::WORKER_JSON, &vps).run)(&rig.fix());
        assert!(outcome.ok, "{code}: {}", outcome.message);
        let calls = rig.calls();
        assert_eq!(calls.len(), 2, "read, then write");
        let written = worker_json::parse(&calls[1].stdin_text()).unwrap();
        assert_eq!(written["machine"], json!("vps-sing-1"));
    }
}

#[test]
fn a_read_error_is_never_treated_as_a_missing_file() {
    // exit 1 with nothing on stdout, but a reason on stderr: the file exists and can't be read
    for (code, err) in [
        (
            1,
            "head: /home/agent/.config/sd/worker.json: Permission denied\n",
        ),
        (0, "sh: Permission denied\n"),
    ] {
        let rig = Rig::new(linux("vps-sing-1"));
        rig.runner.on(
            script_has("head -c 262144"),
            CommandResult::new(code, "", err),
        );
        let outcome = (fix(&StepId::WORKER_JSON, &rig.machine).run)(&rig.fix());
        assert!(!outcome.ok, "{code}");
        assert!(
            outcome.message.contains("Permission denied"),
            "{}",
            outcome.message
        );
        assert_eq!(rig.calls().len(), 1, "the file was not overwritten");
    }
}

#[test]
fn worker_json_fix_shows_the_reason_of_a_real_read_error() {
    let rig = Rig::new(linux("vps-sing-1"));
    rig.runner.on(
        script_has("head -c 262144"),
        CommandResult::new(1, "", "head: I/O error\n"),
    );
    let outcome = (fix(&StepId::WORKER_JSON, &rig.machine).run)(&rig.fix());
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("head: I/O error"),
        "{}",
        outcome.message
    );
    assert_eq!(rig.calls().len(), 1);

    let ssh = Rig::new(linux("vps-sing-1"));
    ssh.runner.on(
        script_has("head -c 262144"),
        CommandResult::new(255, "", "ssh: connect timed out\n"),
    );
    let outcome = (fix(&StepId::WORKER_JSON, &ssh.machine).run)(&ssh.fix());
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("connect timed out"),
        "{}",
        outcome.message
    );
    assert_eq!(ssh.calls().len(), 1, "nothing was written");
}

#[test]
fn worker_json_fix_refuses_to_overwrite_a_file_it_cant_read() {
    let rig = Rig::new(linux("vps-sing-1"));
    rig.runner
        .on(script_has("head -c 262144"), CommandResult::ok("{broken"));
    let outcome = (fix(&StepId::WORKER_JSON, &rig.machine).run)(&rig.fix());
    assert!(!outcome.ok);
    assert_eq!(rig.calls().len(), 1, "nothing was written");
}

#[test]
fn worker_json_needs_the_plugins_token_login_and_each_clone() {
    let step = find(&steps(&linux("vps-sing-1"), None), &StepId::WORKER_JSON);
    assert_eq!(
        step.needs,
        vec![
            StepId::PLUGINS,
            StepId::ISSUES_TOKEN,
            StepId::CLAUDE_LOGIN,
            StepId::clone_repo(REPO)
        ]
    );
}

#[test]
fn the_sd_steps_come_in_swifts_order() {
    let all = steps(&mac("m1"), None);
    let names: Vec<&str> = all.iter().map(|s| s.id.raw()).collect();
    assert_eq!(
        names,
        ["plugins", "worker-json", "service", "sd-check", "running"]
    );
}

#[test]
fn is_at_least_compares_number_by_number() {
    assert!(is_at_least("1.19.3", "1.19.3"));
    assert!(is_at_least("1.20.0", "1.19.3"));
    assert!(is_at_least("2.0", "1.19.3"));
    assert!(is_at_least("1.19.10", "1.19.3"), "numbers, not text");
    assert!(!is_at_least("1.19.2", "1.19.3"));
    assert!(!is_at_least("1.9.9", "1.19.3"));
    assert!(!is_at_least("(unreadable version)", "1.19.3"));
    assert!(!is_at_least("", "1.19.3"));
    assert_eq!(MIN_SD, "1.19.3");
}
