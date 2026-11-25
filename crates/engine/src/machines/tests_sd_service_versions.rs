use super::fakes_b::*;
use super::service_files::service_vars;
use super::status::{Job, WorkerStatus};
use super::step::CheckState;
use super::steps_sd_service::{install_script, may_restart};
use super::tests_sd_service::*;
use crate::versions::knob::{AGY_VAR, CLAUDE_VAR};

const LINE: &str = "Environment=DISABLE_AUTOUPDATER=1";
const PLIST_PAIR: &str = "\n\t\t<key>DISABLE_AUTOUPDATER</key>\n\t\t<string>1</string>";

#[test]
fn unit_has_the_variable_beside_path() {
    let files = render_linux("agent", "/home/agent").unwrap();
    let service = files
        .iter()
        .find(|f| f.name == "sd-worker.service")
        .unwrap();
    let lines: Vec<&str> = service.content.split('\n').collect();
    let at = lines
        .iter()
        .position(|l| l.starts_with("Environment=PATH="))
        .unwrap();
    assert_eq!(lines[at + 1], LINE);
    assert_eq!(lines.iter().filter(|l| **l == LINE).count(), 1);
    for f in files.iter().filter(|f| !f.name.ends_with(".service")) {
        assert!(!f.content.contains(CLAUDE_VAR), "{}", f.name);
    }
}

#[test]
fn plist_has_the_variable() {
    let files = render_mac();
    let c = &files[0].content;
    let want = format!("</string>{PLIST_PAIR}\n\t</dict>");
    assert!(c.contains(&want), "{c}");
    assert_eq!(c.matches(CLAUDE_VAR).count(), 1);
}

#[test]
fn old_file_without_it_is_changed() {
    let tmp = TempDir::new("oldunit");
    let rig = rig_with_templates(linux("vps-sing-1"), tmp);
    let files = render_linux("agent", "/home/agent").unwrap();
    let old: Vec<String> = files
        .iter()
        .map(|f| {
            let text: Vec<&str> = f.content.split('\n').filter(|l| *l != LINE).collect();
            format!("{}  {}", digest(&text.join("\n")), f.remote_path)
        })
        .collect();
    assert_eq!(
        service_check(&rig, &old.join("\n")).state,
        CheckState::Fail("differs: sd-worker.service, sd-testflight.service".into())
    );

    let mac_rig = rig_with_templates(mac("m1"), TempDir::new("oldplist"));
    let plist = &render_mac()[0].content;
    let old_plist = plist.replace(PLIST_PAIR, "");
    assert_ne!(&old_plist, plist);
    let out = format!("{}  /x/com.sd.worker.plist\nloaded\n", digest(&old_plist));
    assert_eq!(
        service_check(&mac_rig, &out).state,
        CheckState::Fail("differs: com.sd.worker.plist".into())
    );
}

#[test]
fn restart_waits_for_no_job() {
    let idle = WorkerStatus::default();
    let busy = WorkerStatus {
        current: Some(Job {
            repo: "o/r".into(),
            issue: 4,
            ..Job::default()
        }),
        ..WorkerStatus::default()
    };
    assert!(may_restart(Some(&idle)));
    assert!(!may_restart(Some(&busy)));
    assert!(!may_restart(None));

    let files = render_linux("agent", "/home/agent").unwrap();
    let script = install_script(&files);
    for word in ["restart", "kickstart", "bootout", "enable", "start "] {
        assert!(!script.contains(word), "{word}");
    }
}

#[test]
fn agy_variable_only_when_verified() {
    assert_eq!(service_vars(false), vec![(CLAUDE_VAR, "1")]);
    assert_eq!(
        service_vars(true),
        vec![(CLAUDE_VAR, "1"), (AGY_VAR, "true")]
    );
    for f in render_linux("agent", "/home/agent").unwrap() {
        assert!(!f.content.contains(AGY_VAR), "{}", f.name);
    }
    assert!(!render_mac()[0].content.contains(AGY_VAR));
}
